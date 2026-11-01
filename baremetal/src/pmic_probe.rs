// SPDX-License-Identifier: GPL-2.0-only
// MT6768 WACS2 read protocol adapted from MediaTek mtk-pmic-wrap.c.
// Copyright (c) 2014 MediaTek Inc.; original author Flora Fu, MediaTek.
// Rust adaptation OpenAI GPT-6 Astra, 2026.
// Register provenance and exact restrictions: docs/SCREEN_BOARD6_HANDOFF.md.
use crate::{
    device_tree::{self, Event},
    usb_control::Io,
};

pub const BASE: usize = 0x1000_d000;
pub const INFRA: usize = 0x1000_1090;
pub const CMD: usize = BASE + 0xc20;
pub const RDATA: usize = BASE + 0xc24;
pub const CLEAR: usize = BASE + 0xc28;
pub const BUDGET: u32 = 32_768;
pub const INIT: u32 = 1 << 22;
pub const SYNC: u32 = 1 << 20;
pub const SNAPSHOTS: [usize; 11] = [
    INFRA,
    BASE,
    BASE + 4,
    BASE + 8,
    BASE + 0x6c,
    BASE + 0x7c,
    BASE + 0x9c,
    BASE + 0xa0,
    RDATA,
    BASE + 0xc0,
    BASE + 0xd0,
];
pub const REGISTERS: [u16; 33] = [
    0x040e, 0x0008, 0x000a, 0x2288, 0x2388, 0x238a, 0x238c, 0x238e, 0x2390, 0x2392, 0x2394, 0x2396,
    0x2398, 0x239a, 0x239c, 0x239e, 0x23a0, 0x2408, 0x240a, 0x240c, 0x240e, 0x2410, 0x2412, 0x2414,
    0x2416, 0x2418, 0x241a, 0x241c, 0x241e, 0x2420, 0x2422, 0x2424, 0x2426,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Status {
    NotRun = 0,
    Complete = 1,
    FdtMismatch = 2,
    Fault = 3,
    ClockOff = 4,
    Config = 5,
    Busy = 6,
    Timeout = 7,
    ReadTest = 8,
    Identity = 9,
    AckTimeout = 10,
}

#[derive(Clone, Copy, Default)]
struct Node<'a> {
    name: &'a str,
    compatible: &'a [u8],
    reg: &'a [u8],
    ac: Option<u32>,
    sc: Option<u32>,
    disabled: bool,
    unsupported: bool,
}
fn word(v: &[u8]) -> Option<u32> {
    Some(u32::from_be_bytes(v.try_into().ok()?))
}
fn has(v: &[u8], s: &[u8]) -> bool {
    v.last() == Some(&0) && v.split(|c| *c == 0).any(|x| x == s)
}
fn reg(v: &[u8], a: u32) -> bool {
    v.len() == 16
        && v.chunks_exact(4)
            .zip([0, a, 0, 0x1000])
            .all(|(x, w)| word(x) == Some(w))
}

/// Direct-root mappings only. Clocks are inherited; no clock-name/index guesses
/// from the vendor FDT's one-clock/two-name declaration are used to enable them.
pub fn matches(blob: &[u8]) -> Result<bool, device_tree::Error> {
    let mut stack = [Node::default(); 32];
    let mut depth = 0;
    let mut root = false;
    let mut seen = [0; 3];
    let mut good = [0; 3];
    device_tree::walk(blob, |e| match e {
        Event::Begin(name) => {
            stack[depth] = Node {
                name,
                disabled: depth > 0 && stack[depth - 1].disabled,
                ..Node::default()
            };
            depth += 1;
        }
        Event::Property(name, v) => {
            let n = &mut stack[depth - 1];
            match name {
                "compatible" => n.compatible = v,
                "reg" => n.reg = v,
                "#address-cells" => n.ac = word(v),
                "#size-cells" => n.sc = word(v),
                "status" => n.disabled |= v != b"okay\0" && v != b"ok\0",
                "ranges" | "power-domains" => n.unsupported = true,
                _ => {}
            }
        }
        Event::End => {
            depth -= 1;
            let n = stack[depth];
            if depth == 0 {
                root = !n.disabled
                    && !n.unsupported
                    && n.ac == Some(2)
                    && n.sc == Some(2)
                    && (has(n.compatible, b"mediatek,MT6768")
                        || has(n.compatible, b"mediatek,mt6768"));
            }
            let candidate = match (depth, n.name) {
                (1, "infracfg_ao@10001000") => Some((
                    0,
                    has(n.compatible, b"mediatek,infracfg_ao") && reg(n.reg, 0x10001000),
                )),
                (1, "pwrap@1000d000") => Some((
                    1,
                    has(n.compatible, b"mediatek,mt6768-pwrap") && reg(n.reg, 0x1000d000),
                )),
                (2, "mt6358-pmic") if stack[1].name == "pwrap@1000d000" => {
                    Some((2, has(n.compatible, b"mediatek,mt6358")))
                }
                _ => None,
            };
            if let Some((i, valid)) = candidate {
                seen[i] += 1;
                good[i] += u32::from(valid && !n.disabled && !n.unsupported);
            }
        }
    })?;
    Ok(root && seen == [1; 3] && good == [1; 3])
}

fn fsm(v: u32) -> u32 {
    (v >> 16) & 7
}
fn idle(v: u32) -> bool {
    fsm(v) == 0 && v & SYNC != 0 && v & (1 << 19) == 0
}

/// Five words: status, raw value, last RDATA, response polls, ack-idle polls.
/// Never consumes an inherited response or submits a PMIC write command.
fn read(io: &mut impl Io, address: u16, out: &mut [u32]) -> Status {
    let before = io.read32(RDATA);
    if io.faulted() {
        return Status::Fault;
    }
    out[2] = before;
    if before & INIT == 0 {
        return Status::Config;
    }
    if !idle(before) {
        return Status::Busy;
    }
    io.write32(CMD, u32::from(address >> 1) << 16);
    if io.faulted() {
        return Status::Fault;
    }
    for polls in 1..=BUDGET {
        let v = io.read32(RDATA);
        if io.faulted() {
            return Status::Fault;
        }
        out[2] = v;
        out[3] = polls;
        if v & INIT == 0 {
            return Status::Config;
        }
        if fsm(v) == 6 {
            out[1] = v & 0xffff;
            io.write32(CLEAR, 1);
            if io.faulted() {
                return Status::Fault;
            }
            for ack in 1..=BUDGET {
                let end = io.read32(RDATA);
                if io.faulted() {
                    return Status::Fault;
                }
                out[4] = ack;
                if end & INIT == 0 {
                    return Status::Config;
                }
                if idle(end) {
                    return Status::Complete;
                }
            }
            return Status::AckTimeout;
        }
    }
    Status::Timeout
}

/// Header32 words: status, profile, budget, last-observed idle; 11 snapshots,
/// final RDATA, then reserved zeros. Entries33*5, initialized by Board6.
pub fn run(io: &mut impl Io, blob: &[u8], header: &mut [u32], entries: &mut [u32]) {
    assert_eq!(header.len(), 32);
    assert_eq!(entries.len(), REGISTERS.len() * 5);
    header.fill(0);
    header[2] = BUDGET;
    header[4..16].fill(u32::MAX);
    let status = probe(io, blob, header, entries);
    header[0] = if io.faulted() {
        Status::Fault as u32
    } else {
        status as u32
    };
}
fn probe(io: &mut impl Io, blob: &[u8], h: &mut [u32], entries: &mut [u32]) -> Status {
    if io.faulted() {
        return Status::Fault;
    }
    if !matches(blob).unwrap_or(false) {
        return Status::FdtMismatch;
    }
    h[1] = 1;
    for (i, a) in SNAPSHOTS.into_iter().enumerate() {
        let v = io.read32(a);
        if io.faulted() {
            return Status::Fault;
        }
        h[4 + i] = v;
        if i == 0 && v & 2 != 0 {
            return Status::ClockOff;
        }
    }
    // MUX=wrapper, WRAP_EN/WACS2_EN/INIT_DONE2=1, manual disabled.
    if h[5] != 0 || h[6] != 1 || h[9] != 0 || h[10] != 1 || h[11] != 1 || h[12] & INIT == 0 {
        return Status::Config;
    }
    h[15] = h[12];
    h[3] = u32::from(idle(h[12]));
    if h[3] == 0 {
        return Status::Busy;
    }
    for (i, address) in REGISTERS.into_iter().enumerate() {
        io.keepalive();
        h[3] = 0; // no claim of quiescence until the final status read
        let out = &mut entries[i * 5..i * 5 + 5];
        let status = read(io, address, out);
        out[0] = status as u32;
        if io.faulted() {
            return Status::Fault;
        }
        let end = io.read32(RDATA);
        if io.faulted() {
            return Status::Fault;
        }
        h[15] = end;
        h[3] = u32::from(end & INIT != 0 && idle(end));
        if status != Status::Complete {
            return status;
        }
        if h[3] == 0 {
            return Status::Busy;
        }
        if i == 0 && out[1] != 0x5aa5 {
            return Status::ReadTest;
        }
        // Kernel MFD identifies MT6358 by SWCID high byte, not literal 0x6358.
        if i == 2 && out[1] >> 8 != 0x58 {
            return Status::Identity;
        }
    }
    Status::Complete
}
