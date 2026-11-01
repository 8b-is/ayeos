// SPDX-License-Identifier: GPL-2.0-only
// MediaTek MT6768 I2C v2 FIFO sequence adapted from i2c-mt65xx.c;
// Copyright (c) 2014 MediaTek Inc.; original I2C author Xudong Chen.
// Pin definitions: Copyright (C) 2018 MediaTek Inc.; Light Hsieh.
// Clock definitions from MediaTek's GPL MT6768 driver.
// Rust diagnostic adaptation: OpenAI GPT-6 Astra, 2026.
// Pinned references and limits: docs/SCREEN_BOARD5_HANDOFF.md.
use crate::{audio_board, board_pages::crc32, usb_control::Io};

pub const BASE: usize = 0x1100_4000;
pub const TOP: usize = 0x1000_00a0; // CLK_CFG_6, i2c_sel in bits 0..2, gate bit7
pub const INFRA: usize = 0x1000_10ac; // MODULE_SW_CG_4, I2C4 bit7
pub const PIN: usize = 0x1000_53d0; // GPIO104..111; 105/106 mode1 = SCL4/SDA4
pub const BUDGET: u32 = 32_768; // work budget, NOT milliseconds
pub const BYTES: usize = 192;
// AP FIFO_STAT: count in bits7:4, not the low status flags. The low-five-bit
// mask in the newer reference is used for its SCP channel, not this AP FIFO.
fn fifo_count(status: u16) -> u16 {
    (status >> 4) & 0x0f
}
// Saved configuration (not FIFO data, START, W1C status, or write-only strobes).
pub const OFFSETS: [usize; 17] = [
    0x04, 0x08, 0x10, 0x14, 0x18, 0x1c, 0x20, 0x28, 0x2c, 0x30, 0x34, 0x3c, 0x44, 0x48, 0x90, 0xe8,
    0xf8,
];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Status {
    NotRun = 0,
    Complete = 1,
    FdtMismatch = 2,
    Fault = 3,
    ClockOff = 4,
    Pins = 5,
    Busy = 6,
    Config = 7,
    Timeout = 8,
    Nack = 9,
    Arbitration = 10,
    Controller = 11,
    Fifo = 12,
    Restore = 13,
}

pub struct Report {
    pub words: [u32; BYTES / 4],
}
impl Report {
    pub fn new(blob: &[u8]) -> Self {
        let mut words = [u32::MAX; BYTES / 4];
        words[..8].copy_from_slice(&[
            u32::from_le_bytes(*b"A5RP"),
            1,
            BYTES as u32,
            crc32(blob),
            Status::NotRun as u32,
            0,
            BUDGET,
            0,
        ]);
        // Results are [status, raw ID, IRQ, FIFO count, polls] per address.
        words[32] = Status::NotRun as u32;
        words[37] = Status::NotRun as u32;
        words[42] = 0; // config restored/read back
        words[43..].fill(0);
        Self { words }
    }
    pub fn encode(&self) -> [u8; BYTES] {
        let mut b = [0; BYTES];
        for (dst, w) in b.chunks_exact_mut(4).zip(self.words) {
            dst.copy_from_slice(&w.to_le_bytes());
        }
        b
    }
}

/// Before USB attachment: two fixed register-zero reads, never arbitrary I2C.
/// Requires a configured, idle LK bus. Does not retune shared parent clocks,
/// configure GPIOs, enable DMA, write amplifier values or touch charger addresses.
pub fn run(io: &mut impl Io, blob: &[u8]) -> Report {
    run_inner(io, blob, &mut [])
}

/// Board6 fixed window, one combined byte read per register, same restoration.
/// Caller initializes 256 five-word entries to NotRun + unobserved values.
pub fn run_window(io: &mut impl Io, blob: &[u8], window: &mut [u32]) -> Report {
    assert_eq!(window.len(), 256 * 5);
    run_inner(io, blob, window)
}

fn run_inner(io: &mut impl Io, blob: &[u8], window: &mut [u32]) -> Report {
    let mut r = Report::new(blob);
    if !audio_board::matches(blob).unwrap_or(false) {
        r.words[4] = Status::FdtMismatch as u32;
        return r;
    }
    r.words[5] = 1;
    let status = probe(io, &mut r, window);
    r.words[4] = if io.faulted() {
        Status::Fault as u32
    } else {
        status as u32
    };
    r
}

fn probe(io: &mut impl Io, r: &mut Report, window: &mut [u32]) -> Status {
    for (i, address) in [TOP, INFRA, PIN].into_iter().enumerate() {
        let value = io.read32(address);
        if io.faulted() {
            return Status::Fault;
        }
        r.words[8 + i] = value;
    }
    // Inherit the clocks; an off gate is a diagnostic result, not permission to
    // retune a shared bus that also carries the charger.
    if r.words[8] & 0x80 != 0 || r.words[9] & 0x80 != 0 {
        return Status::ClockOff;
    }
    for (i, offset) in [0x24, 0x0c, 0xf4, 0xe4].into_iter().enumerate() {
        let value = io.read16(BASE + offset);
        if io.faulted() {
            return Status::Fault;
        }
        r.words[11 + i] = u32::from(value);
    }
    // Snapshot configuration before bus/pin/config rejection. Clock-off or an
    // MMIO fault still stops reads; never manufacture observations in that case.
    let mut saved = [0u16; 17];
    for (i, &offset) in OFFSETS.iter().enumerate() {
        saved[i] = io.read16(BASE + offset);
        if io.faulted() {
            return Status::Fault;
        }
        r.words[15 + i] = u32::from(saved[i]);
    }
    if io.faulted() {
        return Status::Fault;
    }
    if (r.words[10] >> 4) & 0xff != 0x11 {
        return Status::Pins;
    }
    if r.words[11] & 1 != 0 || fifo_count(r.words[13] as u16) != 0 {
        return Status::Busy;
    }
    // TIMING nonzero, HS disabled, open-drain selected. Rate is inherited and
    // reported, not claimed to equal the FDT's requested 400kHz.
    if saved[6] == 0 || saved[9] & 1 != 0 || saved[10] & 3 != 3 {
        return Status::Config;
    }
    let mut result = Status::Complete;
    'addresses: for (i, address) in [0x58u16, 0x59].into_iter().enumerate() {
        let at = 32 + i * 5;
        let count = if window.is_empty() { 1 } else { 128 };
        for register in 0..count {
            if !window.is_empty() {
                io.keepalive();
            }
            let mut entry = [u32::MAX; 5];
            let status = read_register(io, address, register as u8, &mut entry);
            entry[0] = if io.faulted() {
                Status::Fault as u32
            } else {
                status as u32
            };
            if register == 0 {
                r.words[at..at + 5].copy_from_slice(&entry);
            }
            if !window.is_empty() {
                let start = (i * 128 + register) * 5;
                window[start..start + 5].copy_from_slice(&entry);
            }
            if io.faulted() {
                return Status::Fault;
            }
            if status != Status::Complete {
                result = status;
                break 'addresses;
            }
        }
    }
    // A failed transfer is reset before restoring saved configuration. No bus
    // recovery pulses, retries, chip resets or guessed amplifier profiles.
    if result != Status::Complete {
        io.write16(BASE + 0x50, 1);
    }
    let mut idle = false;
    for _ in 0..256 {
        if io.read16(BASE + 0x24) & 1 == 0 {
            idle = true;
            break;
        }
    }
    if io.faulted() {
        return Status::Fault;
    }
    if !idle {
        return Status::Restore;
    }
    io.write16(BASE + 0x38, 1);
    let irq = io.read16(BASE + 0x0c);
    if irq != 0 {
        io.write16(BASE + 0x0c, irq);
    }
    for (&offset, &value) in OFFSETS.iter().zip(&saved) {
        // Restore interrupt mask last; CPU interrupts remain masked throughout.
        if offset != 8 {
            io.write16(BASE + offset, value);
        }
    }
    io.write16(BASE + 8, saved[1]);
    let mut restored = true;
    for (&offset, &value) in OFFSETS.iter().zip(&saved) {
        restored &= io.read16(BASE + offset) == value;
    }
    if io.faulted() {
        return Status::Fault;
    }
    r.words[42] = u32::from(restored);
    if !restored {
        Status::Restore
    } else {
        result
    }
}

fn read_register(io: &mut impl Io, address: u16, register: u8, out: &mut [u32]) -> Status {
    io.write16(BASE + 8, 0); // polled status, no interrupt delivery
    io.write16(BASE + 0x10, 0xba); // ACKERR, stretch, IRQ_SEL, direction change, RS; no DMA
    io.write16(BASE + 4, address << 1);
    let irq = io.read16(BASE + 0xc);
    if irq != 0 {
        io.write16(BASE + 0xc, irq);
    }
    io.write16(BASE + 0x38, 1);
    io.write16(BASE + 0x14, 1);
    io.write16(BASE + 0x44, 1);
    io.write16(BASE + 0x18, 2);
    io.write8(BASE, register); // register address only; no register-value write
    io.write16(BASE + 0x24, 1);
    for polls in 1..=BUDGET {
        let irq = io.read16(BASE + 0xc);
        out[4] = polls;
        if io.faulted() {
            return Status::Fault;
        }
        out[2] = u32::from(irq);
        if irq != 0 {
            io.write16(BASE + 0xc, irq);
        }
        if io.faulted() {
            return Status::Fault;
        }
        if irq & 0x200 != 0 {
            return Status::Controller;
        }
        if irq & 8 != 0 {
            return Status::Arbitration;
        }
        if irq & 6 != 0 {
            return Status::Nack;
        }
        if irq & 1 != 0 {
            let fifo = fifo_count(io.read16(BASE + 0xf4));
            if io.faulted() {
                return Status::Fault;
            }
            out[3] = u32::from(fifo);
            if out[3] != 1 {
                return Status::Fifo;
            }
            let id = io.read8(BASE);
            if io.faulted() {
                return Status::Fault;
            }
            out[1] = u32::from(id);
            // Completion IRQ may precede START readback clearing. Do not arm
            // the next address while this transaction still owns the bus.
            for _ in 0..256 {
                if io.read16(BASE + 0x24) & 1 == 0 {
                    return Status::Complete; // raw ID, not profile validation
                }
            }
            return Status::Timeout;
        }
    }
    Status::Timeout
}
