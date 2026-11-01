// Single boot CPU, DAIF masked. Never lend a reference to global state across I/O.
static mut TONE_ARMED: bool = false;
static mut TONE_SAVED: [u16; 17] = [0; 17];
static mut TONE_FATAL_ACTIVE: bool = false;
static mut TONE_FATAL_OFF: bool = false;

impl tone_bus::HardwareIo for UsbIo {
    fn counter(&mut self) -> tone::Result<u64> {
        let v: u64;
        unsafe {
            asm!("isb", "mrs {}, cntpct_el0", out(reg) v, options(nostack, preserves_flags));
        }
        Ok(v)
    }
    fn frequency(&mut self) -> u64 {
        let v: u64;
        unsafe {
            asm!("mrs {}, cntfrq_el0", out(reg) v, options(nomem, nostack, preserves_flags));
        }
        v
    }
    fn fresh(&mut self) {
        self.start_faults = fault_count();
    }
    fn armed(&mut self, armed: bool, saved: [u16; 17]) {
        unsafe {
            write_volatile(core::ptr::addr_of_mut!(TONE_SAVED), saved);
            write_volatile(core::ptr::addr_of_mut!(TONE_ARMED), armed);
        }
    }
}

fn tone_run(
    blob: &[u8],
    board: &[u8],
    tests: (u32, u32, u64, u64),
    base: usize,
    stride: usize,
) -> tone::Report {
    use core::fmt::Write;
    let w = |i: usize| u32::from_le_bytes(board[i * 4..i * 4 + 4].try_into().unwrap());
    // The Board6 report is the pre-tone snapshot; require every read and restored
    // controller, plus zero unexpected faults and every alpha self-test.
    let gate = tests.0 == diagnostics::EXPECTED_TESTS
        && tests.1 == 0
        && fault_count() == 0
        && audio_board::matches(blob).unwrap_or(false)
        && pmic_probe::matches(blob).unwrap_or(false)
        && tone_board::matches(blob).unwrap_or(false)
        && w(20) == 1
        && w(58) == 1
        && w(1344) == 1
        && w(1347) == 1
        && (0..256).all(|i| w(board6::AMP + i * 5) == 1)
        && (0..33).all(|i| w(board6::READS + i * 5) == 1);
    board_line(
        base,
        stride,
        76,
        "TONE1: one 250ms SGEN arpeggio, lineout 0dB; preparing Off evidence",
    );
    flush_dcache_range(base, stride * SCREEN_H * 4);
    let r = if gate {
        match tone_bus::Hardware::prepare(UsbIo {
            start_faults: fault_count(),
        }) {
            Ok(mut bus) => tone::run(&mut bus, true),
            Err(e) => {
                let mut r = tone::Report::new();
                r.outcome = 2;
                r.error = e as u32;
                r
            }
        }
    } else {
        let mut r = tone::Report::new();
        r.outcome = 2;
        r.error = tone::Error::Gate as u32;
        r
    };
    let mut line = board_screen::Line::new();
    let _ = write!(
        line,
        "TONE outcome={} err={} cleanup={} Off={:x}/{:x} restored={}",
        r.outcome,
        r.error,
        r.cleanup_errors,
        r.off,
        tone::OFF_MASK,
        r.restored
    );
    board_line(base, stride, 76, line.text());
    let mut line = board_screen::Line::new();
    let _ = write!(
        line,
        "Counter Hz={} window ticks={} verified-Off={}",
        r.frequency,
        r.closed.saturating_sub(r.opened),
        r.verified_off()
    );
    board_line(base, stride, 77, line.text());
    board_line(
        base,
        stride,
        78,
        "SGEN bypasses PCM/Marine/Phoenix. Report sound, stop and pop separately.",
    );
    flush_dcache_range(base, stride * SCREEN_H * 4);
    r
}

fn div_to_key_index(div: u16) -> u8 {
    match div {
        22.. => 0,
        20..=21 => 1,
        18..=19 => 2,
        17 => 3,
        16 => 4,
        15 => 5,
        14 => 6,
        13 => 7,
        12 => 6,
        11 => 8,
        10 => 9,
        9 => 10,
        8 => 10,
        7 => 12,
        6 => 13,
        5 => 15,
        4 => 17,
        3 => 20,
        2 => 22,
        _ => 23,
    }
}

fn tone_play_live(
    gate: bool,
    studio_visible: bool,
    cmd_value: u16,
    cmd_index: u16,
    base: usize,
    stride: usize,
    active_row: &mut usize,
) -> (u32, u32) {
    if !gate {
        return (2, 0);
    }
    let Ok(mut bus) = tone_bus::Hardware::prepare(UsbIo {
        start_faults: fault_count(),
    }) else {
        return (2, 0);
    };
    let custom_step;
    let (steps, base_row): (&[tone::TrackerStep], usize) = if cmd_value == 0x0080 {
        (&tone::PATTERN, 0)
    } else if (0x0081..=0x0088).contains(&cmd_value) {
        let idx = ((cmd_value - 0x0081) as usize) % tone::PATTERN.len();
        *active_row = idx;
        (&tone::PATTERN[idx..=idx], idx)
    } else if (1..=31).contains(&cmd_value) {
        let div = cmd_value;
        let spk_mask = match (cmd_index & 3) as u8 {
            1 => 1,
            2 => 2,
            _ => 3,
        };
        let (gain_reg, gain_db) = match (cmd_index >> 2) & 7 {
            1 => (0x0204, 4),
            2 => (0x0000, 8),
            3 => (0x060c, -4),
            4 => (0x0912, -10),
            _ => (0x0408, 0),
        };
        let dur_10ms = (cmd_index >> 8) as u32;
        let duration_us = if dur_10ms == 0 {
            140_000
        } else {
            (dur_10ms * 10_000).clamp(20_000, 1_500_000)
        };
        custom_step = [tone::TrackerStep {
            note: "JAM",
            key_index: div_to_key_index(div),
            hz: 6000 / div,
            div,
            gain_reg,
            gain_db,
            spk_mask,
            duration_us,
        }];
        (&custom_step, *active_row)
    } else {
        return (2, 0);
    };

    let r = tone::play_steps(&mut bus, steps, |idx, step| {
        pet_mediatek_watchdog();
        let row = (base_row + idx) % tone::PATTERN.len();
        *active_row = row;
        if studio_visible {
            let mut canvas = PhoneCanvas { base, stride };
            phone_studio::paint_tracker_rows(&mut canvas, glyph_8x16_hd, row);
            phone_studio::paint_keyboard(&mut canvas, glyph_8x16_hd, step.key_index);
            flush_dcache_range(base, stride * SCREEN_H * 4);
        }
    });
    (r.outcome, r.off)
}

fn tone_fatal_cleanup() {
    unsafe {
        if read_volatile(core::ptr::addr_of!(TONE_FATAL_ACTIVE)) {
            alpha_halt();
        }
        if !read_volatile(core::ptr::addr_of!(TONE_ARMED)) {
            return;
        }
        write_volatile(core::ptr::addr_of_mut!(TONE_FATAL_ACTIVE), true);
        // Permit ONLY the normal exact-MMIO fixups during this bounded attempt.
        // A second nonrecoverable exception reaches the guard above and halts.
        // The original fatal record has precedence in exception_runtime.rs.
        write_volatile(core::ptr::addr_of_mut!(AYEOS_IN_EXCEPTION), 0);
        let saved = read_volatile(core::ptr::addr_of!(TONE_SAVED));
        let mut bus = tone_bus::Hardware::emergency(
            UsbIo {
                start_faults: fault_count(),
            },
            saved,
        );
        let mut r = tone::Report::new();
        tone::shutdown(&mut bus, &mut r);
        write_volatile(core::ptr::addr_of_mut!(TONE_FATAL_OFF), r.verified_off());
        write_volatile(core::ptr::addr_of_mut!(AYEOS_IN_EXCEPTION), 1);
    }
}
