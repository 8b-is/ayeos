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
        "TONE1: one 250ms SGEN window, lineout -40dB; preparing Off evidence",
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
