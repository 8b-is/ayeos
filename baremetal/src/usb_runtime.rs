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
            #[cfg(feature = "alpha-report")]
            safe_mmio_write32(a, v);
            #[cfg(not(feature = "alpha-report"))]
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
    fn keepalive(&mut self) {
        self.write32(MTK_WDT_RESTART, 0x1971);
    }
}

#[cfg(feature = "usb-report")]
fn screen_usb(address: u64, base: usize, stride: usize) -> ! {
    use core::fmt::Write;
    use usb_control::Io;
    clear_board_page(base, stride);
    #[cfg(not(feature = "audio-report"))]
    let banner = "AYEOS studio-board4-r3 / USB EP0 READBACK (not CDC serial)";
    #[cfg(all(feature = "audio-report", not(feature = "board6-report")))]
    let banner = "AYEOS studio-board5-r2 / SmartPA ID probe + USB EP0";
    #[cfg(all(feature = "board6-report", not(feature = "alpha-report")))]
    let banner = "AYEOS studio-board6-r2 / inherited PA windows + MT6358 readback";
    #[cfg(all(feature = "alpha-report", not(feature = "tone-report")))]
    let banner = "AYEOS audio-alpha1 / fault records + genuine Marine fixture";
    #[cfg(feature = "tone-report")]
    let banner = "AYEOS audio-tone1 / bounded SGEN + explicit Off readback";
    board_line(base, stride, 0, banner);
    board_line(
        base,
        stride,
        1,
        "Full-speed USB-C; host requests only. No updater or radio commands.",
    );
    #[cfg(feature = "alpha-report")]
    let alpha_tests = alpha_selftests(base, stride);
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
                let mut line = board_screen::Line::new();
                let _ = write!(
                    line,
                    "Seen PHY v1={} v2={} other USB/PHY bindings={}",
                    board.phy_v1_seen, board.phy_v2_seen, board.other_bindings_seen
                );
                board_line(base, stride, 12, line.text());
                board_line(
                    base,
                    stride,
                    13,
                    "MMIO requires verified v1 layout; v2 COM=port+300, not port+0",
                );
                board_line(
                    base,
                    stride,
                    5,
                    "Stage: bounded clock/PHY init; rails and PLL parents inherited from LK",
                );
                // Draw the fallback BEFORE attempting controller attachment.
                #[cfg(not(feature = "board6-report"))]
                {
                    let mut row = 18;
                    usb_board::summary(blob, |s| {
                        board_line(base, stride, row, s);
                        row += 1;
                    });
                }
                board_line(
                    base,
                    stride,
                    69,
                    "Capture this ONE page if no USB device appears. Keep raw FDT private.",
                );
                #[cfg(not(feature = "audio-report"))]
                board_line(
                    base,
                    stride,
                    70,
                    "Expected ID 18d1:4ee0 bcd0544; use capture_board4.py, not an OTA daemon",
                );
                #[cfg(all(feature = "audio-report", not(feature = "board6-report")))]
                board_line(
                    base,
                    stride,
                    70,
                    "ID 18d1:4ee0 bcd0545; capture_board5.py; no tone/profile enabled",
                );
                #[cfg(all(feature = "board6-report", not(feature = "alpha-report")))]
                board_line(
                    base,
                    stride,
                    70,
                    "ID 18d1:4ee0 bcd0546; capture_board6.py; inherited state, not POR",
                );
                #[cfg(all(feature = "alpha-report", not(feature = "tone-report")))]
                board_line(
                    base,
                    stride,
                    70,
                    "ID 18d1:4ee0 bcd0561; capture_alpha.py; keep raw evidence private",
                );
                #[cfg(feature = "tone-report")]
                board_line(
                    base,
                    stride,
                    70,
                    "ID 18d1:4ee0 bcd0562; capture_tone.py; keep raw evidence private",
                );
                board_line(
                    base,
                    stride,
                    71,
                    "~ marks shortened fallback fields; the requested USB dump is complete.",
                );
                flush_dcache_range(base, stride * SCREEN_H * 4);
                #[cfg(all(feature = "audio-report", not(feature = "board6-report")))]
                let report_bytes = {
                    board_line(
                        base,
                        stride,
                        14,
                        "Board5: bounded I2C4 probe before USB attach; inherited clocks/pins",
                    );
                    usb_barrier();
                    let mut probe_io = UsbIo {
                        start_faults: fault_count(),
                    };
                    let report = audio_probe::run(&mut probe_io, blob);
                    let mut line = board_screen::Line::new();
                    let _ = write!(
                        line,
                        "A5 status={} PA58={}/id{:02x} PA59={}/id{:02x} restored={}",
                        report.words[4],
                        report.words[32],
                        report.words[33],
                        report.words[37],
                        report.words[38],
                        report.words[42]
                    );
                    board_line(base, stride, 15, line.text());
                    let mut line = board_screen::Line::new();
                    let _ = write!(
                        line,
                        "A5 clocks top={:08x} infra={:08x} GPIO105/106={:08x}",
                        report.words[8], report.words[9], report.words[10]
                    );
                    board_line(base, stride, 16, line.text());
                    report.encode()
                };
                #[cfg(feature = "board6-report")]
                let report_bytes = {
                    board_line(
                        base,
                        stride,
                        14,
                        "Board6: I2C4 2x128 byte reads, then inherited PWRAP; bounded, pre-USB",
                    );
                    flush_dcache_range(base, stride * SCREEN_H * 4);
                    let mut probe_io = UsbIo {
                        start_faults: fault_count(),
                    };
                    let report = board6::run(&mut probe_io, blob);
                    #[cfg(feature = "alpha-report")]
                    alpha_event(6, report.words[20] as u64, report.words[1344] as u64);
                    let w = &report.words;
                    let mut line = board_screen::Line::new();
                    let done = w[board6::AMP..board6::PMIC]
                        .chunks_exact(5)
                        .filter(|e| e[0] == 1)
                        .count();
                    let _ = write!(
                        line,
                        "A6 I2C status={} reads={}/256 restored={} PA58={:02x} PA59={:02x}",
                        w[20], done, w[58], w[49], w[54]
                    );
                    board_line(base, stride, 15, line.text());
                    let mut line = board_screen::Line::new();
                    let _ = write!(
                        line,
                        "PWRAP status={} idle={} test={:04x} HWCID={:04x} SWCID={:04x}",
                        w[1344], w[1347], w[1377], w[1382], w[1387]
                    );
                    board_line(base, stride, 16, line.text());
                    #[cfg(not(feature = "tone-report"))]
                    board_line(
                        base,
                        stride,
                        17,
                        "ffffffff = unobserved; status1=complete. No tone/profile loaded.",
                    );
                    #[cfg(feature = "tone-report")]
                    board_line(
                        base,
                        stride,
                        17,
                        "Board6 values = PRE-TONE snapshot. Final Off evidence appears below.",
                    );
                    // Full byte window fits on this fixed fallback page. '--' is
                    // not a zero: it means the read did not complete.
                    for chip in 0..2 {
                        for group in 0..16 {
                            let mut line = board_screen::Line::new();
                            let _ = write!(line, "PA{:02x} {:02x}:", 0x58 + chip, group * 8);
                            for reg in group * 8..group * 8 + 8 {
                                let e = board6::AMP + (chip * 128 + reg) * 5;
                                if w[e] == 1 {
                                    let _ = write!(line, " {:02x}", w[e + 1]);
                                } else {
                                    let _ = write!(line, " --");
                                }
                            }
                            board_line(base, stride, 18 + chip * 16 + group, line.text());
                        }
                    }
                    for group in 0..17 {
                        let mut line = board_screen::Line::new();
                        for i in group * 2..(group * 2 + 2).min(pmic_probe::REGISTERS.len()) {
                            let e = board6::READS + i * 5;
                            let _ = write!(
                                line,
                                " P{:04x}={:08x}/s{}",
                                pmic_probe::REGISTERS[i],
                                w[e + 1],
                                w[e]
                            );
                        }
                        board_line(base, stride, 50 + group, line.text());
                    }
                    let encoded = report.encode();
                    let mut line = board_screen::Line::new();
                    let _ = write!(
                        line,
                        "WACS2 before={:08x} after={:08x}; report CRC={:08x}",
                        w[1356],
                        w[1359],
                        board_pages::crc32(&encoded)
                    );
                    board_line(base, stride, 67, line.text());
                    encoded
                };
                #[cfg(not(feature = "audio-report"))]
                let usb_fault_baseline = initial_faults;
                #[cfg(feature = "tone-report")]
                let tone_report = tone_run(blob, &report_bytes, alpha_tests, base, stride);
                #[cfg(feature = "audio-report")]
                let usb_fault_baseline = fault_count();
                let mut io = UsbIo {
                    // Board5 probe faults are recorded in its frozen report. USB
                    // has its own fault-stop boundary and separate register block.
                    start_faults: usb_fault_baseline,
                };
                let mut device = usb_control::Device::new(blob); // CRC computed before attach
                #[cfg(feature = "audio-report")]
                device.control.set_report(&report_bytes);
                #[cfg(feature = "alpha-report")]
                let diagnostics_bytes =
                    alpha_snapshot(alpha_tests, board_pages::crc32(&report_bytes));
                #[cfg(feature = "alpha-report")]
                device.control.set_diagnostics(&diagnostics_bytes);
                #[cfg(feature = "tone-report")]
                let tone_bytes = tone_report.encode(
                    env!("AYEOS_BUILD_REVISION"),
                    env!("AYEOS_BUILD_DIRTY") != "0",
                    board_pages::crc32(&report_bytes),
                    board_pages::crc32(&diagnostics_bytes),
                    board_pages::crc32(blob),
                );
                #[cfg(feature = "tone-report")]
                device.control.set_tone(&tone_bytes);
                // Commit the completed fallback before potentially faulting in USB init.
                #[cfg(feature = "board6-report")]
                flush_dcache_range(base, stride * SCREEN_H * 4);
                let mut init = usb_phy::initialize(&mut io, board.ready());
                let mut line = board_screen::Line::new();
                let _ = write!(line, "INIT={:?} skipped faults={}", init, fault_count());
                board_line(base, stride, 5, line.text());
                // Snapshot non-destructive status registers, only with matched FDT.
                #[cfg(feature = "tone-report")]
                let mut usb_snapshot = phone_studio::UsbSnapshot::default();
                if board.ready() {
                    let p = io.read8(usb_phy::MUSB + 1);
                    let d = io.read8(usb_phy::MUSB + 0x60);
                    let h = io.read16(usb_phy::MUSB + 0x6c);
                    let c = io.read16(usb_phy::MUSB + 0x12);
                    #[cfg(feature = "tone-report")]
                    {
                        usb_snapshot.power = p;
                        usb_snapshot.devctl = d;
                        usb_snapshot.hwvers = h;
                        usb_snapshot.csr0 = c;
                    }
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
                    #[cfg(feature = "tone-report")]
                    {
                        usb_snapshot.acr6 = a;
                        usb_snapshot.dtm0 = b;
                        usb_snapshot.dtm1 = c;
                    }
                    let mut line = board_screen::Line::new();
                    let _ = write!(
                        line,
                        "U2PORT=11cc0800 ACR6={:08x} DTM0={:08x} DTM1={:08x}",
                        a, b, c
                    );
                    board_line(base, stride, 7, line.text());
                    let a = io.read32(usb_phy::INFRA + 0x90);
                    let b = io.read32(usb_phy::TOP + 0x90);
                    #[cfg(feature = "tone-report")]
                    {
                        usb_snapshot.infra = a;
                        usb_snapshot.top = b;
                    }
                    let mut line = board_screen::Line::new();
                    let _ = write!(
                        line,
                        "INFRA_CG0={:08x} TOP_CFG5={:08x}; regulator setup NOT verified",
                        a, b
                    );
                    board_line(base, stride, 8, line.text());
                }
                board_line(
                    base,
                    stride,
                    10,
                    "USB: W1C enabled; status row updates during quiet polls only",
                );
                // Presentation only: consume already frozen evidence. Keep the
                // dense fallback on initialization failure. All full-screen work
                // and cache flushing must finish BEFORE SOFTCONN is asserted.
                #[cfg(feature = "tone-report")]
                let studio_visible = init == usb_phy::Init::Ready && !io.faulted();
                #[cfg(feature = "tone-report")]
                if studio_visible {
                    let w = |i: usize| {
                        u32::from_le_bytes(report_bytes[i * 4..i * 4 + 4].try_into().unwrap())
                    };
                    let view = phone_studio::View {
                        revision: env!("AYEOS_BUILD_REVISION"),
                        dirty: env!("AYEOS_BUILD_DIRTY") != "0",
                        passed: alpha_tests.0,
                        failed: alpha_tests.1,
                        faults: usb_fault_baseline,
                        i2c_status: w(20),
                        amp_reads: (0..256).filter(|i| w(board6::AMP + i * 5) == 1).count(),
                        i2c_restored: w(58) == 1,
                        pmic_status: w(1344),
                        pmic_reads: (0..33).filter(|i| w(board6::READS + i * 5) == 1).count(),
                        pmic_idle: w(1347) == 1,
                        report_crc: board_pages::crc32(&tone_bytes),
                        usb: &usb_snapshot,
                        tone: &tone_report,
                    };
                    phone_studio::paint(&mut PhoneCanvas { base, stride }, glyph_8x16_hd, &view);
                }
                #[cfg(feature = "tone-report")]
                let status_y = if studio_visible {
                    phone_studio::USB_STATUS_Y
                } else {
                    68 + 10 * 18
                };
                #[cfg(not(feature = "tone-report"))]
                let status_y = 68 + 10 * 18;
                flush_dcache_range(base, stride * SCREEN_H * 4);
                if init == usb_phy::Init::Ready {
                    init = usb_phy::connect(&mut io);
                }
                let mut prior = (u8::MAX, u8::MAX, u32::MAX, false, u32::MAX, u32::MAX);
                let mut status_row = usb_status::StatusRow::new();
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
                        device.requests,
                        device.stalls,
                    );
                    let idle =
                        init != usb_phy::Init::Attached || io.faulted() || device.display_idle();
                    if state != prior && idle && status_row.ready() {
                        prior = state;
                        let mut line = board_screen::Line::new();
                        let _ = write!(
                            line,
                            "{:?} a={} c={} resets={} fault={} req={} stall={}",
                            init, state.0, state.1, state.2, state.3, state.4, state.5
                        );
                        status_row.begin(line.text());
                    }
                    if let Some((column, byte)) = status_row.tick(idle) {
                        let cell = [byte];
                        let text = core::str::from_utf8(&cell).unwrap_or("?");
                        // Same bounded cell budget on both presentations:
                        // scale=1: 8x16 glyph plus one spacing column.
                        draw_text_opaque_scaled(
                            base,
                            stride,
                            20 + column * 9,
                            status_y,
                            1,
                            text,
                            0xFF62E6C9,
                            0xFF05060A,
                            1,
                        );
                        usb_barrier(); // only order this cell's writes; no range sweep
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
