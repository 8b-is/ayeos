// SPDX-License-Identifier: GPL-2.0-only
// MT6768 register sequence adapted from MediaTek's GPL-2.0 USB/clock drivers.
// Copyright (C) 2015-2019 MediaTek Inc.; Rust diagnostic adaptation 2026 OpenAI GPT-6 Astra.
// Pinned sources and scope: docs/SCREEN_BOARD4_HANDOFF.md. No Linux runtime.
use crate::usb_control::Io;
pub const MUSB: usize = 0x1120_0000;
pub const PHY: usize = 0x11cc_0800;
pub const INFRA: usize = 0x1000_1000;
pub const TOP: usize = 0x1000_0000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Init {
    Ready,
    Attached,
    FdtMismatch,
    Fault,
    ClockReadback,
    ControllerAbsent,
    PhyReadback,
    ConnectReadback,
}
fn update(io: &mut impl Io, addr: usize, clear: u32, set: u32) {
    let old = io.read32(addr);
    if !io.faulted() {
        io.write32(addr, (old & !clear) | set);
    }
}
pub fn initialize(io: &mut impl Io, matched: bool) -> Init {
    if !matched {
        return Init::FdtMismatch;
    }
    if io.faulted() {
        return Init::Fault;
    }
    // Enable only the two known gates. Preserve the bootloader's parent/PLL.
    // CLK_CFG_5 bit23 gate, update bit22; infra MODULE_SW_CG_0 ICUSB bit8.
    io.write32(TOP + 0x98, 1 << 23);
    io.write32(TOP + 4, 1 << 22);
    io.write32(INFRA + 0x84, 1 << 8);
    io.settle();
    let clocks = io.read32(TOP + 0x90) & (1 << 23) | io.read32(INFRA + 0x90) & (1 << 8);
    if io.faulted() {
        return Init::Fault;
    }
    if clocks != 0 {
        return Init::ClockReadback;
    }
    // Fixed full-speed operation avoids claiming calibrated high-speed PHY.
    io.write8(MUSB + 1, 0); // detach before changing PHY
    let version = io.read16(MUSB + 0x6c);
    if io.faulted() {
        return Init::Fault;
    }
    if version == 0 || version == 0xffff {
        return Init::ControllerAbsent;
    }
    io.settle(); // conservative instruction-count settling, not a measured us clock
    update(io, PHY + 0x68, (1 << 26) | (1 << 18), 0);
    update(io, PHY + 0x6c, 1 << 16, 0);
    update(io, PHY, 0, 1 << 5); // USB20_INTR_EN
    update(io, PHY + 0x14, 1 << 11, 0); // disable HS_100U_U3
    update(
        io,
        PHY + 0x20,
        (1 << 9) | (1 << 8) | (1 << 17) | (1 << 16),
        1 << 18,
    );
    // Release save-current/test overrides; PHY handles the wire protocol.
    update(
        io,
        PHY + 0x68,
        (3 << 4)
            | (15 << 10)
            | (1 << 23)
            | (3 << 20)
            | (1 << 19)
            | (1 << 18)
            | (1 << 17)
            | (3 << 6)
            | (1 << 2),
        0,
    );
    update(
        io,
        PHY + 0x18,
        (1 << 23) | (0xff << 24),
        (1 << 20) | (0x40 << 24),
    );
    // Force device role for this USB-C laptop-connected diagnostic, never host/VBUS drive.
    update(io, PHY + 0x6c, 0x10, 0x2f | (0x3f << 8));
    io.settle();
    if io.faulted() {
        return Init::Fault;
    }
    let dtm0 = io.read32(PHY + 0x68);
    let dtm1 = io.read32(PHY + 0x6c);
    if io.faulted() {
        return Init::Fault;
    }
    if dtm0 & ((1 << 26) | (1 << 18)) != 0 || dtm1 & ((1 << 16) | 0x10) != 0 {
        return Init::PhyReadback;
    }
    io.write8(MUSB + 0x0e, 0);
    io.write8(MUSB, 0);
    io.write16(MUSB + 0x12, 0x100); // flush EP0, not endpoint FIFOs elsewhere
    io.write16(MUSB + 6, 1);
    io.write16(MUSB + 8, 0); // latch EP0 only; CPU IRQs remain masked
    io.write8(MUSB + 0x0b, 4); // bus reset latch
    let _ = io.read8(MUSB + 0x0a);
    let _ = io.read16(MUSB + 2);
    if io.faulted() {
        return Init::Fault;
    }
    Init::Ready
}
/// Call after rendering the initial screen; enter the service loop immediately.
pub fn connect(io: &mut impl Io) -> Init {
    if io.faulted() {
        return Init::Fault;
    }
    io.write8(MUSB + 1, 0x40); // SOFTCONN, HSENAB clear
    let power = io.read8(MUSB + 1);
    if io.faulted() {
        Init::Fault
    } else if power & 0x60 != 0x40 {
        Init::ConnectReadback
    } else {
        Init::Attached
    }
}
