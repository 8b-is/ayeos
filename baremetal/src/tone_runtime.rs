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

// Concert-pitch frequencies (Hz) for the 24 keys (C4..B5, indices 0..23).
const SEMITONE_HZ: [u16; 24] = [
    262, 277, 294, 311, 330, 349, 370, 392, 415, 440, 466, 494,
    523, 554, 587, 622, 659, 698, 740, 784, 831, 880, 932, 988,
];

const SEMITONE_DIVS: [u16; 24] = [
    1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 8, 8, 9, 9, 10, 11, 11, 12, 12, 13, 13, 14,
];

fn div_to_key_index(div: u16) -> u8 {
    match div & 0x1f {
        0 | 1 => 0,
        2 => 2,
        3 => 4,
        4 => 5,
        5 => 7,
        6 => 9,
        7 => 11,
        8 => 12,
        9 => 14,
        10 => 16,
        11 => 17,
        12 => 19,
        13 => 21,
        14 => 23,
        n => ((n - 1) as u8).min(23),
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
    } else if (0x0090..=0x00a7).contains(&cmd_value) || (1..=31).contains(&cmd_value) {
        let (div, key_index) = if (0x0090..=0x00a7).contains(&cmd_value) {
            let k = (cmd_value - 0x0090) as u8;
            (SEMITONE_DIVS[k as usize], k)
        } else {
            (cmd_value, div_to_key_index(cmd_value))
        };
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
            _ => (0x0000, 8),
        };
        let dur_10ms = (cmd_index >> 8) as u32;
        let duration_us = if dur_10ms == 0 {
            150_000
        } else {
            (dur_10ms * 10_000).clamp(20_000, 1_500_000)
        };
        let step_mult = (div & 0x1f).clamp(1, 31);
        let hz = SEMITONE_HZ[(key_index as usize).min(23)];
        custom_step = [tone::TrackerStep {
            note: "JAM",
            key_index,
            hz,
            div: step_mult,
            gain_reg,
            gain_db,
            spk_mask,
            duration_us,
        }];
        (&custom_step, *active_row)
    } else {
        return (2, 0);
    };

    let mut current_key = steps[0].key_index;
    if studio_visible && steps.len() == 1 {
        let mut canvas = PhoneCanvas { base, stride };
        phone_studio::paint_tracker_rows(&mut canvas, glyph_8x16_hd, *active_row);
        phone_studio::paint_keyboard(&mut canvas, glyph_8x16_hd, current_key);
        flush_dcache_rows(base, stride, 730, 414);
    }
    let is_single_note = steps.len() == 1;
    let r = tone::play_steps_with_ui(
        &mut bus,
        steps,
        |idx, _step| {
            pet_mediatek_watchdog();
            let row = (base_row + idx) % tone::PATTERN.len();
            *active_row = row;
        },
        |_elapsed| {
            pet_mediatek_watchdog();
            if !studio_visible || (safe_mmio_read32(0x1000_5200) & (1 << 1)) == 0 {
                return tone::UiAction::Continue;
            }
            match gc7272_poll_touch(false) {
                Some((tx, ty, true)) => {
                    if let Some(semi) = phone_studio::key_at_point(tx, ty) {
                        if is_single_note {
                            if semi != current_key {
                                current_key = semi;
                                let mut canvas = PhoneCanvas { base, stride };
                                phone_studio::paint_keyboard(
                                    &mut canvas,
                                    glyph_8x16_hd,
                                    current_key,
                                );
                                flush_dcache_rows(base, stride, 1020, 124);
                            }
                            return tone::UiAction::ChangePitch(
                                SEMITONE_HZ[(semi as usize).min(23)],
                            );
                        }
                    } else if ty < 1000 || ty > 1160 {
                        // Touch outside keyboard (e.g. Qwave Browser scroll/pinch/tab or Tracker):
                        // immediately yield P1 Audio thread to P0 UI thread!
                        return tone::UiAction::Preempt;
                    }
                    tone::UiAction::Continue
                }
                _ => tone::UiAction::Continue,
            }
        },
    );
    if studio_visible && steps.len() > 1 {
        let last_key = steps[steps.len() - 1].key_index;
        let mut canvas = PhoneCanvas { base, stride };
        phone_studio::paint_tracker_rows(&mut canvas, glyph_8x16_hd, *active_row);
        phone_studio::paint_keyboard(&mut canvas, glyph_8x16_hd, last_key);
        flush_dcache_rows(base, stride, 730, 414);
    }
    (r.outcome, r.off)
}

fn tone_debug_exec(
    control: &mut usb_control::Control<'_>,
    op_val: u16,
    idx: u16,
    base: usize,
    stride: usize,
    active_row: &mut usize,
    qwave_scroll: &mut usize,
    qwave_tab: &mut usize,
    qwave_zoom: &mut usize,
    qwave_full_mode: &mut bool,
    bat_mv: u16,
    studio_visible: bool,
    tone_gate: bool,
) {
    use tone::Bus;
    let op = (op_val & 0x00ff) as u8;
    let sub = (op_val >> 8) as u8;
    control.dbg_seq = control.dbg_seq.wrapping_add(1);
    control.dbg_last_op = op_val;
    control.dbg_last_idx = idx;
    control.dbg_buf = [0u8; 32];
    match op {
        2 => match tone_bus::Hardware::prepare(UsbIo {
            start_faults: fault_count(),
        }) {
            Ok(mut bus) => {
                match bus.pmic_read(idx) {
                    Ok(v) => {
                        control.dbg_status = 1;
                        control.dbg_result = v as u32;
                    }
                    Err(e) => {
                        control.dbg_status = 0x100 | (e as u32);
                        control.dbg_result = 0;
                    }
                }
                if sub != 0 {
                    for i in 0..8usize {
                        let r = idx.wrapping_add((i * 2) as u16);
                        let val = bus.pmic_read(r).unwrap_or(0xffff);
                        control.dbg_buf[i * 2..i * 2 + 2].copy_from_slice(&val.to_le_bytes());
                    }
                }
                let _ = bus.restore();
            }
            Err(e) => {
                control.dbg_status = 0x200 | (e as u32);
                control.dbg_result = 0;
            }
        },
        3 => match tone_bus::Hardware::prepare(UsbIo {
            start_faults: fault_count(),
        }) {
            Ok(mut bus) => {
                let val = control.dbg_arg_lo;
                let mask = control.dbg_arg_hi;
                let res = if mask != 0 {
                    bus.pmic_read(idx).and_then(|old| {
                        let next = (old & !mask) | (val & mask);
                        bus.pmic_write(idx, next).and_then(|_| bus.pmic_read(idx))
                    })
                } else {
                    bus.pmic_write(idx, val).and_then(|_| bus.pmic_read(idx))
                };
                match res {
                    Ok(v) => {
                        control.dbg_status = 1;
                        control.dbg_result = v as u32;
                    }
                    Err(e) => {
                        control.dbg_status = 0x100 | (e as u32);
                        control.dbg_result = 0;
                    }
                }
                let _ = bus.restore();
            }
            Err(e) => {
                control.dbg_status = 0x200 | (e as u32);
                control.dbg_result = 0;
            }
        },
        4 => {
            let addr = (idx >> 8) as u8;
            let reg = (idx & 0xff) as u8;
            match tone_bus::Hardware::prepare(UsbIo {
                start_faults: fault_count(),
            }) {
                Ok(mut bus) => {
                    match bus.amp_read(addr, reg) {
                        Ok(v) => {
                            control.dbg_status = 1;
                            control.dbg_result = v as u32;
                        }
                        Err(e) => {
                            control.dbg_status = 0x100 | (e as u32);
                            control.dbg_result = 0;
                        }
                    }
                    if sub != 0 {
                        for i in 0..32usize {
                            let r = reg.wrapping_add(i as u8) & 0x7f;
                            control.dbg_buf[i] = bus.amp_read(addr, r).unwrap_or(0xff);
                        }
                    }
                    let _ = bus.restore();
                }
                Err(e) => {
                    control.dbg_status = 0x200 | (e as u32);
                    control.dbg_result = 0;
                }
            }
        }
        5 => {
            let addr = (idx >> 8) as u8;
            let reg = (idx & 0xff) as u8;
            let val = (control.dbg_arg_lo & 0xff) as u8;
            match tone_bus::Hardware::prepare(UsbIo {
                start_faults: fault_count(),
            }) {
                Ok(mut bus) => {
                    match bus.amp_write(addr, reg, val).and_then(|_| bus.amp_read(addr, reg)) {
                        Ok(v) => {
                            control.dbg_status = 1;
                            control.dbg_result = v as u32;
                        }
                        Err(e) => {
                            control.dbg_status = 0x100 | (e as u32);
                            control.dbg_result = 0;
                        }
                    }
                    let _ = bus.restore();
                }
                Err(e) => {
                    control.dbg_status = 0x200 | (e as u32);
                    control.dbg_result = 0;
                }
            }
        }
        6 => {
            let addr = control.dbg_addr() as usize;
            let f0 = fault_count();
            let v = match sub {
                1 => safe_mmio_read8(addr) as u32,
                2 => safe_mmio_read16(addr) as u32,
                _ => safe_mmio_read32(addr),
            };
            let f1 = fault_count();
            control.dbg_result = v;
            let count = (idx as usize).min(8);
            for i in 0..count {
                let w = safe_mmio_read32(addr.wrapping_add(i * 4));
                control.dbg_buf[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
            }
            let f2 = fault_count();
            control.dbg_status = if f1 == f0 && f2 == f0 {
                1
            } else {
                0x300 | ((f2.saturating_sub(f0) & 0xff) as u32)
            };
        }
        7 => {
            let addr = control.dbg_addr() as usize;
            let val = control.dbg_arg();
            let f0 = fault_count();
            match sub {
                1 => safe_mmio_write8(addr, val as u8),
                2 => safe_mmio_write16(addr, val as u16),
                _ => safe_mmio_write32(addr, val),
            }
            let rb = match sub {
                1 => safe_mmio_read8(addr) as u32,
                2 => safe_mmio_read16(addr) as u32,
                _ => safe_mmio_read32(addr),
            };
            let f1 = fault_count();
            control.dbg_result = rb;
            control.dbg_status = if f1 == f0 {
                1
            } else {
                0x300 | ((f1.saturating_sub(f0) & 0xff) as u32)
            };
        }
        8 => {
            const SPI0_BASE: usize = 0x1100_a000;
            let f0 = fault_count();
            if safe_mmio_read32(0x1000_1094) & (1 << 21) != 0 || (sub & 2 != 0) {
                safe_mmio_write32(0x1000_108c, 1 << 21);
            }
            if sub == 0 {
                for i in 0..8usize {
                    let w = safe_mmio_read32(SPI0_BASE + i * 4);
                    control.dbg_buf[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
                }
                control.dbg_result = safe_mmio_read32(SPI0_BASE + 0x18);
                let f1 = fault_count();
                control.dbg_status = if f1 == f0 {
                    1
                } else {
                    0x300 | ((f1 - f0) as u32)
                };
            } else {
                let nwords = ((sub & 0x0f) as usize).clamp(1, 4);
                let cmd0 = safe_mmio_read32(SPI0_BASE + 0x18);
                safe_mmio_write32(SPI0_BASE + 0x18, cmd0 | (1 << 2));
                safe_mmio_write32(SPI0_BASE + 0x18, 0);
                // Restore SPI0 ~9.6 MHz clock timing after SPI_CMD_RST
                safe_mmio_write32(SPI0_BASE + 0x00, 0x000a_000a);
                safe_mmio_write32(SPI0_BASE + 0x28, 0x0005_0005);
                let pkt_bytes = (nwords * 4) as u32;
                safe_mmio_write32(SPI0_BASE + 0x04, (pkt_bytes.saturating_sub(1) << 16) | 0x000a);
                safe_mmio_write32(SPI0_BASE + 0x0c, 0);
                let tx0 = control.dbg_arg();
                let tx1 = control.dbg_addr();
                let tx_words = [tx0, tx1, idx as u32, 0];
                for i in 0..nwords {
                    safe_mmio_write32(SPI0_BASE + 0x10, tx_words[i]);
                }
                // SPI_CMD_ACT (bit 0) | TXMSBF (bit 12) | RXMSBF (bit 13) | DEASSERT (bit 16)
                safe_mmio_write32(SPI0_BASE + 0x18, 0x0001_3001);
                let mut done = false;
                for _ in 0..16_384 {
                    let st0 = safe_mmio_read32(SPI0_BASE + 0x1c);
                    let st1 = safe_mmio_read32(SPI0_BASE + 0x20);
                    if (st0 & 1) != 0 && (st1 & 1) != 0 {
                        done = true;
                        break;
                    }
                }
                for i in 0..nwords {
                    let rx = safe_mmio_read32(SPI0_BASE + 0x14);
                    control.dbg_buf[i * 4..i * 4 + 4].copy_from_slice(&rx.to_le_bytes());
                }
                for i in 0..4usize {
                    let w = safe_mmio_read32(SPI0_BASE + 0x18 + i * 4);
                    control.dbg_buf[16 + i * 4..20 + i * 4].copy_from_slice(&w.to_le_bytes());
                }
                control.dbg_result = u32::from_le_bytes(control.dbg_buf[0..4].try_into().unwrap());
                let f1 = fault_count();
                control.dbg_status = if f1 != f0 {
                    0x300 | ((f1 - f0) as u32)
                } else if done {
                    1
                } else {
                    2
                };
            }
        }
        9 => match sub {
            0 => {
                let x = idx;
                let y = control.dbg_arg_lo;
                control.dbg_result = tone_dispatch_touch_point(
                    control,
                    x,
                    y,
                    base,
                    stride,
                    active_row,
                    qwave_scroll,
                    qwave_tab,
                    qwave_zoom,
                    qwave_full_mode,
                    bat_mv,
                    studio_visible,
                    tone_gate,
                );
                control.dbg_status = 1;
            }
            1 => {
                let div = (idx & 0x1f).clamp(1, 31);
                let spk_mask = match (idx >> 8) & 3 {
                    1 => 1,
                    2 => 2,
                    _ => 3,
                };
                let gain_reg = if control.dbg_arg_lo == 0 && control.dbg_arg_hi == 0 {
                    0x0000
                } else {
                    control.dbg_arg_lo
                };
                match tone_bus::Hardware::prepare(UsbIo {
                    start_faults: fault_count(),
                }) {
                    Ok(mut bus) => {
                        let r = tone::debug_synth_start(&mut bus, div, gain_reg, spk_mask);
                        control.dbg_status = r.outcome;
                        control.dbg_result = if r.outcome == 1 {
                            ((spk_mask as u32) << 16) | (div as u32)
                        } else {
                            r.error
                        };
                        if r.outcome == 1 && studio_visible {
                            let mut canvas = PhoneCanvas { base, stride };
                            phone_studio::paint_keyboard(
                                &mut canvas,
                                glyph_8x16_hd,
                                div_to_key_index(div),
                            );
                            flush_dcache_rows(base, stride, 1020, 124);
                        }
                    }
                    Err(e) => {
                        control.dbg_status = 0x200 | (e as u32);
                        control.dbg_result = 0;
                    }
                }
            }
            2 => match tone_bus::Hardware::prepare(UsbIo {
                start_faults: fault_count(),
            }) {
                Ok(mut bus) => {
                    let r = tone::debug_synth_stop(&mut bus);
                    control.dbg_status = r.outcome;
                    control.dbg_result = r.off;
                }
                Err(e) => {
                    control.dbg_status = 0x200 | (e as u32);
                    control.dbg_result = 0;
                }
            },
            3 => {
                control.touch_enabled = idx != 0;
                control.touch_irq_gpio = control.dbg_arg_lo;
                control.dbg_status = 1;
                control.dbg_result =
                    ((control.touch_irq_gpio as u32) << 8) | (control.touch_enabled as u32);
            }
            4 => {
                let delta = idx as i16;
                if control.dbg_arg_lo != 0xffff {
                    *qwave_tab = (control.dbg_arg_lo as usize) % phone_studio::QWAVE_TABS.len();
                }
                if delta < 0 {
                    *qwave_scroll = qwave_scroll.saturating_sub((-delta) as usize);
                } else if delta > 0 {
                    *qwave_scroll =
                        (*qwave_scroll + (delta as usize)).min(phone_studio::QWAVE_MAX_SCROLL);
                }
                if studio_visible {
                    let mut canvas = PhoneCanvas { base, stride };
                    if *qwave_full_mode {
                        phone_studio::paint_qwave_top_viewport_zoom(
                            &mut canvas,
                            glyph_8x16_hd,
                            *qwave_scroll,
                            *qwave_tab,
                            *qwave_zoom,
                            bat_mv,
                        );
                        flush_dcache_rows(base, stride, 64, 598);
                    }
                    phone_studio::paint_qwave_browser(
                        &mut canvas,
                        glyph_8x16_hd,
                        *qwave_scroll,
                        *qwave_tab,
                    );
                    flush_dcache_rows(base, stride, 1176, 152);
                }
                control.dbg_status = 1;
                control.dbg_result = ((*qwave_tab as u32) << 8) | (*qwave_scroll as u32);
            }
            5 => {
                match idx {
                    0 => {
                        control.dbg_result = gc7272_hostdownload_reset_b1();
                        unsafe {
                            control.dbg_buf = read_volatile(core::ptr::addr_of!(GC7272_LAST_PKT));
                        }
                        control.dbg_status = 1;
                    }
                    1 => {
                        let s0 = gc7272_hostdownload_reset_b1();
                        gc7272_upgrade_soft_reset();
                        control.dbg_result = s0 | 0x0100_0000;
                        unsafe {
                            control.dbg_buf = read_volatile(core::ptr::addr_of!(GC7272_LAST_PKT));
                        }
                        control.dbg_status = 1;
                    }
                    2 => {
                        control.dbg_result = gc7272_hostdownload_idm2();
                        unsafe {
                            control.dbg_buf = read_volatile(core::ptr::addr_of!(GC7272_LAST_PKT));
                        }
                        control.dbg_status = 1;
                    }
                    3 => {
                        control.dbg_result = gc7272_idm_read_id(&mut control.dbg_buf);
                        control.dbg_status = 1;
                    }
                    4 => {
                        let mut pkt = [0u8; 65];
                        gc7272_spi_read(&mut pkt);
                        control.dbg_buf.copy_from_slice(&pkt[0..32]);
                        control.dbg_result = (pkt[0] as u32)
                            | ((pkt[1] as u32) << 8)
                            | ((pkt[2] as u32) << 16)
                            | ((pkt[64] as u32) << 24);
                        control.dbg_status = 1;
                    }
                    _ => {
                        unsafe {
                            control.dbg_result =
                                read_volatile(core::ptr::addr_of!(GC7272_BOOT_STATUS));
                            control.dbg_buf = read_volatile(core::ptr::addr_of!(GC7272_LAST_PKT));
                        }
                        control.dbg_status = 1;
                    }
                }
            }
            6 => {
                // High-speed on-chip digital audio frequency counter & triggered oscilloscope
                // Reads MT6358_AFUNC_AUD_MON0 (0x22a4 by default, or PMIC reg `idx`) in a tight
                // 13 MHz hardware-timed window (default 40ms, or `dbg_arg_lo` ms).
                let mon_reg = if idx == 0 { 0x22a4 } else { idx };
                let win_ms = if control.dbg_arg_lo == 0 {
                    40u64
                } else {
                    (control.dbg_arg_lo as u64).clamp(5, 250)
                };
                let win_ticks = win_ms * 13_000;
                match tone_bus::Hardware::prepare(UsbIo {
                    start_faults: fault_count(),
                }) {
                    Ok(mut bus) => {
                        let t0 = bus.ticks().unwrap_or(0);
                        let mut min_l: i8 = 127;
                        let mut max_l: i8 = -128;
                        let mut min_r: i8 = 127;
                        let mut max_r: i8 = -128;
                        let mut n_samples: u32 = 0;
                        let mut armed_l = false;
                        let mut armed_r = false;
                        let mut zc_l: u32 = 0;
                        let mut zc_r: u32 = 0;
                        let mut t_first_l: u64 = 0;
                        let mut t_last_l: u64 = 0;
                        let mut t_first_r: u64 = 0;
                        let mut t_last_r: u64 = 0;
                        let mut wf_idx: usize = 0;
                        let mut wf_decim: u32 = 0;
                        loop {
                            if (n_samples & 0x3ff) == 0 {
                                pet_mediatek_watchdog();
                            }
                            let now = bus.ticks().unwrap_or(t0);
                            if now.wrapping_sub(t0) >= win_ticks {
                                break;
                            }
                            if let Ok(v) = bus.pmic_read(mon_reg) {
                                let l = (v >> 8) as i8;
                                let r = (v & 0xff) as i8;
                                if l < min_l {
                                    min_l = l;
                                }
                                if l > max_l {
                                    max_l = l;
                                }
                                if r < min_r {
                                    min_r = r;
                                }
                                if r > max_r {
                                    max_r = r;
                                }
                                if l <= -2 {
                                    armed_l = true;
                                } else if armed_l && l >= 2 {
                                    armed_l = false;
                                    if zc_l == 0 {
                                        t_first_l = now;
                                    }
                                    t_last_l = now;
                                    zc_l += 1;
                                }
                                if r <= -2 {
                                    armed_r = true;
                                } else if armed_r && r >= 2 {
                                    armed_r = false;
                                    if zc_r == 0 {
                                        t_first_r = now;
                                    }
                                    t_last_r = now;
                                    zc_r += 1;
                                }
                                if zc_l >= 1 && wf_idx < 24 {
                                    if wf_decim == 0 {
                                        control.dbg_buf[8 + wf_idx] = l as u8;
                                        wf_idx += 1;
                                        wf_decim = 3;
                                    } else {
                                        wf_decim -= 1;
                                    }
                                }
                                n_samples += 1;
                            } else {
                                break;
                            }
                        }
                        if wf_idx == 0 {
                            // If signal never crossed zero, still record 24 raw samples
                            for s_i in 0..24usize {
                                if let Ok(v) = bus.pmic_read(mon_reg) {
                                    control.dbg_buf[8 + s_i] = (v >> 8) as u8;
                                }
                            }
                        }
                        let dt_l = t_last_l.saturating_sub(t_first_l);
                        let hz_l = if zc_l >= 2 && dt_l > 0 {
                            (((zc_l - 1) as u64) * 13_000_000 + (dt_l / 2)) / dt_l
                        } else {
                            0
                        };
                        let dt_r = t_last_r.saturating_sub(t_first_r);
                        let hz_r = if zc_r >= 2 && dt_r > 0 {
                            (((zc_r - 1) as u64) * 13_000_000 + (dt_r / 2)) / dt_r
                        } else {
                            0
                        };
                        control.dbg_buf[0] = min_l as u8;
                        control.dbg_buf[1] = max_l as u8;
                        control.dbg_buf[2] = min_r as u8;
                        control.dbg_buf[3] = max_r as u8;
                        let ns16 = (n_samples.min(65535) as u16).to_le_bytes();
                        control.dbg_buf[4] = ns16[0];
                        control.dbg_buf[5] = ns16[1];
                        control.dbg_buf[6] = zc_l.min(255) as u8;
                        control.dbg_buf[7] = zc_r.min(255) as u8;
                        control.dbg_result =
                            ((hz_r.min(65535) as u32) << 16) | (hz_l.min(65535) as u32);
                        control.dbg_status = 1;
                        let _ = bus.restore();
                    }
                    Err(e) => {
                        control.dbg_status = 0x200 | (e as u32);
                        control.dbg_result = 0;
                    }
                }
            }
            _ => {
                control.dbg_status = 0xff;
                control.dbg_result = 0;
            }
        },
        _ => {
            control.dbg_status = 0xff;
            control.dbg_result = 0;
        }
    }
}

/// Non-blocking live battery voltage read from `MT6358 AUXADC CH0 (BATADC)` via `tone_bus::Hardware`:
/// Triggers `AUXADC_RQST0` (`0x1108 = 0x0001`), polls `AUXADC_ADC0` (`0x1088`) up to 32 times
/// for `RDY` (bit 15 = 1), restores the PMIC bus cleanly, and converts via `(adc * 5400) / 32768` mV.
fn read_mt6358_battery_mv() -> u16 {
    use tone::Bus;
    let Ok(mut bus) = tone_bus::Hardware::prepare(UsbIo {
        start_faults: fault_count(),
    }) else {
        return 3700;
    };
    let _ = bus.pmic_write(0x1108, 0x0001);
    let mut raw = 0u16;
    for _ in 0..32 {
        if let Ok(v) = bus.pmic_read(0x1088) {
            if (v & 0x8000) != 0 {
                raw = v & 0x7fff;
                break;
            }
        } else {
            break;
        }
    }
    if raw == 0 {
        raw = bus.pmic_read(0x1088).unwrap_or(0) & 0x7fff;
    }
    let _ = bus.restore();
    let mv = (((raw as u32) * 5400) >> 15) as u16;
    if (2800..=4450).contains(&mv) {
        mv
    } else {
        3700
    }
}

fn tone_dispatch_touch_point(
    control: &mut usb_control::Control<'_>,
    x: u16,
    y: u16,
    base: usize,
    stride: usize,
    active_row: &mut usize,
    qwave_scroll: &mut usize,
    qwave_tab: &mut usize,
    qwave_zoom: &mut usize,
    qwave_full_mode: &mut bool,
    bat_mv: u16,
    studio_visible: bool,
    tone_gate: bool,
) -> u32 {
    control.touch_last_x = x;
    control.touch_last_y = y;
    control.touch_events = control.touch_events.wrapping_add(1);
    if let Some(semi) = phone_studio::key_at_point(x, y) {
        let cmd = 0x0090 + semi as u16;
        let (outcome, off) = tone_play_live(
            tone_gate,
            studio_visible,
            cmd,
            0,
            base,
            stride,
            active_row,
        );
        control.tracker_plays = control.tracker_plays.saturating_add(1);
        control.tracker_last_cmd = cmd;
        control.tracker_last_outcome = outcome;
        control.tracker_last_off = off;
        control.tracker_active_row = *active_row as u8;
        (0x0100 | (semi as u32)) | (outcome << 16)
    } else if let Some(row) = phone_studio::row_at_point(x, y) {
        let cmd = 0x0081 + row as u16;
        let (outcome, off) = tone_play_live(
            tone_gate,
            studio_visible,
            cmd,
            0,
            base,
            stride,
            active_row,
        );
        control.tracker_plays = control.tracker_plays.saturating_add(1);
        control.tracker_last_cmd = cmd;
        control.tracker_last_outcome = outcome;
        control.tracker_last_off = off;
        control.tracker_active_row = *active_row as u8;
        (0x0200 | (row as u32)) | (outcome << 16)
    } else if let Some(zdelta) = phone_studio::zoom_delta_at_point(x, y) {
        if zdelta < 0 {
            *qwave_zoom = qwave_zoom.saturating_sub(1);
        } else {
            *qwave_zoom = (*qwave_zoom + 1).min(2);
        }
        if studio_visible && *qwave_full_mode {
            let mut canvas = PhoneCanvas { base, stride };
            phone_studio::paint_qwave_doc_only(
                &mut canvas,
                glyph_8x16_hd,
                *qwave_scroll,
                *qwave_tab,
                *qwave_zoom,
                bat_mv,
            );
            flush_dcache_rows(base, stride, 68, 592);
        }
        0x0600 | (*qwave_zoom as u32)
    } else if phone_studio::mode_toggle_at_point(x, y) {
        *qwave_full_mode = !*qwave_full_mode;
        if studio_visible {
            let mut canvas = PhoneCanvas { base, stride };
            if *qwave_full_mode {
                phone_studio::paint_qwave_top_viewport_zoom(
                    &mut canvas,
                    glyph_8x16_hd,
                    *qwave_scroll,
                    *qwave_tab,
                    *qwave_zoom,
                    bat_mv,
                );
            } else {
                phone_studio::paint_studio_top_viewport(&mut canvas, glyph_8x16_hd);
            }
            flush_dcache_rows(base, stride, 64, 598);
        }
        0x0500 | (*qwave_full_mode as u32)
    } else if let Some(tab) = phone_studio::tab_at_point(x, y) {
        *qwave_tab = tab;
        *qwave_scroll = 0;
        if studio_visible {
            let mut canvas = PhoneCanvas { base, stride };
            if *qwave_full_mode {
                phone_studio::paint_qwave_top_viewport_zoom(
                    &mut canvas,
                    glyph_8x16_hd,
                    *qwave_scroll,
                    *qwave_tab,
                    *qwave_zoom,
                    bat_mv,
                );
                flush_dcache_rows(base, stride, 64, 598);
            }
            phone_studio::paint_qwave_browser(
                &mut canvas,
                glyph_8x16_hd,
                *qwave_scroll,
                *qwave_tab,
            );
            flush_dcache_rows(base, stride, 1176, 152);
        }
        0x0300 | (tab as u32)
    } else if let Some(delta) = phone_studio::scroll_at_point(x, y) {
        if delta < 0 {
            *qwave_scroll = qwave_scroll.saturating_sub((-delta) as usize);
        } else {
            *qwave_scroll = (*qwave_scroll + (delta as usize)).min(phone_studio::QWAVE_MAX_SCROLL);
        }
        if studio_visible {
            let mut canvas = PhoneCanvas { base, stride };
            if *qwave_full_mode {
                phone_studio::paint_qwave_doc_only(
                    &mut canvas,
                    glyph_8x16_hd,
                    *qwave_scroll,
                    *qwave_tab,
                    *qwave_zoom,
                    bat_mv,
                );
                flush_dcache_rows(base, stride, 68, 592);
            }
            phone_studio::paint_qwave_browser(
                &mut canvas,
                glyph_8x16_hd,
                *qwave_scroll,
                *qwave_tab,
            );
            flush_dcache_rows(base, stride, 1176, 152);
        }
        0x0400 | (*qwave_scroll as u32)
    } else {
        0
    }
}

// ============================================================================
// GalaxyCore GC7272 TDDI SPI Touchscreen Driver (BLU G75 / Jingtai HKC 6.88")
// ============================================================================
// Bus: spi0@0x1100a000 (SPI_MODE_3: CPOL=1, CPHA=1, ~6 MHz)
// Pins: GPIO23 = RST (output), GPIO1 = IRQ (input), GPIO156 = CS0 (manual GPIO)
// Firmware: E34_HKC6_88_JT_GC7272_0x073C_0x1003_20250609.bin (65,536 bytes, CRC 0x7d422344)

static GC7272_FW: &[u8; 65536] = include_bytes!("gc7272_fw.bin");

const GC7272_IDM_KEY: [u8; 65] = [
    0x4c, 0x70, 0xb8, 0x5c, 0x2e, 0x17, 0x0b, 0x05,
    0x02, 0x81, 0xc0, 0x60, 0x30, 0x98, 0x4c, 0x26,
    0x13, 0x09, 0x84, 0x42, 0xa1, 0xd0, 0xe8, 0x74,
    0x3a, 0x9d, 0x4e, 0xa7, 0x53, 0x29, 0x14, 0x0a,
    0x05, 0x02, 0x81, 0x40, 0xa0, 0xd0, 0xe8, 0xf4,
    0xfa, 0x7d, 0x3e, 0x9f, 0x4f, 0xa7, 0x53, 0x29,
    0x14, 0x0a, 0x05, 0x82, 0xc1, 0xe0, 0xf0, 0xf8,
    0xfc, 0xfe, 0xff, 0x7f, 0x3f, 0x9f, 0xcf, 0xe7,
    0x73,
];

static mut GC7272_BOOT_STATUS: u32 = 0;
static mut GC7272_LAST_PKT: [u8; 32] = [0; 32];
static mut GC7272_B1_COOLDOWN: u32 = 0;

fn gc7272_delay_us(us: u64) {
    let start: u64;
    let freq: u64;
    unsafe {
        asm!("isb", "mrs {}, cntpct_el0", out(reg) start, options(nostack, preserves_flags));
        asm!("mrs {}, cntfrq_el0", out(reg) freq, options(nomem, nostack, preserves_flags));
    }
    let ticks = (freq.max(1_000_000) / 1_000_000).max(1).saturating_mul(us);
    loop {
        let now: u64;
        unsafe {
            asm!("isb", "mrs {}, cntpct_el0", out(reg) now, options(nostack, preserves_flags));
        }
        if now.wrapping_sub(start) >= ticks {
            break;
        }
        core::hint::spin_loop();
    }
}

fn gc7272_spi_fifo_reset() {
    const SPI0_BASE: usize = 0x1100_a000;
    // Hold GPIO158 (SPI0_CLK, bit 30 of GPIO bank 4) HIGH as GPIO output (func 0)
    // while pulsing SPI_CMD_RST (bit 2 of SPI0_BASE + 0x18). This prevents the
    // MediaTek SPI controller's CPOL=0 reset glitch from reaching the physical
    // SPI0_CLK pin while CS (GPIO156) is LOW during multi-chunk transfers.
    safe_mmio_write32(0x1000_5144, 1 << 30); // GPIO158 DOUT = HIGH
    safe_mmio_write32(0x1000_5044, 1 << 30); // GPIO158 DIR = OUT
    let m19 = safe_mmio_read32(0x1000_5430);
    safe_mmio_write32(0x1000_5430, m19 & !0x0f00_0000); // GPIO158 -> func 0 (GPIO HIGH)
    safe_mmio_write32(SPI0_BASE + 0x18, 0x0000_3304); // Assert SPI_CMD_RST
    safe_mmio_write32(SPI0_BASE + 0x18, 0x0000_3300); // Latch CPOL=1, CPHA=1, TXMSBF, RXMSBF
    safe_mmio_write32(0x1000_5430, (m19 & !0x0f00_0000) | 0x0100_0000); // GPIO158 -> func 1 (SPI0_CLK)
}

fn gc7272_spi_setup() {
    const SPI0_BASE: usize = 0x1100_a000;
    // Ungate SPI0 clock in infracfg_ao
    safe_mmio_write32(0x1000_108c, 1 << 21);
    // Ensure GPIO23 (RST) is output HIGH and GPIO1 (IRQ) is input
    safe_mmio_write32(0x1000_5104, 1 << 23);
    safe_mmio_write32(0x1000_5004, 1 << 23);
    safe_mmio_write32(0x1000_5008, 1 << 1);
    // Configure GPIO156 (SPI0_CSB) as manual active-low GPIO output (func 0),
    // preload GPIO158 (SPI0_CLK) DOUT=HIGH / DIR=OUT for glitch-free FIFO resets,
    // and set GPIO155 (MI), GPIO157 (MO), GPIO158 (CLK) to func 1 (SPI0).
    safe_mmio_write32(0x1000_5144, (1 << 28) | (1 << 30)); // CS HIGH, CLK latch HIGH
    safe_mmio_write32(0x1000_5044, (1 << 28) | (1 << 30)); // DIR OUT
    let mode19 = safe_mmio_read32(0x1000_5430);
    safe_mmio_write32(0x1000_5430, (mode19 & !0x0fff_f000) | 0x0110_1000);
    // Reset SPI0 FIFO glitch-free while CS is HIGH, then configure ~6 MHz timing
    gc7272_spi_fifo_reset();
    safe_mmio_write32(SPI0_BASE + 0x00, 0x000a_000a);
    safe_mmio_write32(SPI0_BASE + 0x28, 0x0008_0008);
    safe_mmio_write32(SPI0_BASE + 0x04, 0x0000_000a);
    safe_mmio_write32(SPI0_BASE + 0x0c, 0);
    gc7272_delay_us(50);
}

fn gc7272_spi_write_raw_no_cs(tx: &[u8]) {
    const SPI0_BASE: usize = 0x1100_a000;
    let mut off = 0usize;
    let n = tx.len();
    while off < n {
        if (off & 0x1fff) == 0 {
            pet_mediatek_watchdog();
        }
        let clen = (n - off).min(32);
        let nwords = (clen + 3) / 4;
        gc7272_spi_fifo_reset();
        let _ = safe_mmio_read32(SPI0_BASE + 0x1c);
        safe_mmio_write32(SPI0_BASE + 0x04, (((clen as u32) - 1) << 16) | 0x000a);
        for w_idx in 0..nwords {
            let mut wb = [0u8; 4];
            let base_i = off + w_idx * 4;
            let rem = (clen - w_idx * 4).min(4);
            wb[..rem].copy_from_slice(&tx[base_i..base_i + rem]);
            safe_mmio_write32(SPI0_BASE + 0x10, u32::from_le_bytes(wb));
        }
        safe_mmio_write32(SPI0_BASE + 0x18, 0x0000_3301);
        for _ in 0..16_384 {
            let st0 = safe_mmio_read32(SPI0_BASE + 0x1c);
            let st1 = safe_mmio_read32(SPI0_BASE + 0x20);
            if (st0 & 1) != 0 && (st1 & 1) != 0 {
                break;
            }
        }
        for _ in 0..nwords {
            let _ = safe_mmio_read32(SPI0_BASE + 0x14);
        }
        off += clen;
    }
}

#[repr(C, align(64))]
struct Gc7272DmaBuf([u8; 128]);

static mut GC7272_DMA_TX: Gc7272DmaBuf = Gc7272DmaBuf([0u8; 128]);
static mut GC7272_DMA_RX: Gc7272DmaBuf = Gc7272DmaBuf([0u8; 128]);

#[inline(always)]
fn clean_inval_dcache_range(start: usize, len: usize) {
    // SCTLR_EL1.M == 0 & SCTLR_EL1.C == 0 at bare-metal entry: memory is uncached
    // and directly coherent with SPI0 DMA; dc civac traps on uncached memory, so use dsb sy.
    flush_dcache_range(start, len);
}

fn gc7272_spi_write(tx: &[u8]) {
    const SPI0_BASE: usize = 0x1100_a000;
    if tx.len() == 65536 {
        // Use SPI0 Hardware TX DMA (64 packets x 1024 bytes = 65,536 bytes) matching
        // MediaTek spi-mt65xx + gcore_spi_write so the GC7272 ROM bootloader receives
        // a continuous 65,536-byte stream under one CS-low window without FIFO gaps.
        clean_inval_dcache_range(tx.as_ptr() as usize, tx.len());
        gc7272_spi_fifo_reset();
        let _ = safe_mmio_read32(SPI0_BASE + 0x1c);
        safe_mmio_write32(SPI0_BASE + 0x08, tx.as_ptr() as usize as u32);
        safe_mmio_write32(SPI0_BASE + 0x2c, 0);
        safe_mmio_write32(SPI0_BASE + 0x04, ((1024 - 1) << 16) | ((64 - 1) << 8) | 0x000a);
        safe_mmio_write32(0x1000_5148, 1 << 28); // CS LOW
        // CPHA(8) | CPOL(9) | TX_DMA(11) | TXMSBF(12) | RXMSBF(13) | ACT(0) = 0x0000_3b01
        safe_mmio_write32(SPI0_BASE + 0x18, 0x0000_3b01);
        for step in 0..1_000_000u32 {
            if (step & 0x3fff) == 0 {
                pet_mediatek_watchdog();
            }
            let st0 = safe_mmio_read32(SPI0_BASE + 0x1c);
            let st1 = safe_mmio_read32(SPI0_BASE + 0x20);
            if (st0 & 1) != 0 && (st1 & 1) != 0 {
                break;
            }
        }
        safe_mmio_write32(0x1000_5144, 1 << 28); // CS HIGH
        safe_mmio_write32(SPI0_BASE + 0x18, 0x0000_3300);
        safe_mmio_write32(SPI0_BASE + 0x08, 0);
        return;
    }
    safe_mmio_write32(0x1000_5148, 1 << 28); // CS LOW
    gc7272_spi_write_raw_no_cs(tx);
    safe_mmio_write32(0x1000_5144, 1 << 28); // CS HIGH
}

fn gc7272_spi_read(rx: &mut [u8]) {
    const SPI0_BASE: usize = 0x1100_a000;
    let n = rx.len();
    safe_mmio_write32(0x1000_5148, 1 << 28); // CS LOW
    let mut off = 0usize;
    while off < n {
        let clen = (n - off).min(32);
        let nwords = (clen + 3) / 4;
        gc7272_spi_fifo_reset();
        let _ = safe_mmio_read32(SPI0_BASE + 0x1c);
        safe_mmio_write32(SPI0_BASE + 0x04, (((clen as u32) - 1) << 16) | 0x000a);
        for _ in 0..nwords {
            safe_mmio_write32(SPI0_BASE + 0x10, 0xffff_ffff);
        }
        safe_mmio_write32(SPI0_BASE + 0x18, 0x0000_3301);
        for _ in 0..16_384 {
            let st0 = safe_mmio_read32(SPI0_BASE + 0x1c);
            let st1 = safe_mmio_read32(SPI0_BASE + 0x20);
            if (st0 & 1) != 0 && (st1 & 1) != 0 {
                break;
            }
        }
        for w_idx in 0..nwords {
            let rw = safe_mmio_read32(SPI0_BASE + 0x14).to_le_bytes();
            let base_i = off + w_idx * 4;
            let rem = (clen - w_idx * 4).min(4);
            rx[base_i..base_i + rem].copy_from_slice(&rw[..rem]);
        }
        off += clen;
    }
    safe_mmio_write32(0x1000_5144, 1 << 28); // CS HIGH
}

fn gc7272_enter_idm() {
    gc7272_spi_write(&GC7272_IDM_KEY);
    gc7272_delay_us(1000);
    gc7272_spi_write(&[0x4c, 0x0f, 0xf0]);
    gc7272_delay_us(1000);
}

fn gc7272_exit_idm() {
    gc7272_spi_write(&[0x4c, 0x0a, 0xf5]);
    gc7272_delay_us(1000);
}

fn gc7272_idm_write_reg(addr: u32, data: &[u8]) {
    const SPI0_BASE: usize = 0x1100_a000;
    let hdr = [
        0x4c,
        0x05,
        0xfa,
        (addr >> 24) as u8,
        (addr >> 16) as u8,
        (addr >> 8) as u8,
        addr as u8,
    ];
    let total = 7 + data.len();
    let mut off = 0usize;
    safe_mmio_write32(0x1000_5148, 1 << 28); // CS LOW
    while off < total {
        if (off & 0x3ff) == 0 {
            pet_mediatek_watchdog();
        }
        let clen = (total - off).min(32);
        let nwords = (clen + 3) / 4;
        let mut chunk = [0u8; 32];
        for i in 0..clen {
            let g_idx = off + i;
            chunk[i] = if g_idx < 7 {
                hdr[g_idx]
            } else {
                data[g_idx - 7]
            };
        }
        gc7272_spi_fifo_reset();
        let _ = safe_mmio_read32(SPI0_BASE + 0x1c);
        safe_mmio_write32(SPI0_BASE + 0x04, (((clen as u32) - 1) << 16) | 0x000a);
        for w_idx in 0..nwords {
            let wb: [u8; 4] = chunk[w_idx * 4..w_idx * 4 + 4].try_into().unwrap();
            safe_mmio_write32(SPI0_BASE + 0x10, u32::from_le_bytes(wb));
        }
        safe_mmio_write32(SPI0_BASE + 0x18, 0x0000_3301);
        for _ in 0..16_384 {
            let st0 = safe_mmio_read32(SPI0_BASE + 0x1c);
            let st1 = safe_mmio_read32(SPI0_BASE + 0x20);
            if (st0 & 1) != 0 && (st1 & 1) != 0 {
                break;
            }
        }
        for _ in 0..nwords {
            let _ = safe_mmio_read32(SPI0_BASE + 0x14);
        }
        off += clen;
    }
    safe_mmio_write32(0x1000_5144, 1 << 28); // CS HIGH
}

fn gc7272_idm_read_reg(addr: u32, rx16: &mut [u8; 16]) {
    const SPI0_BASE: usize = 0x1100_a000;
    let tx = [
        0x4c,
        0x04,
        0xfb,
        (addr >> 24) as u8,
        (addr >> 16) as u8,
        (addr >> 8) as u8,
        addr as u8,
        0x00,
        0x00,
        0x00,
        0x00,
        0x00,
        0x00,
        0x00,
        0x00,
        0x00,
    ];
    gc7272_spi_fifo_reset();
    safe_mmio_write32(0x1000_5148, 1 << 28); // CS LOW
    let _ = safe_mmio_read32(SPI0_BASE + 0x1c);
    safe_mmio_write32(SPI0_BASE + 0x04, (15 << 16) | 0x000a);
    for w_idx in 0..4usize {
        let wb: [u8; 4] = tx[w_idx * 4..w_idx * 4 + 4].try_into().unwrap();
        safe_mmio_write32(SPI0_BASE + 0x10, u32::from_le_bytes(wb));
    }
    safe_mmio_write32(SPI0_BASE + 0x18, 0x0000_3301);
    for _ in 0..16_384 {
        let st0 = safe_mmio_read32(SPI0_BASE + 0x1c);
        let st1 = safe_mmio_read32(SPI0_BASE + 0x20);
        if (st0 & 1) != 0 && (st1 & 1) != 0 {
            break;
        }
    }
    for w_idx in 0..4usize {
        let rw = safe_mmio_read32(SPI0_BASE + 0x14).to_le_bytes();
        rx16[w_idx * 4..w_idx * 4 + 4].copy_from_slice(&rw);
    }
    safe_mmio_write32(0x1000_5144, 1 << 28); // CS HIGH
}

fn gc7272_idm_read_id(out_buf: &mut [u8; 32]) -> u32 {
    gc7272_spi_setup();
    gc7272_enter_idm();
    let mut r94 = [0u8; 16];
    gc7272_idm_read_reg(0xc000_0094, &mut r94);
    gc7272_delay_us(1000);
    gc7272_idm_write_reg(0xc000_0096, &[r94[11] & 0x7f]);
    gc7272_delay_us(1000);
    gc7272_idm_write_reg(0xd000_ff00, &[0x55, 0xaa, 0x66]);
    gc7272_delay_us(1000);
    gc7272_idm_write_reg(0xd000_ff00, &[0x10]);
    gc7272_delay_us(1000);
    let mut rid = [0u8; 16];
    gc7272_idm_read_reg(0xd000_0400, &mut rid);
    gc7272_delay_us(1000);
    gc7272_idm_write_reg(0xd000_ff00, &[0x00]);
    gc7272_delay_us(1000);
    gc7272_exit_idm();
    out_buf[0..16].copy_from_slice(&rid);
    out_buf[16..32].copy_from_slice(&r94);
    ((rid[9] as u32) << 8) | (rid[10] as u32)
}

fn gc7272_upgrade_soft_reset() {
    gc7272_enter_idm();
    gc7272_idm_write_reg(0x4000_0050, &[0x00]);
    gc7272_delay_us(1000);
    gc7272_idm_write_reg(0x8002_0000, &[0x00, 0x01, 0x00, 0x00]);
    gc7272_delay_us(1000);
    gc7272_idm_write_reg(
        0x8002_0100,
        &[
            0x02, 0xea, 0x00, 0x40, 0x23, 0xea, 0x00, 0x40,
            0x00, 0x30, 0x54, 0xb3, 0x3c, 0x78, 0x00, 0x00,
        ],
    );
    gc7272_delay_us(1000);
    gc7272_idm_write_reg(0x4000_0050, &[0x01]);
    gc7272_delay_us(1000);
    gc7272_exit_idm();
    gc7272_delay_us(50_000);
}

fn gc7272_hostdownload_idm2() -> u32 {
    gc7272_spi_setup();
    let mut id_buf = [0u8; 32];
    let chip_id = gc7272_idm_read_id(&mut id_buf);
    gc7272_enter_idm();
    for chunk in 0..64usize {
        if (chunk & 7) == 0 {
            pet_mediatek_watchdog();
        }
        let addr = 0x8002_0000u32 + (chunk as u32) * 1024;
        gc7272_idm_write_reg(addr, &GC7272_FW[chunk * 1024..(chunk + 1) * 1024]);
        gc7272_delay_us(1000);
    }
    // gcore_run_pram_fw2: write 0x81 to 0x40000050, wait 1ms, exit IDM
    gc7272_idm_write_reg(0x4000_0050, &[0x81]);
    gc7272_delay_us(1000);
    gc7272_exit_idm();
    gc7272_delay_us(50_000);
    let mut pkt = [0u8; 65];
    gc7272_spi_read(&mut pkt);
    unsafe {
        let mut snap = [0u8; 32];
        snap.copy_from_slice(&pkt[0..32]);
        write_volatile(core::ptr::addr_of_mut!(GC7272_LAST_PKT), snap);
    }
    (chip_id << 16) | ((pkt[0] as u32) << 8) | (pkt[64] as u32)
}

fn gc7272_hostdownload_reset_b1() -> u32 {
    gc7272_spi_setup();
    // Pulse GPIO23 (RST) LOW for 2ms, then HIGH (gcore_update_hostdownload_trigger)
    safe_mmio_write32(0x1000_5108, 1 << 23);
    gc7272_delay_us(2_000);
    safe_mmio_write32(0x1000_5104, 1 << 23);
    for _ in 0..30 {
        gc7272_delay_us(1_000);
        if (safe_mmio_read32(0x1000_5200) >> 1) & 1 != 0 {
            break;
        }
    }
    gc7272_delay_us(1_000);
    let mut b0 = [0u8; 1];
    for _ in 0..16 {
        gc7272_spi_read(&mut b0);
        if b0[0] == 0xb1 {
            break;
        }
        gc7272_delay_us(1_000);
    }
    // Write 65,536-byte firmware image in one CS-low SPI0 Hardware DMA transfer
    gc7272_spi_write(GC7272_FW);
    // Allow 15ms for the GC7272 ROM bootloader to compute CRC32 and boot PRAM FW
    // without CS/CLK disturbance before reading status on the next GPIO1 assertion
    gc7272_delay_us(15_000);
    let mut b1 = [0u8; 1];
    for step in 0..85u32 {
        if (step & 15) == 0 {
            pet_mediatek_watchdog();
        }
        if (safe_mmio_read32(0x1000_5200) >> 1) & 1 != 0 {
            gc7272_spi_read(&mut b1);
            break;
        }
        gc7272_delay_us(1_000);
    }
    if b1[0] == 0 {
        gc7272_spi_read(&mut b1);
    }
    gc7272_delay_us(50_000);
    let mut pkt = [0u8; 65];
    gc7272_spi_read(&mut pkt);
    let gpio1 = ((safe_mmio_read32(0x1000_5200) >> 1) & 1) as u32;
    let status = (b0[0] as u32)
        | ((b1[0] as u32) << 8)
        | (gpio1 << 16)
        | ((pkt[0] as u32) << 24);
    unsafe {
        let mut snap = [0u8; 32];
        snap.copy_from_slice(&pkt[0..32]);
        write_volatile(core::ptr::addr_of_mut!(GC7272_LAST_PKT), snap);
        write_volatile(core::ptr::addr_of_mut!(GC7272_BOOT_STATUS), status);
    }
    status
}

#[derive(Clone, Copy)]
struct Gc7272MultiTouch {
    count: u8,
    x0: u16,
    y0: u16,
    x1: u16,
    y1: u16,
}

fn gc7272_poll_touch_multi(force_read: bool) -> Option<Gc7272MultiTouch> {
    let irq_high = (safe_mmio_read32(0x1000_5200) >> 1) & 1 != 0;
    if !irq_high && !force_read {
        return None;
    }
    let mut pkt = [0u8; 65];
    gc7272_spi_read(&mut pkt);
    let is_wdt = pkt[2..6] == [0x72, 0x02, 0x00, 0x01]
        || pkt[1..5] == [0x72, 0x02, 0x00, 0x01];
    if pkt[0..8] != [0u8; 8]
        && pkt[0..8] != [0xff; 8]
        && !is_wdt
        && !(pkt[0] == 0xb1 && pkt[1..64].iter().all(|&b| b == 0))
    {
        unsafe {
            let mut snap = [0u8; 32];
            snap.copy_from_slice(&pkt[0..32]);
            write_volatile(core::ptr::addr_of_mut!(GC7272_LAST_PKT), snap);
        }
    }
    if pkt[0] == 0xb1 && pkt[1..64].iter().all(|&b| b == 0) {
        unsafe {
            let cd = read_volatile(core::ptr::addr_of!(GC7272_B1_COOLDOWN));
            if cd == 0 {
                let _ = gc7272_hostdownload_reset_b1();
                write_volatile(core::ptr::addr_of_mut!(GC7272_B1_COOLDOWN), 512);
            } else {
                write_volatile(core::ptr::addr_of_mut!(GC7272_B1_COOLDOWN), cd - 1);
            }
        }
        return None;
    }
    if pkt[0..8] == [0u8; 8] || pkt[0..8] == [0xff; 8] || is_wdt {
        return None;
    }
    let mut count = 0u8;
    let mut x0 = 0u16;
    let mut y0 = 0u16;
    let mut x1 = 0u16;
    let mut y1 = 0u16;
    for slot in 0..5usize {
        let off = slot * 6;
        let b0 = pkt[off];
        if b0 != 0 && b0 != 0xff && b0 <= 0x57 {
            let raw_x = ((pkt[off + 1] as u16) << 4) | ((pkt[off + 3] as u16) >> 4);
            let raw_y = ((pkt[off + 2] as u16) << 4) | ((pkt[off + 3] as u16) & 0x0f);
            if raw_x < 720 && raw_y < 1640 && (raw_x != 0 || raw_y != 0) {
                if count == 0 {
                    x0 = raw_x;
                    y0 = raw_y;
                    count = 1;
                } else if count == 1 {
                    x1 = raw_x;
                    y1 = raw_y;
                    count = 2;
                    break;
                }
            }
        }
    }
    if count > 0 {
        return Some(Gc7272MultiTouch { count, x0, y0, x1, y1 });
    }
    if (pkt[0] & 0x80) != 0 {
        return Some(Gc7272MultiTouch {
            count: 0,
            x0: 0,
            y0: 0,
            x1: 0,
            y1: 0,
        });
    }
    let sum = pkt[0..64].iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
    if pkt[64] != sum.wrapping_neg() {
        return None;
    }
    Some(Gc7272MultiTouch {
        count: 0,
        x0: 0,
        y0: 0,
        x1: 0,
        y1: 0,
    })
}

fn gc7272_poll_touch(force_read: bool) -> Option<(u16, u16, bool)> {
    let mt = gc7272_poll_touch_multi(force_read)?;
    if mt.count > 0 {
        Some((mt.x0, mt.y0, true))
    } else {
        Some((0, 0, false))
    }
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
