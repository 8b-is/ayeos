include!("exception_runtime.rs");
static mut ALPHA_JOURNAL: diagnostics::Journal = diagnostics::Journal::new();
static mut ALPHA_FRAMEBUFFER: (usize, usize) = (0, 0);
static mut ALPHA_PANIC: (u64, u32, u32) = (0, 0, 0);

fn alpha_event(stage: u64, result: u64, detail: u64) {
    unsafe {
        let mut journal = read_volatile(core::ptr::addr_of!(ALPHA_JOURNAL));
        journal.record(stage, result, detail);
        write_volatile(core::ptr::addr_of_mut!(ALPHA_JOURNAL), journal);
    }
}
fn alpha_screen_ready(base: usize, stride: usize) {
    unsafe {
        write_volatile(core::ptr::addr_of_mut!(ALPHA_FRAMEBUFFER), (base, stride));
    }
    alpha_event(2, 0, base as u64);
}

fn alpha_selftests(base: usize, stride: usize) -> (u32, u32, u64, u64) {
    use core::fmt::Write;
    board_line(
        base,
        stride,
        72,
        "ALPHA: CRC / Marine reference fixture / exception self-test",
    );
    flush_dcache_range(base, stride * SCREEN_H * 4);
    let mut passed = 0;
    let crc_ok = board_pages::crc32(b"123456789") == 0xcbf4_3926;
    if crc_ok {
        passed |= diagnostics::TEST_CRC;
    }
    alpha_event(3, u64::from(crc_ok), 0);
    let (hash, samples) = marine_fixture::run();
    let marine_ok = samples == marine_fixture::SAMPLES && hash == marine_fixture::REFERENCE_HASH;
    if marine_ok {
        passed |= diagnostics::TEST_MARINE;
    }
    alpha_event(4, u64::from(marine_ok), hash);
    unsafe {
        write_volatile(core::ptr::addr_of_mut!(ALPHA_TEST_ARMED), true);
        ayeos_exception_test();
        write_volatile(core::ptr::addr_of_mut!(ALPHA_TEST_ARMED), false);
        if read_volatile(core::ptr::addr_of!(ALPHA_TEST_SEEN)) {
            passed |= diagnostics::TEST_EXCEPTION;
        }
    }
    alpha_event(5, u64::from(passed & diagnostics::TEST_EXCEPTION != 0), 0);
    let failed = diagnostics::EXPECTED_TESTS & !passed;
    let mut line = board_screen::Line::new();
    let _ = write!(
        line,
        "Self-tests passed={:x} failed={:x}; Marine samples={} hash={:016x}",
        passed, failed, samples, hash
    );
    board_line(base, stride, 73, line.text());
    let mut line = board_screen::Line::new();
    let _ = write!(
        line,
        "Revision {} dirty={}",
        env!("AYEOS_BUILD_REVISION"),
        env!("AYEOS_BUILD_DIRTY")
    );
    board_line(base, stride, 74, line.text());
    #[cfg(not(feature = "tone-report"))]
    board_line(
        base,
        stride,
        75,
        "Marine input=fixture. PCM/capture/Phoenix/tone are not active.",
    );
    #[cfg(feature = "tone-report")]
    board_line(
        base,
        stride,
        75,
        "Marine input=fixture. SGEN attempt follows; live PCM/Phoenix inactive.",
    );
    (passed, failed, hash, samples as u64)
}

fn alpha_snapshot(tests: (u32, u32, u64, u64), board_crc: u32) -> [u8; diagnostics::BYTES] {
    alpha_event(7, 1, board_crc as u64); // report frozen before USB initialization
    unsafe {
        let journal = read_volatile(core::ptr::addr_of!(ALPHA_JOURNAL));
        diagnostics::encode(diagnostics::Snapshot {
            revision: env!("AYEOS_BUILD_REVISION"),
            dirty: env!("AYEOS_BUILD_DIRTY") != "0",
            passed: tests.0,
            failed: tests.1,
            board_crc,
            fault_count: fault_count(),
            fault: read_volatile(core::ptr::addr_of!(ALPHA_FAULT)),
            marine_hash: tests.2,
            marine_samples: tests.3,
            journal: &journal,
        })
    }
}

fn alpha_panic(info: &PanicInfo<'_>) -> ! {
    unsafe {
        if read_volatile(core::ptr::addr_of!(AYEOS_IN_EXCEPTION)) != 0 {
            alpha_halt();
        }
        write_volatile(core::ptr::addr_of_mut!(AYEOS_IN_EXCEPTION), 1);
        if let Some(location) = info.location() {
            let hash =
                marine_fixture::hash_bytes(marine_fixture::FNV_OFFSET, location.file().as_bytes());
            write_volatile(
                core::ptr::addr_of_mut!(ALPHA_PANIC),
                (hash, location.line(), location.column()),
            );
        }
    }
    ayeos_fatal_halt()
}

#[no_mangle]
extern "C" fn ayeos_fatal_halt() -> ! {
    use core::fmt::Write;
    #[cfg(feature = "tone-report")]
    tone_fatal_cleanup();
    let (base, stride) = unsafe { read_volatile(core::ptr::addr_of!(ALPHA_FRAMEBUFFER)) };
    if base != 0 {
        // No peripheral probe, watchdog access, USB restart or format allocation.
        // A framebuffer fault reenters the assembly's bounded secondary-fault halt.
        let show = |row: usize, text: &str| {
            draw_text_opaque_scaled(
                base,
                stride,
                20,
                68 + row * 18,
                1,
                text,
                0xFFE8B05C,
                0xFF05060A,
                76,
            )
        };
        show(0, "AYEOS AUDIO ALPHA1 / FATAL - photograph this screen");
        show(1, env!("AYEOS_BUILD_REVISION"));
        let f = unsafe { read_volatile(core::ptr::addr_of!(ALPHA_FAULT)) };
        let mut line = board_screen::Line::new();
        let _ = write!(
            line,
            "vector={} EL={} action={} ESR={:016x}",
            f.vector, f.level, f.action, f.esr
        );
        show(2, line.text());
        let mut line = board_screen::Line::new();
        let _ = write!(line, "PC={:016x} FAR={:016x}", f.pc, f.far);
        show(3, line.text());
        let mut line = board_screen::Line::new();
        let _ = write!(line, "SP={:016x} SPSR={:016x}", f.sp, f.spsr);
        show(4, line.text());
        let (file, row, column) = unsafe { read_volatile(core::ptr::addr_of!(ALPHA_PANIC)) };
        let mut line = board_screen::Line::new();
        let _ = write!(
            line,
            "Panic file FNV64={:016x} line={} column={}",
            file, row, column
        );
        show(5, line.text());
        #[cfg(feature = "tone-report")]
        show(
            7,
            if unsafe { read_volatile(core::ptr::addr_of!(TONE_FATAL_OFF)) } {
                "Emergency stop: Off predicates verified; fatal snapshot is screen-only."
            } else {
                "Emergency Off NOT verified. If sounding, use physical power-off."
            },
        );
        show(
            6,
            "RAM evidence only; no automatic repair or reboot. USB halted.",
        );
        flush_dcache_range(base, stride * SCREEN_H * 4);
    }
    alpha_halt()
}
fn alpha_halt() -> ! {
    loop {
        unsafe {
            asm!("wfe", options(nomem, nostack, preserves_flags));
        }
    }
}
