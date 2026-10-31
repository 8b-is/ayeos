// Included into main.rs so the MMIO boundary stays beside existing primitives.
#[cfg(feature = "usb-report")]
fn fault_count() -> u64 {
    unsafe { read_volatile(core::ptr::addr_of!(AYEOS_FAULT_COUNT)) }
}

#[cfg(feature = "usb-report")]
fn usb_barrier() {
    unsafe {
        asm!("dsb sy", options(nostack, preserves_flags));
    }
}

#[cfg(feature = "usb-report")]
struct UsbIo {
    start_faults: u64,
}
#[cfg(feature = "usb-report")]
impl usb_control::Io for UsbIo {
    fn faulted(&self) -> bool {
        fault_count() != self.start_faults
    }
    fn read8(&mut self, a: usize) -> u8 {
        if self.faulted() {
            0
        } else {
            let v = safe_mmio_read8(a);
            usb_barrier();
            v
        }
    }
    fn read16(&mut self, a: usize) -> u16 {
        if self.faulted() {
            0
        } else {
            let v = safe_mmio_read16(a);
            usb_barrier();
            v
        }
    }
    fn read32(&mut self, a: usize) -> u32 {
        if self.faulted() {
            0
        } else {
            let v = safe_mmio_read32(a);
            usb_barrier();
            v
        }
    }
    fn write8(&mut self, a: usize, v: u8) {
        if !self.faulted() {
            safe_mmio_write8(a, v);
            usb_barrier();
        }
    }
    fn write16(&mut self, a: usize, v: u16) {
        if !self.faulted() {
            safe_mmio_write16(a, v);
            usb_barrier();
        }
    }
    fn write32(&mut self, a: usize, v: u32) {
        if !self.faulted() {
            unsafe {
                write_volatile(a as *mut u32, v);
            }
            usb_barrier();
        }
    }
    fn settle(&mut self) {
        // At least 4M NOP instructions. No calibrated time claim. Used ONLY
        // before attachment, never in the USB service loop.
        for _ in 0..4 {
            pet_mediatek_watchdog();
            for _ in 0..1_000_000 {
                unsafe {
                    asm!("nop", options(nomem, nostack, preserves_flags));
                }
            }
        }
    }
}

#[cfg(feature = "usb-report")]
fn screen_usb(address: u64, base: usize, stride: usize) -> ! {
    use core::fmt::Write;
    use usb_control::Io;
    clear_board_page(base, stride);
    board_line(
        base,
        stride,
        0,
        "AYEOS studio-board4 / USB EP0 READBACK (not CDC serial)",
    );
    board_line(
        base,
        stride,
        1,
        "Full-speed USB-C; host requests only. No updater or radio commands.",
    );
    let initial_faults = fault_count();
    let mut line = board_screen::Line::new();
    let _ = write!(
        line,
        "x0={:016x} framebuffer={:016x} stride={}",
        address, base, stride
    );
    board_line(base, stride, 2, line.text());
    board_line(
        base,
        stride,
        3,
        "Stage: validate boot FDT (readability still requires bootloader ABI)",
    );
    flush_dcache_range(base, stride * SCREEN_H * 4);
    let blob = if (0x4000_0000..=0xffff_ffd8).contains(&address) && address & 7 == 0 {
        let magic = u32::from_be(safe_mmio_read32(address as usize));
        let size = u32::from_be(safe_mmio_read32(address as usize + 4)) as usize;
        if magic == 0xd00d_feed
            && (40..=device_tree::MAX_BYTES).contains(&size)
            && address + size as u64 <= 0x1_0000_0000
            && fault_count() == initial_faults
        {
            Some(unsafe { core::slice::from_raw_parts(address as *const u8, size) })
        } else {
            None
        }
    } else {
        None
    };
    if let Some(blob) = blob {
        if let Ok(board) = usb_board::discover(blob) {
            if fault_count() != initial_faults {
                board_line(
                    base,
                    stride,
                    4,
                    "STOP: a skipped fault occurred during FDT discovery",
                );
            } else {
                let mut line = board_screen::Line::new();
                let _ = write!(
                    line,
                    "FDT bytes={} CRC32={:08x} parse OK",
                    blob.len(),
                    board_pages::crc32(blob)
                );
                board_line(base, stride, 3, line.text());
                let mut line = board_screen::Line::new();
                let _ = write!(
                    line,
                    "FDT match soc={} musb={} phy={} infra={} top={}",
                    board.soc, board.controller, board.phy, board.infra, board.top
                );
                board_line(base, stride, 4, line.text());
                board_line(
                    base,
                    stride,
                    5,
                    "Stage: bounded clock/PHY init; rails and PLL parents inherited from LK",
                );
                // Draw the fallback BEFORE attempting controller attachment.
                let mut row = 18;
                usb_board::summary(blob, |s| {
                    board_line(base, stride, row, s);
                    row += 1;
                });
                board_line(
                    base,
                    stride,
                    69,
                    "Capture this ONE page if no USB device appears. Keep raw FDT private.",
                );
                board_line(
                    base,
                    stride,
                    70,
                    "Expected ID 18d1:4ee0 bcd0544; use capture_board4.py, not an OTA daemon",
                );
                board_line(
                    base,
                    stride,
                    71,
                    "~ marks shortened fallback fields; the requested USB dump is complete.",
                );
                flush_dcache_range(base, stride * SCREEN_H * 4);
                let mut io = UsbIo {
                    start_faults: initial_faults,
                };
                let mut device = usb_control::Device::new(blob); // CRC computed before attach
                let mut init = usb_phy::initialize(&mut io, board.ready());
                let mut line = board_screen::Line::new();
                let _ = write!(line, "INIT={:?} skipped faults={}", init, fault_count());
                board_line(base, stride, 5, line.text());
                // Snapshot non-destructive status registers, only with matched FDT.
                if board.ready() {
                    let p = io.read8(usb_phy::MUSB + 1);
                    let d = io.read8(usb_phy::MUSB + 0x60);
                    let h = io.read16(usb_phy::MUSB + 0x6c);
                    let c = io.read16(usb_phy::MUSB + 0x12);
                    let mut line = board_screen::Line::new();
                    let _ = write!(
                        line,
                        "MUSB POWER={:02x} DEVCTL={:02x} HWVERS={:04x} CSR0={:04x}",
                        p, d, h, c
                    );
                    board_line(base, stride, 6, line.text());
                    let a = io.read32(usb_phy::PHY + 0x18);
                    let b = io.read32(usb_phy::PHY + 0x68);
                    let c = io.read32(usb_phy::PHY + 0x6c);
                    let mut line = board_screen::Line::new();
                    let _ = write!(
                        line,
                        "U2PORT=11cc0800 ACR6={:08x} DTM0={:08x} DTM1={:08x}",
                        a, b, c
                    );
                    board_line(base, stride, 7, line.text());
                    let a = io.read32(usb_phy::INFRA + 0x90);
                    let b = io.read32(usb_phy::TOP + 0x90);
                    let mut line = board_screen::Line::new();
                    let _ = write!(
                        line,
                        "INFRA_CG0={:08x} TOP_CFG5={:08x}; regulator setup NOT verified",
                        a, b
                    );
                    board_line(base, stride, 8, line.text());
                }
                flush_dcache_range(base, stride * SCREEN_H * 4);
                if init == usb_phy::Init::Ready {
                    init = usb_phy::connect(&mut io);
                }
                let mut prior = (u8::MAX, u8::MAX, u32::MAX, false);
                loop {
                    pet_mediatek_watchdog();
                    if init == usb_phy::Init::Attached {
                        device.poll(&mut io, usb_phy::MUSB);
                    }
                    let state = (
                        device.control.address,
                        device.control.configuration,
                        device.resets,
                        io.faulted(),
                    );
                    if state != prior {
                        prior = state;
                        let mut line = board_screen::Line::new();
                        let _ = write!(
                            line,
                            "{:?} addr={} cfg={} resets={} fault={}",
                            init, state.0, state.1, state.2, state.3
                        );
                        board_line(base, stride, 10, line.text());
                        flush_dcache_range(base, stride * SCREEN_H * 4);
                    }
                    // No pacer, timer wait, keypad action, full-screen repaint or logging.
                }
            }
        } else {
            board_line(
                base,
                stride,
                4,
                "STOP: malformed FDT; no USB initialization attempted",
            );
        }
    } else {
        board_line(
            base,
            stride,
            4,
            "STOP: invalid FDT header/pointer or skipped read fault",
        );
    }
    flush_dcache_range(base, stride * SCREEN_H * 4);
    loop {
        pet_mediatek_watchdog();
        core::hint::spin_loop();
    }
}
