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

const SEMITONE_DIVS: [u16; 24] = [
    24, 22, 21, 20, 19, 18, 17, 16, 15, 14, 13, 12, 11, 10, 9, 8, 8, 7, 6, 6, 5, 4, 4, 3,
];

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
            140_000
        } else {
            (dur_10ms * 10_000).clamp(20_000, 1_500_000)
        };
        custom_step = [tone::TrackerStep {
            note: "JAM",
            key_index,
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

fn tone_debug_exec(
    control: &mut usb_control::Control<'_>,
    op_val: u16,
    idx: u16,
    base: usize,
    stride: usize,
    active_row: &mut usize,
    qwave_scroll: &mut usize,
    qwave_tab: &mut usize,
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
                for _ in 0..4096 {
                    let st0 = safe_mmio_read32(SPI0_BASE + 0x1c);
                    let st1 = safe_mmio_read32(SPI0_BASE + 0x20);
                    if st0 & 1 != 0 || st1 & 1 == 0 {
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
                    control.dbg_status = 1;
                    control.dbg_result = (0x0100 | (semi as u32)) | (outcome << 16);
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
                    control.dbg_status = 1;
                    control.dbg_result = (0x0200 | (row as u32)) | (outcome << 16);
                } else if let Some(tab) = phone_studio::tab_at_point(x, y) {
                    *qwave_tab = tab;
                    *qwave_scroll = 0;
                    if studio_visible {
                        let mut canvas = PhoneCanvas { base, stride };
                        phone_studio::paint_qwave_browser(
                            &mut canvas,
                            glyph_8x16_hd,
                            *qwave_scroll,
                            *qwave_tab,
                        );
                        flush_dcache_range(base, stride * SCREEN_H * 4);
                    }
                    control.dbg_status = 1;
                    control.dbg_result = 0x0300 | (tab as u32);
                } else if let Some(delta) = phone_studio::scroll_at_point(x, y) {
                    if delta < 0 {
                        *qwave_scroll = qwave_scroll.saturating_sub((-delta) as usize);
                    } else {
                        *qwave_scroll =
                            (*qwave_scroll + (delta as usize)).min(phone_studio::QWAVE_MAX_SCROLL);
                    }
                    if studio_visible {
                        let mut canvas = PhoneCanvas { base, stride };
                        phone_studio::paint_qwave_browser(
                            &mut canvas,
                            glyph_8x16_hd,
                            *qwave_scroll,
                            *qwave_tab,
                        );
                        flush_dcache_range(base, stride * SCREEN_H * 4);
                    }
                    control.dbg_status = 1;
                    control.dbg_result = 0x0400 | (*qwave_scroll as u32);
                } else {
                    control.dbg_status = 1;
                    control.dbg_result = 0;
                }
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
                            flush_dcache_range(base, stride * SCREEN_H * 4);
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
                    phone_studio::paint_qwave_browser(
                        &mut canvas,
                        glyph_8x16_hd,
                        *qwave_scroll,
                        *qwave_tab,
                    );
                    flush_dcache_range(base, stride * SCREEN_H * 4);
                }
                control.dbg_status = 1;
                control.dbg_result = ((*qwave_tab as u32) << 8) | (*qwave_scroll as u32);
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
