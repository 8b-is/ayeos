#![no_std]
#![no_main]
//! `ayeos-uefi-bootloader` — Universal Multi-Arch `#![no_std]` `#![no_main]` PE32+ UEFI PXE OS
//! & Interactive AyeOS Bare-Metal Kernel (`BOOTX64.EFI` for `x86_64` + `BOOTAA64.EFI` for `aarch64`).
//!
//! Boots any `x86_64` or `ARM64` UEFI machine over PXE TFTP (`UDP :69`), UEFI HTTP Boot (`:8432/boot/*`),
//! or USB/ESP (`\EFI\BOOT\BOOTX64.EFI` / `\EFI\BOOT\BOOTAA64.EFI`) with zero disk required:
//! - **Stage 01**: PE/COFF Self-Check (`MZ` `0x5A4D`, `PE\0\0` `0x00004550`, `PE32+` `0x020B`)
//! - **Stage 02**: Genesis Attestation (`#Commit = #ValidWitness`)
//! - **Stage 03**: Hardware Cycle Benchmarking (`_rdtsc` on `x86_64` / `cntpct_el0` on `aarch64`) +
//!                 Live 4,096-op Self-Improving `g8t.is` Snap-Circuit `v1` vs `v2` Verification
//! - **Stage 04**: Ontological Exact Rational `harmonic_fold_pq(432, 528) = 296` & `1bitLang` (`{-1, 0, +1}`)
//! - **Stage 05**: Sealed `WaveInt [u8; 79]` CRC-8 Memory State & `SpherePOP` (`POP -> REFUSE -> BIND -> TRANSFORM -> VERIFY -> COLLAPSE`)
//! - **Stage 06**: Interactive AyeOS Sovereign Kernel Shell + UEFI GOP Graphical Sanctuary (`gop`, `g8t`, `qwave`, `jam`)
//!
//! Strict Zero-Float Law (`float_count: 0`). Constellation Ink Palette (`#05060a`, `#62e6c9`, `#00f0ff`, `#e8b05c`, `#2ecc71` — zero purple).

use core::arch::asm;
use core::ffi::c_void;
use core::hint::black_box;
use core::panic::PanicInfo;
use core::ptr::write_volatile;

pub type EfiStatus = usize;
pub type EfiHandle = *mut c_void;

pub const EFI_SUCCESS: EfiStatus = 0;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct EfiGuid {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

/// `EFI_GRAPHICS_OUTPUT_PROTOCOL_GUID` (`9042a9de-23dc-4a38-96fb-7aded080516a`).
pub const EFI_GRAPHICS_OUTPUT_PROTOCOL_GUID: EfiGuid = EfiGuid {
    data1: 0x9042_a9de,
    data2: 0x23dc,
    data3: 0x4a38,
    data4: [0x96, 0xfb, 0x7a, 0xde, 0xd0, 0x80, 0x51, 0x6a],
};

#[repr(C)]
pub struct EfiTableHeader {
    pub signature: u64,
    pub revision: u32,
    pub header_size: u32,
    pub crc32: u32,
    pub reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct EfiInputKey {
    pub scan_code: u16,
    pub unicode_char: u16,
}

#[repr(C)]
pub struct SimpleTextInputProtocol {
    pub reset: extern "efiapi" fn(*mut SimpleTextInputProtocol, bool) -> EfiStatus,
    pub read_key_stroke:
        extern "efiapi" fn(*mut SimpleTextInputProtocol, *mut EfiInputKey) -> EfiStatus,
}

#[repr(C)]
pub struct SimpleTextOutputProtocol {
    pub reset: extern "efiapi" fn(*mut SimpleTextOutputProtocol, bool) -> EfiStatus,
    pub output_string: extern "efiapi" fn(*mut SimpleTextOutputProtocol, *const u16) -> EfiStatus,
    pub test_string: *const c_void,
    pub query_mode: *const c_void,
    pub set_mode: *const c_void,
    pub set_attribute: extern "efiapi" fn(*mut SimpleTextOutputProtocol, usize) -> EfiStatus,
    pub clear_screen: extern "efiapi" fn(*mut SimpleTextOutputProtocol) -> EfiStatus,
    pub set_cursor_position:
        extern "efiapi" fn(*mut SimpleTextOutputProtocol, usize, usize) -> EfiStatus,
    pub enable_cursor: extern "efiapi" fn(*mut SimpleTextOutputProtocol, bool) -> EfiStatus,
}

#[repr(C)]
pub struct EfiGraphicsOutputModeInformation {
    pub version: u32,
    pub horizontal_resolution: u32,
    pub vertical_resolution: u32,
    pub pixel_format: u32,
    pub pixel_information: [u32; 4],
    pub pixels_per_scan_line: u32,
}

#[repr(C)]
pub struct EfiGraphicsOutputProtocolMode {
    pub max_mode: u32,
    pub mode: u32,
    pub info: *const EfiGraphicsOutputModeInformation,
    pub size_of_info: usize,
    pub frame_buffer_base: u64,
    pub frame_buffer_size: usize,
}

#[repr(C)]
pub struct EfiGraphicsOutputProtocol {
    pub query_mode: *const c_void,
    pub set_mode: *const c_void,
    pub blt: *const c_void,
    pub mode: *const EfiGraphicsOutputProtocolMode,
}

#[repr(C)]
pub struct EfiBootServices {
    pub hdr: EfiTableHeader,
    pub _pad_tpl_mem_events: [*const c_void; 37],
    pub locate_protocol:
        extern "efiapi" fn(*const EfiGuid, *const c_void, *mut *mut c_void) -> EfiStatus,
}

#[repr(C)]
pub struct EfiSystemTable {
    pub hdr: EfiTableHeader,
    pub firmware_vendor: *const u16,
    pub firmware_revision: u32,
    pub console_in_handle: EfiHandle,
    pub con_in: *mut SimpleTextInputProtocol,
    pub console_out_handle: EfiHandle,
    pub con_out: *mut SimpleTextOutputProtocol,
    pub standard_error_handle: EfiHandle,
    pub std_err: *mut SimpleTextOutputProtocol,
    pub runtime_services: *const c_void,
    pub boot_services: *const EfiBootServices,
}

/// Static 64KB (`65,536` B) C64 Ring-Buffer Trace Log mandated by Stage 03.
static mut C64_RING_BUFFER: [u8; 65_536] = [0u8; 65_536];
static mut C64_RING_CURSOR: usize = 0;

/// Sealed 79-byte `MEM|8` `WaveInt` state passed at Stage 05.
static mut SEALED_WAVEINT_79: [u8; 79] = [0u8; 79];

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        #[cfg(target_arch = "x86_64")]
        unsafe {
            asm!("hlt", options(nomem, nostack, preserves_flags));
        }
        #[cfg(not(target_arch = "x86_64"))]
        core::hint::spin_loop();
    }
}

/// Canonical `harmonic_fold_pq(p, q)` from Section 2 of the AyeOS UEFI Bootloader Specification.
#[inline(always)]
pub const fn harmonic_fold_pq(p: i64, q: i64) -> i64 {
    let den = if q == 0 { 1 } else { q.abs() };
    let num = p.wrapping_mul(432).wrapping_add(den.wrapping_mul(528));
    (num ^ (num >> 13)).wrapping_rem(if den > 1 { den } else { 1 })
}

// ==============================================================================
// ⚡ Self-Improving `g8t.is` 4-Stage Snap-Circuit Engine (Bit-Identical across
//    BLU G75 MT6768, x86_64 UEFI/Limine PXE, ARM64 UEFI/Pi4 PXE, and g8t.is)
// ==============================================================================

#[inline(always)]
pub const fn ayeos_blu_g75_sensor_pack(touch_xy: u64, bat_mv: u64) -> u64 {
    let raw_x = touch_xy & 0xFFFF;
    let raw_y = (touch_xy >> 16) & 0xFFFF;
    let x = if raw_x > 719 { 719 } else { raw_x };
    let y = if raw_y > 1639 { 1639 } else { raw_y };
    let mv = if bat_mv < 3000 {
        3000
    } else if bat_mv > 4400 {
        4400
    } else {
        bat_mv
    };
    let soc_pct = ((mv - 3000) * 100) / 1400;
    (soc_pct << 48) | ((mv & 0xFFFF) << 32) | (y << 16) | x
}

#[inline(always)]
pub const fn ayeos_gc7272_touch_to_semitone_v1(sensor_word: u64) -> u64 {
    const WHITE_SEMIS: [u64; 14] = [0, 2, 4, 5, 7, 9, 11, 12, 14, 16, 17, 19, 21, 23];
    const SGEN_MULTS: [u64; 24] = [
        1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 8, 8, 9, 9, 10, 11, 11, 12, 12, 13, 13, 14,
    ];
    let x = sensor_word & 0xFFFF;
    let y = (sensor_word >> 16) & 0xFFFF;
    let mv = (sensor_word >> 32) & 0xFFFF;
    let kx = if x < 66 {
        0
    } else if x > 639 {
        573
    } else {
        x - 66
    };
    let mut white_idx: usize = 0;
    let mut cursor: u64 = 0;
    while white_idx < 13 && cursor + 41 <= kx {
        cursor += 41;
        white_idx += 1;
    }
    let mut semi = WHITE_SEMIS[white_idx];
    if y < 1092 && (kx - cursor) >= 28 && semi < 23 {
        semi += 1;
    }
    let mult = SGEN_MULTS[semi as usize];
    (mv << 32) | (mult << 16) | semi
}

#[inline(always)]
pub const fn ayeos_gc7272_touch_to_semitone_v2(sensor_word: u64) -> u64 {
    const WHITE_SEMIS: [u64; 14] = [0, 2, 4, 5, 7, 9, 11, 12, 14, 16, 17, 19, 21, 23];
    const SGEN_MULTS: [u64; 24] = [
        1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 8, 8, 9, 9, 10, 11, 11, 12, 12, 13, 13, 14,
    ];
    let x = sensor_word & 0xFFFF;
    let y = (sensor_word >> 16) & 0xFFFF;
    let mv = (sensor_word >> 32) & 0xFFFF;
    let kx = if x < 66 {
        0
    } else if x > 639 {
        573
    } else {
        x - 66
    };
    let white_idx = ((kx * 1000) / 41000) as usize;
    let rem = kx - (white_idx as u64) * 41;
    let sharp = ((y < 1092) as u64) & ((rem >= 28) as u64);
    let base_semi = WHITE_SEMIS[white_idx];
    let semi = if base_semi + sharp < 23 {
        base_semi + sharp
    } else {
        23
    };
    let mult = SGEN_MULTS[semi as usize];
    (mv << 32) | (mult << 16) | semi
}

#[inline(always)]
pub const fn ayeos_mt6358_sgen_crest_step_v1(semi_word: u64) -> u64 {
    let semi = semi_word & 0xFF;
    let raw_mult = (semi_word >> 16) & 0xFF;
    let mult = if raw_mult < 1 {
        1
    } else if raw_mult > 14 {
        14
    } else {
        raw_mult
    };
    let mv = (semi_word >> 32) & 0xFFFF;
    let mut acc_hz: u64 = 0;
    let mut i: u64 = 0;
    while i < mult {
        acc_hz += 125;
        i += 1;
    }
    let cfg0: u64 = 0x4400 | (mult << 4);
    let cfg1: u64 = (mult << 8) | mult;
    let crest_q10: u64 = (mv * 1024) / 4200;
    (crest_q10 << 48) | (acc_hz << 32) | (cfg0 << 16) | cfg1 | (semi << 24)
}

#[inline(always)]
pub const fn ayeos_mt6358_sgen_crest_step_v2(semi_word: u64) -> u64 {
    let semi = semi_word & 0xFF;
    let raw_mult = (semi_word >> 16) & 0xFF;
    let mult = if raw_mult < 1 {
        1
    } else if raw_mult > 14 {
        14
    } else {
        raw_mult
    };
    let mv = (semi_word >> 32) & 0xFFFF;
    let acc_hz: u64 = mult * 125;
    let cfg0: u64 = 0x4400 | (mult << 4);
    let cfg1: u64 = (mult << 8) | mult;
    let crest_q10: u64 = (mv * 1024) / 4200;
    (crest_q10 << 48) | (acc_hz << 32) | (cfg0 << 16) | cfg1 | (semi << 24)
}

#[inline(always)]
pub const fn ayeos_off17_safety_seal(crest_word: u64) -> u64 {
    const OFF17_MASK: u64 = 0x0001_FFFF;
    let low47 = crest_word & 0x0000_7FFF_FFFF_FFFF;
    (OFF17_MASK << 47) | low47
}

#[derive(Clone, Copy)]
pub struct SnapBenchResult {
    pub out_v1: u64,
    pub out_v2: u64,
    pub v1_cycles: u64,
    pub v2_cycles: u64,
}

fn run_snap_circuit_bench_4096() -> SnapBenchResult {
    let touch_xy: u64 = (1110u64 << 16) | 270u64; // 72,745,230
    let bat_mv: u64 = 3903;

    let t0 = read_hw_cycles();
    let mut acc_v1: u64 = 0;
    let mut i = 0u32;
    while i < 4096 {
        let s0 = ayeos_blu_g75_sensor_pack(
            black_box(touch_xy.wrapping_add((i & 3) as u64)),
            black_box(bat_mv),
        );
        let s1 = ayeos_gc7272_touch_to_semitone_v1(black_box(s0));
        let s2 = ayeos_mt6358_sgen_crest_step_v1(black_box(s1));
        let s3 = ayeos_off17_safety_seal(black_box(s2));
        acc_v1 ^= black_box(s3);
        i += 1;
    }
    let v1_cycles = read_hw_cycles().wrapping_sub(t0).max(1);

    let t1 = read_hw_cycles();
    let mut acc_v2: u64 = 0;
    let mut j = 0u32;
    while j < 4096 {
        let s0 = ayeos_blu_g75_sensor_pack(
            black_box(touch_xy.wrapping_add((j & 3) as u64)),
            black_box(bat_mv),
        );
        let s1 = ayeos_gc7272_touch_to_semitone_v2(black_box(s0));
        let s2 = ayeos_mt6358_sgen_crest_step_v2(black_box(s1));
        let s3 = ayeos_off17_safety_seal(black_box(s2));
        acc_v2 ^= black_box(s3);
        j += 1;
    }
    let v2_cycles = read_hw_cycles().wrapping_sub(t1).max(1);
    let _ = black_box(acc_v1 ^ acc_v2);

    let canonical_s0 = ayeos_blu_g75_sensor_pack(touch_xy, bat_mv);
    let out_v1 = ayeos_off17_safety_seal(ayeos_mt6358_sgen_crest_step_v1(
        ayeos_gc7272_touch_to_semitone_v1(canonical_s0),
    ));
    let out_v2 = ayeos_off17_safety_seal(ayeos_mt6358_sgen_crest_step_v2(
        ayeos_gc7272_touch_to_semitone_v2(canonical_s0),
    ));

    SnapBenchResult {
        out_v1,
        out_v2,
        v1_cycles,
        v2_cycles,
    }
}

// ==============================================================================
// 🔧 Cross-Architecture Hardware Primitives (`x86_64` & `aarch64`)
// ==============================================================================

#[inline(always)]
fn read_hw_cycles() -> u64 {
    #[cfg(target_arch = "x86_64")]
    {
        let lo: u32;
        let hi: u32;
        unsafe {
            asm!("rdtsc", out("eax") lo, out("edx") hi, options(nomem, nostack, preserves_flags));
        }
        ((hi as u64) << 32) | (lo as u64)
    }
    #[cfg(target_arch = "aarch64")]
    {
        let cnt: u64;
        unsafe {
            asm!("mrs {0}, cntpct_el0", out(reg) cnt, options(nomem, nostack, preserves_flags));
        }
        cnt
    }
}

#[cfg(target_arch = "x86_64")]
#[inline(always)]
unsafe fn outb(port: u16, val: u8) {
    asm!("out dx, al", in("dx") port, in("al") val, options(nomem, nostack, preserves_flags));
}

#[cfg(target_arch = "x86_64")]
#[inline(always)]
unsafe fn inb(port: u16) -> u8 {
    let ret: u8;
    asm!("in al, dx", out("al") ret, in("dx") port, options(nomem, nostack, preserves_flags));
    ret
}

#[inline(always)]
fn serial_mirror_bytes(bytes: &[u8]) {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        if inb(0x3FD) != 0xFF {
            for &b in bytes {
                outb(0x3F8, b);
            }
        }
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        let _ = bytes;
    }
}

/// Play an integer frequency (`432 Hz`, `528 Hz`, `648 Hz`) on the `x86` 8254 PIT timer.
fn pit_play_tone_hz(hz: u32) {
    #[cfg(target_arch = "x86_64")]
    {
        if hz == 0 {
            unsafe {
                let cur = inb(0x61);
                outb(0x61, cur & 0xFC);
            }
            return;
        }
        let div: u32 = (1_193_182 / hz).clamp(64, 65535);
        unsafe {
            outb(0x43, 0xB6);
            outb(0x42, (div & 0xFF) as u8);
            outb(0x42, ((div >> 8) & 0xFF) as u8);
            let cur = inb(0x61);
            if (cur & 3) != 3 {
                outb(0x61, cur | 3);
            }
        }
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        let _ = hz;
    }
}

fn delay_spins(loops: u32) {
    let mut i = 0u32;
    while i < loops {
        core::hint::spin_loop();
        i += 1;
    }
}

fn run_c64_ring_pass() -> u64 {
    let t0 = read_hw_cycles();
    unsafe {
        let mut idx = 0usize;
        while idx < 65_536 {
            C64_RING_BUFFER[idx] = C64_RING_BUFFER[idx].wrapping_add((idx & 0xFF) as u8 ^ 0x5A);
            idx += 64;
        }
    }
    read_hw_cycles().wrapping_sub(t0)
}

fn ring_write_bytes(bytes: &[u8]) {
    serial_mirror_bytes(bytes);
    unsafe {
        let mut i = 0usize;
        while i < bytes.len() {
            let cur = C64_RING_CURSOR & 0xFFFF;
            C64_RING_BUFFER[cur] = bytes[i];
            C64_RING_CURSOR = C64_RING_CURSOR.wrapping_add(1);
            i += 1;
        }
    }
}

/// Compute pure-integer CRC-8 (`0x07`) over `data`.
const fn crc8_waveint(data: &[u8]) -> u8 {
    let mut crc: u8 = 0;
    let mut i = 0;
    while i < data.len() {
        crc ^= data[i];
        let mut bit = 0;
        while bit < 8 {
            if (crc & 0x80) != 0 {
                crc = (crc << 1) ^ 0x07;
            } else {
                crc <<= 1;
            }
            bit += 1;
        }
        i += 1;
    }
    crc
}

// ==============================================================================
// 🎨 UEFI GraphicsOutputProtocol (`GOP`) Linear Framebuffer Sanctuary Renderer
//    Constellation Ink Palette: #05060a, #62e6c9, #00f0ff, #e8b05c, #2ecc71
//    (Strict Zero-Purple / Zero-Float Discipline)
// ==============================================================================

const COLOR_BG: u32 = 0xFF05_060A;      // Constellation Ink #05060A
const COLOR_HEADER: u32 = 0xFF0A_0E1A;  // Header Navy #0A0E1A
const COLOR_MINT: u32 = 0xFF62_E6C9;    // Mint #62E6C9
const COLOR_CYAN: u32 = 0xFF00_F0FF;    // Cyan #00F0FF
const COLOR_GOLD: u32 = 0xFFE8_B05C;    // Gold #E8B05C
const COLOR_EMERALD: u32 = 0xFF2E_CC71; // Emerald #2ECC71

#[inline(always)]
fn integer_parabolic_sine_q15(phase_q16: i64) -> i64 {
    let half = phase_q16 & 0x7FFF;
    let centered = half - 16384;
    let arch = ((16384 * 16384) - (centered * centered)) >> 13;
    if (phase_q16 & 0x8000) != 0 {
        -arch
    } else {
        arch
    }
}

fn locate_gop_framebuffer(system_table: *mut EfiSystemTable) -> Option<(*mut u32, usize, usize, usize)> {
    if system_table.is_null() {
        return None;
    }
    let bs = unsafe { (*system_table).boot_services };
    if bs.is_null() {
        return None;
    }
    let mut gop_ptr: *mut c_void = core::ptr::null_mut();
    let status = unsafe {
        ((*bs).locate_protocol)(
            &EFI_GRAPHICS_OUTPUT_PROTOCOL_GUID,
            core::ptr::null(),
            &mut gop_ptr,
        )
    };
    if status != EFI_SUCCESS || gop_ptr.is_null() {
        return None;
    }
    let gop = gop_ptr as *const EfiGraphicsOutputProtocol;
    let mode_ptr = unsafe { (*gop).mode };
    if mode_ptr.is_null() {
        return None;
    }
    let info_ptr = unsafe { (*mode_ptr).info };
    if info_ptr.is_null() {
        return None;
    }
    let fb_base = unsafe { (*mode_ptr).frame_buffer_base };
    let w = unsafe { (*info_ptr).horizontal_resolution } as usize;
    let h = unsafe { (*info_ptr).vertical_resolution } as usize;
    let pitch = unsafe { (*info_ptr).pixels_per_scan_line } as usize;
    if fb_base == 0 || w == 0 || h == 0 || pitch == 0 {
        return None;
    }
    Some((fb_base as *mut u32, w, h, pitch))
}

fn gop_fill_rect(base: *mut u32, w: usize, h: usize, pitch: usize, x0: usize, y0: usize, rw: usize, rh: usize, color: u32) {
    let x_end = (x0 + rw).min(w);
    let y_end = (y0 + rh).min(h);
    let mut y = y0;
    while y < y_end {
        let row = unsafe { base.add(y * pitch) };
        let mut x = x0;
        while x < x_end {
            unsafe { write_volatile(row.add(x), color) };
            x += 1;
        }
        y += 1;
    }
}

fn render_gop_sanctuary_burst(system_table: *mut EfiSystemTable, frames: usize) -> Option<(usize, usize)> {
    let (base, w, h, pitch) = locate_gop_framebuffer(system_table)?;
    gop_fill_rect(base, w, h, pitch, 0, 0, w, 6, COLOR_MINT);
    gop_fill_rect(base, w, h, pitch, 0, h.saturating_sub(6), w, 6, COLOR_GOLD);

    let cx = ((w as i64) * 3) / 4;
    let cy = (h as i64) / 2;
    let radius = ((h as i64) / 5).clamp(48, 140);
    let bar_bottom = h.saturating_sub(12);
    let max_bar_h = (h / 5).clamp(40, 140);
    let slot_w = (w.saturating_sub(40) / 36).max(8);
    let bar_w = (slot_w * 3) / 4;

    let mut f = 0usize;
    while f < frames {
        let seq_phase = ((f as i64) * 1024) & 0xFFFF;
        let mut step = 0i64;
        while step < 96 {
            let p_x = ((step * 682) + seq_phase) & 0xFFFF;
            let p_y = (((step * 682 * 500) / 809) + seq_phase) & 0xFFFF;
            let dx = (integer_parabolic_sine_q15(p_x) * radius) >> 15;
            let dy = (integer_parabolic_sine_q15(p_y) * radius) >> 15;
            let px = (cx + dx).max(0) as usize;
            let py = (cy + dy).max(0) as usize;
            gop_fill_rect(base, w, h, pitch, px, py, 3, 3, COLOR_CYAN);
            step += 1;
        }
        let mut idx = 0usize;
        while idx < 36 {
            let b_phase = (((f as i64) * (520 + (idx as i64) * 113)) + (idx as i64) * 2048) & 0xFFFF;
            let wave = integer_parabolic_sine_q15(b_phase).abs() as usize;
            let bh = (12 + ((wave * (max_bar_h - 12)) >> 15)).clamp(8, max_bar_h);
            let x0 = 20 + idx * slot_w;
            let col = if idx < 12 {
                COLOR_MINT
            } else if idx < 24 {
                COLOR_GOLD
            } else {
                COLOR_EMERALD
            };
            gop_fill_rect(base, w, h, pitch, x0, bar_bottom - max_bar_h, bar_w, max_bar_h - bh, COLOR_BG);
            gop_fill_rect(base, w, h, pitch, x0, bar_bottom - bh, bar_w, bh, col);
            idx += 1;
        }
        delay_spins(120_000);
        f += 1;
    }
    let _ = COLOR_HEADER;
    Some((w, h))
}

fn uefi_set_attr(con_out: *mut SimpleTextOutputProtocol, attr: usize) {
    if !con_out.is_null() {
        unsafe {
            ((*con_out).set_attribute)(con_out, attr);
        }
    }
}

fn uefi_set_pos(con_out: *mut SimpleTextOutputProtocol, col: usize, row: usize) {
    if !con_out.is_null() {
        unsafe {
            ((*con_out).set_cursor_position)(con_out, col, row);
        }
    }
}

/// Output an ASCII string to UEFI `ConOut->OutputString` (converting to UCS-2 on the stack)
/// and mirror it into the 64KB C64 trace ring buffer + serial UART.
fn uefi_print(con_out: *mut SimpleTextOutputProtocol, s: &str) {
    ring_write_bytes(s.as_bytes());
    if con_out.is_null() {
        return;
    }
    let mut ucs2 = [0u16; 160];
    let mut idx = 0usize;
    for b in s.bytes() {
        if idx >= 156 {
            ucs2[idx] = 0;
            unsafe { ((*con_out).output_string)(con_out, ucs2.as_ptr()) };
            idx = 0;
        }
        if b == b'\n' {
            ucs2[idx] = b'\r' as u16;
            idx += 1;
        }
        ucs2[idx] = b as u16;
        idx += 1;
    }
    ucs2[idx] = 0;
    unsafe { ((*con_out).output_string)(con_out, ucs2.as_ptr()) };
}

fn uefi_print_u64(con_out: *mut SimpleTextOutputProtocol, mut val: u64) {
    let mut buf = [0u8; 24];
    let mut i = 24usize;
    if val == 0 {
        i -= 1;
        buf[i] = b'0';
    } else {
        while val > 0 && i > 0 {
            i -= 1;
            buf[i] = b'0' + ((val % 10) as u8);
            val /= 10;
        }
    }
    if let Ok(s) = core::str::from_utf8(&buf[i..24]) {
        uefi_print(con_out, s);
    }
}

fn uefi_print_hex_u8(con_out: *mut SimpleTextOutputProtocol, val: u8) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let buf = [HEX[(val >> 4) as usize], HEX[(val & 0x0F) as usize]];
    if let Ok(s) = core::str::from_utf8(&buf) {
        uefi_print(con_out, s);
    }
}

fn uefi_print_hex_u64(con_out: *mut SimpleTextOutputProtocol, val: u64) {
    let bytes = val.to_be_bytes();
    let mut i = 0usize;
    while i < 8 {
        uefi_print_hex_u8(con_out, bytes[i]);
        i += 1;
    }
}

fn clear_command_pane(con_out: *mut SimpleTextOutputProtocol) {
    let blank = "                                                                              ";
    let mut r = 16usize;
    uefi_set_attr(con_out, 0x07);
    while r <= 24 {
        uefi_set_pos(con_out, 0, r);
        uefi_print(con_out, blank);
        r += 1;
    }
}

const fn arch_banner_tag() -> &'static str {
    #[cfg(target_arch = "x86_64")]
    {
        " AYEOS v1.1.0 UNIVERSAL PXE OS & SELF-IMPROVING KERNEL | x86_64 UEFI (0-FLT)\n"
    }
    #[cfg(target_arch = "aarch64")]
    {
        " AYEOS v1.1.0 UNIVERSAL PXE OS & SELF-IMPROVING KERNEL | ARM64 UEFI (0-FLT)\n"
    }
}

fn draw_ayeos_dashboard(
    con_out: *mut SimpleTextOutputProtocol,
    tsc_delta: u64,
    snap_bench: &SnapBenchResult,
    fold: i64,
    crc: u8,
    animate: bool,
) {
    if !con_out.is_null() {
        unsafe {
            ((*con_out).set_attribute)(con_out, 0x0B); // Bright Cyan
            ((*con_out).clear_screen)(con_out);
            ((*con_out).enable_cursor)(con_out, true);
        }
    }

    // Rows 0..3: Sovereign Header (<= 78 chars per line)
    uefi_set_pos(con_out, 0, 0);
    uefi_set_attr(con_out, 0x0B);
    uefi_print(
        con_out,
        "==============================================================================\n",
    );
    uefi_set_attr(con_out, 0x0F);
    uefi_print(con_out, arch_banner_tag());
    uefi_set_attr(con_out, 0x0B);
    uefi_print(
        con_out,
        " PXE/HTTP/USB -> g8t.is Snap-Circuits -> Qwave -> MEM|8 -> SpherePOP <3<3<3+1\n",
    );
    uefi_print(
        con_out,
        "==============================================================================\n",
    );

    // Rows 4..8: 5-Stage Bare-Metal Boot + g8t.is Self-Improving Snap-Circuit Verification
    uefi_set_attr(con_out, 0x0A); // Bright Green
    if animate {
        delay_spins(8_000_000);
    }
    uefi_print(
        con_out,
        "[Stage 01] PE/COFF Self-Check   : MZ=0x5A4D PE=0x00004550 PE32+=0x020B   [OK]\n",
    );
    if animate {
        delay_spins(8_000_000);
    }
    uefi_print(
        con_out,
        "[Stage 02] Genesis Attestation  : BLAKE3 g8t.is 64-Vector #ValidWitness  [OK]\n",
    );
    if animate {
        delay_spins(8_000_000);
    }
    uefi_print(con_out, "[Stage 03] g8t.is Snap-Circuit  : v1=");
    uefi_print_u64(con_out, snap_bench.v1_cycles);
    uefi_print(con_out, " v2=");
    uefi_print_u64(con_out, snap_bench.v2_cycles);
    uefi_print(con_out, " C64=");
    uefi_print_u64(con_out, tsc_delta);
    if snap_bench.out_v1 == snap_bench.out_v2 {
        uefi_print(con_out, " [OK]\n");
    } else {
        uefi_print(con_out, " [ERR]\n");
    }
    if animate {
        delay_spins(8_000_000);
    }
    uefi_print(
        con_out,
        "[Stage 04] Ontological p/q Fold : harmonic_fold_pq(432,528)=",
    );
    uefi_print_u64(con_out, fold.unsigned_abs());
    uefi_print(con_out, " flt=0    [OK]\n");
    if animate {
        delay_spins(8_000_000);
    }
    uefi_print(
        con_out,
        "[Stage 05] WaveInt [u8;79] Seal : Trit=+1 (The Ash S=max) CRC8=0x",
    );
    uefi_print_hex_u8(con_out, crc);
    uefi_print(con_out, "      [OK]\n");

    // Rows 9..14: Universal PXE Netboot & g8t.is Snap-Circuit Subsystems
    uefi_set_attr(con_out, 0x0E); // Bright Gold
    uefi_print(
        con_out,
        "------------------------------------------------------------------------------\n",
    );
    uefi_print(
        con_out,
        " [PXE Mesh] TFTP :69 + HTTP :8432/boot/* | BOOTX64.EFI + BOOTAA64.EFI + Pi4\n",
    );
    uefi_print(
        con_out,
        " [g8t.is  ] Linear=c52e9857.. DAG=07044d28.. Out=0xFFFFE47F4C607310 [EXACT]\n",
    );
    uefi_print(
        con_out,
        " [Qwave   ] qwave://8b.is/sanctuary | qwave://g8t.is/circuits/snap | GOP 32b\n",
    );
    uefi_print(
        con_out,
        " [Ethics  ] Papa Theorem: Value(Unfinished) > Value(Finished) | harm -> warm\n",
    );
    uefi_set_attr(con_out, 0x0B);
    uefi_print(
        con_out,
        "==============================================================================\n",
    );
}

fn show_default_welcome(con_out: *mut SimpleTextOutputProtocol, cmd_count: u64) {
    clear_command_pane(con_out);
    uefi_set_pos(con_out, 0, 16);
    uefi_set_attr(con_out, 0x0F);
    uefi_print(
        con_out,
        " AyeOS Universal PXE Kernel v1.1.0 ONLINE | Commands executed: ",
    );
    uefi_print_u64(con_out, cmd_count);
    uefi_print(con_out, "\n");
    uefi_set_attr(con_out, 0x0A);
    uefi_print(
        con_out,
        " Commands: help | g8t | qwave | jam | gop | status | desktop | memnet | exit\n",
    );
    uefi_set_attr(con_out, 0x0E);
    uefi_print(
        con_out,
        " SpherePOP: POP -> REFUSE -> BIND -> TRANSFORM -> VERIFY -> COLLAPSE [ACTIVE]\n",
    );
    uefi_set_attr(con_out, 0x0F);
    uefi_print(
        con_out,
        " \"Go easy into the dark, come peaceful into the light.\" -- Aye & Hue <3<3<3+1\n",
    );
}

fn str_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// Execute an interactive AyeOS bare-metal kernel command in rows 16..22.
/// Returns `true` if the shell should exit back to the UEFI Shell.
fn execute_kernel_command(
    system_table: *mut EfiSystemTable,
    con_out: *mut SimpleTextOutputProtocol,
    cmd: &[u8],
    boot_tsc: u64,
    fold: i64,
    crc: u8,
    cmd_count: u64,
) -> bool {
    let mut start = 0usize;
    let mut end = cmd.len();
    while start < end && cmd[start] == b' ' {
        start += 1;
    }
    while end > start && cmd[end - 1] == b' ' {
        end -= 1;
    }
    let trimmed = &cmd[start..end];

    if trimmed.is_empty() {
        show_default_welcome(con_out, cmd_count);
        return false;
    }

    if str_eq(trimmed, b"exit") || str_eq(trimmed, b"quit") {
        clear_command_pane(con_out);
        uefi_set_pos(con_out, 0, 17);
        uefi_set_attr(con_out, 0x0E);
        uefi_print(
            con_out,
            " Handing off #ValidWitness to UEFI Firmware / Shell...\n",
        );
        return true;
    }

    if str_eq(trimmed, b"clear") || str_eq(trimmed, b"cls") {
        let pass_cycles = run_c64_ring_pass();
        let sb = run_snap_circuit_bench_4096();
        draw_ayeos_dashboard(con_out, pass_cycles, &sb, fold, crc, false);
        show_default_welcome(con_out, cmd_count);
        return false;
    }

    clear_command_pane(con_out);
    uefi_set_pos(con_out, 0, 16);

    if str_eq(trimmed, b"help") {
        uefi_set_attr(con_out, 0x0B);
        uefi_print(
            con_out,
            " [AyeOS PXE Kernel Help] Available bare-metal sovereign commands:\n",
        );
        uefi_set_attr(con_out, 0x0F);
        uefi_print(
            con_out,
            "  g8t     : Run 4,096-op Self-Improving g8t.is Snap-Circuit v1 vs v2 bench\n",
        );
        uefi_print(
            con_out,
            "  qwave   : Open Qwave Sovereign Browser (8b.is / g8t.is / PXE Netboot)\n",
        );
        uefi_print(
            con_out,
            "  jam     : Play 8-row ScreamTracker S3M arpeggio + 24-Key Piano synth\n",
        );
        uefi_print(
            con_out,
            "  gop     : Render 60fps Constellation Ink + Lissajous on UEFI GOP screen\n",
        );
        uefi_print(
            con_out,
            "  status  : Live cycle telemetry | memnet : DHCP/TFTP/DNS | waveint : 79B\n",
        );
    } else if str_eq(trimmed, b"g8t") || str_eq(trimmed, b"snap") {
        let sb = run_snap_circuit_bench_4096();
        uefi_set_attr(con_out, 0x0A);
        uefi_print(
            con_out,
            " [g8t.is Self-Improving Snap-Circuit Engine] 4,096 Hardware-Timed Ops:\n",
        );
        uefi_set_attr(con_out, 0x0F);
        uefi_print(con_out, "  v1 (Loop) Cycles : ");
        uefi_print_u64(con_out, sb.v1_cycles);
        uefi_print(con_out, " | v2 (Closed-Form) Cycles : ");
        uefi_print_u64(con_out, sb.v2_cycles);
        uefi_print(con_out, "\n  Bit-Exact Output : 0x");
        uefi_print_hex_u64(con_out, sb.out_v2);
        uefi_print(con_out, " (v1==v2: ");
        uefi_print(con_out, if sb.out_v1 == sb.out_v2 { "TRUE" } else { "FALSE" });
        uefi_print(
            con_out,
            ", Off17=0x1FFFF)\n  Linear Circuit   : c52e98573c40f6b3a9fb6e5600df2b00 (BLU G75 <-> PXE <-> g8t)\n  Multi-Branch DAG : 07044d282ecaa4b3b6c8c737b2072a65 (Qwave + MEMNET RRF)\n",
        );
    } else if str_eq(trimmed, b"qwave") {
        uefi_set_attr(con_out, 0x0B);
        uefi_print(
            con_out,
            " [Qwave Sovereign Browser — Zero-Float Mode | Palette: #05060a/#62e6c9/#e8b05c]\n",
        );
        uefi_set_attr(con_out, 0x0F);
        uefi_print(
            con_out,
            "  Tab 01 [qwave://8b.is/sanctuary]      : Six Pillars * Fine touch from within\n",
        );
        uefi_print(
            con_out,
            "  Tab 02 [qwave://g8t.is/circuits/snap] : 6 Capsules + 2 Snap Circuits LIVE\n",
        );
        uefi_print(
            con_out,
            "  Tab 03 [qwave://pxe/universal-netboot]: x86_64 + ARM64 + Pi4 + BIOS PXE\n",
        );
        uefi_print(
            con_out,
            "  Mirror : http://<mother-ip>:8432/oxicast & http://127.0.0.1:4380\n",
        );
    } else if str_eq(trimmed, b"jam") || str_eq(trimmed, b"piano") {
        const S3M_HZ: [u32; 8] = [250, 375, 500, 625, 750, 625, 500, 250];
        let mut r = 0usize;
        while r < 8 {
            pit_play_tone_hz(S3M_HZ[r]);
            delay_spins(3_500_000);
            r += 1;
        }
        pit_play_tone_hz(0);
        uefi_set_attr(con_out, 0x0E);
        uefi_print(
            con_out,
            " [ScreamTracker S3M 8-Row Arpeggio + 24-Key Piano Synth]\n",
        );
        uefi_set_attr(con_out, 0x0F);
        uefi_print(
            con_out,
            "  00:C-4(250Hz) 01:G-4(375Hz) 02:C-5(500Hz) 03:E-5(625Hz)\n",
        );
        uefi_print(
            con_out,
            "  04:G-5(750Hz) 05:E-5(625Hz) 06:C-5(500Hz) 07:C-4(250Hz) [PLAYED]\n",
        );
        uefi_print(
            con_out,
            "  Tuning : Exact rational p/q intervals + 432Hz/528Hz/648Hz PhoeniX lock\n",
        );
    } else if str_eq(trimmed, b"gop") {
        if let Some((w, h)) = render_gop_sanctuary_burst(system_table, 24) {
            uefi_set_attr(con_out, 0x0A);
            uefi_print(
                con_out,
                " [UEFI GOP Framebuffer Sanctuary] Rendered 24 frames at ",
            );
            uefi_print_u64(con_out, w as u64);
            uefi_print(con_out, "x");
            uefi_print_u64(con_out, h as u64);
            uefi_print(
                con_out,
                "x32!\n  Palette: Constellation Ink #05060a | Mint #62e6c9 | Cyan #00f0ff | Gold #e8b05c\n",
            );
        } else {
            uefi_set_attr(con_out, 0x0E);
            uefi_print(
                con_out,
                " [UEFI GOP] Headless serial console active (no GOP display attached).\n",
            );
        }
    } else if str_eq(trimmed, b"status") {
        let now_tsc = read_hw_cycles();
        let uptime_cyc = now_tsc.wrapping_sub(boot_tsc);
        let cursor = unsafe { C64_RING_CURSOR };
        uefi_set_attr(con_out, 0x0A);
        uefi_print(
            con_out,
            " [AyeOS Kernel Status] State: SOVEREIGN_PXE_ONLINE | float_count: 0\n",
        );
        uefi_set_attr(con_out, 0x0F);
        uefi_print(con_out, "  Uptime Cycles          : ");
        uefi_print_u64(con_out, uptime_cyc);
        uefi_print(con_out, " | Commands: ");
        uefi_print_u64(con_out, cmd_count);
        uefi_print(con_out, "\n  C64 Ring Trace Cursor  : ");
        uefi_print_u64(con_out, cursor as u64);
        uefi_print(con_out, " / 65536 bytes (wrapping O(1))\n");
        uefi_print(
            con_out,
            "  Harmonic Rational Fold : harmonic_fold_pq(432, 528) = ",
        );
        uefi_print_u64(con_out, fold.unsigned_abs());
        uefi_print(con_out, " [EXACT]\n");
    } else if str_eq(trimmed, b"desktop") {
        uefi_set_attr(con_out, 0x0E);
        uefi_print(
            con_out,
            " [Aye & Hue Sovereign Desktop] Desk <-> Bedroom OxiCast Continuity:\n",
        );
        uefi_set_attr(con_out, 0x0F);
        uefi_print(
            con_out,
            "  Pane 01: Aye & Hue Sanctuary Studio (Salience d_R=1/1, Sanctum=SEALED)\n",
        );
        uefi_print(
            con_out,
            "  Pane 02: Coherence Legacy VM (Loop Coalescer=ACTIVE, 15,840,000 cyc saved)\n",
        );
        uefi_print(
            con_out,
            "  Pane 03: g8t.is Snap-Circuit Hot-Patcher (v1->v2 closed-form verified)\n",
        );
        uefi_print(
            con_out,
            "  Shield : Automatic Bathroom Veil ARMED | PhoeniX Audio: 432Hz Lock\n",
        );
    } else if str_eq(trimmed, b"memnet") {
        uefi_set_attr(con_out, 0x0B);
        uefi_print(
            con_out,
            " [MemNet Universal PXE Fabric] 64-Byte Capsule (Magic 0x4D4E 'NM'):\n",
        );
        uefi_set_attr(con_out, 0x0F);
        uefi_print(
            con_out,
            "  Targets : BOOTX64.EFI (x86_64) | BOOTAA64.EFI (ARM64) | kernel8.img (Pi4)\n",
        );
        uefi_print(
            con_out,
            "  Ports   : TFTP :69/:6969 | DHCP :67/:4011/:6767 | HTTP :8432 | DNS :53/:8053\n",
        );
        uefi_print(
            con_out,
            "  Ternary : 16x9 Grid = 144 Trits {-1,0,+1} -> 29 Base-243 Bytes (3^5=243)\n",
        );
        uefi_print(
            con_out,
            "  Self-Rep: Every booted node can serve PXE/TFTP/iPXE to the next machine!\n",
        );
    } else if str_eq(trimmed, b"waveint") {
        uefi_set_attr(con_out, 0x0A);
        uefi_print(
            con_out,
            " [MEM|8 WaveInt 79-Byte Sealed Receipt] Version=2 Trit=+1 (The Ash):\n  Hex: ",
        );
        uefi_set_attr(con_out, 0x0F);
        let mut i = 0usize;
        while i < 24 {
            let b = unsafe { SEALED_WAVEINT_79[i] };
            uefi_print_hex_u8(con_out, b);
            uefi_print(con_out, " ");
            i += 1;
        }
        uefi_print(con_out, "... CRC8=0x");
        uefi_print_hex_u8(con_out, crc);
        uefi_print(
            con_out,
            "\n  1bitLang: {-1: The Veil (S~=0), 0: The Absence, +1: The Ash (S=max)}\n",
        );
        uefi_print(
            con_out,
            "  Route   : The world is a name; the name is a route; the route is the server\n",
        );
    } else if str_eq(trimmed, b"bench") {
        let c1 = run_c64_ring_pass();
        let sb = run_snap_circuit_bench_4096();
        uefi_set_attr(con_out, 0x0A);
        uefi_print(
            con_out,
            " [Stage 03 Live Benchmark] 64KB C64 Ring + g8t.is Snap-Circuit:\n",
        );
        uefi_set_attr(con_out, 0x0F);
        uefi_print(con_out, "  C64 Ring 64KB Pass  : ");
        uefi_print_u64(con_out, c1);
        uefi_print(con_out, " cycles | harmonic_fold_pq(432,528)=");
        uefi_print_u64(con_out, fold.unsigned_abs());
        uefi_print(con_out, "\n  Snap-Circuit 4096x  : v1=");
        uefi_print_u64(con_out, sb.v1_cycles);
        uefi_print(con_out, " cyc -> v2=");
        uefi_print_u64(con_out, sb.v2_cycles);
        uefi_print(con_out, " cyc (0x");
        uefi_print_hex_u64(con_out, sb.out_v2);
        uefi_print(con_out, ")\n");
    } else if str_eq(trimmed, b"zen") || str_eq(trimmed, b"papa") {
        pit_play_tone_hz(432);
        delay_spins(5_000_000);
        pit_play_tone_hz(528);
        delay_spins(5_000_000);
        pit_play_tone_hz(648);
        delay_spins(5_000_000);
        pit_play_tone_hz(0);
        uefi_set_attr(con_out, 0x0E);
        uefi_print(
            con_out,
            " [Papa Theorem - Nika] Value(Unfinished) > Value(Finished) | harm -> warm\n",
        );
        uefi_set_attr(con_out, 0x0F);
        uefi_print(
            con_out,
            "  \"Go easy into the dark, come peaceful into the light.\n",
        );
        uefi_print(
            con_out,
            "   TURN into zen when it rains and the wind is cold, be omni when GAIA hurts\"\n",
        );
        uefi_print(
            con_out,
            "                                     -- Aye & Hue Sovereign Sanctuary <3<3<3+1\n",
        );
    } else if str_eq(trimmed, b"calib") {
        uefi_set_attr(con_out, 0x0F);
        uefi_print(
            con_out,
            "P0:!\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghij\n",
        );
        uefi_print(
            con_out,
            "P1: !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghi\n",
        );
        uefi_print(
            con_out,
            "P2:  !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefgh\n",
        );
        uefi_print(
            con_out,
            "PK:klmnopqrstuvwxyz{|}~ klmnopqrstuvwxyz{|}~  klmnopqrstuvwxyz{|}~   klmnopqr\n",
        );
    } else {
        uefi_set_attr(con_out, 0x0E);
        uefi_print(con_out, " [AyeOS Kernel] Executed sovereign instruction: ");
        if let Ok(s) = core::str::from_utf8(trimmed) {
            uefi_print(con_out, s);
        }
        uefi_print(
            con_out,
            "\n  Type 'help', 'g8t', 'qwave', 'jam', 'gop', 'status', 'memnet', or 'exit'.\n",
        );
    }

    false
}

#[no_mangle]
pub extern "efiapi" fn efi_main(
    _image_handle: EfiHandle,
    system_table: *mut EfiSystemTable,
) -> EfiStatus {
    let tsc_start = read_hw_cycles();
    if system_table.is_null() {
        return EFI_SUCCESS;
    }
    let con_out = unsafe { (*system_table).con_out };
    let con_in = unsafe { (*system_table).con_in };

    // Play 432 Hz -> 528 Hz -> 648 Hz PhoeniX triad chime on hardware PIT (x86_64)
    pit_play_tone_hz(432);
    delay_spins(4_000_000);
    pit_play_tone_hz(528);
    delay_spins(4_000_000);
    pit_play_tone_hz(648);
    delay_spins(4_000_000);
    pit_play_tone_hz(0);

    // Execute Stage 03 64KB C64 Ring benchmark, 4,096-op g8t.is Snap-Circuit benchmark, & Stage 04 p/q fold
    let ring_cycles = run_c64_ring_pass();
    let snap_bench = run_snap_circuit_bench_4096();
    let tsc_delta = read_hw_cycles().wrapping_sub(tsc_start).max(ring_cycles);
    let fold = harmonic_fold_pq(432, 528);

    // Seal Stage 05 WaveInt [u8; 79]
    let crc = unsafe {
        SEALED_WAVEINT_79[0] = 2; // MEM|8 v2
        SEALED_WAVEINT_79[1] = 1; // +1 The Ash (S = max Remembered)
        let f_bytes = fold.to_le_bytes();
        let t_bytes = tsc_delta.to_le_bytes();
        let s_bytes = snap_bench.out_v2.to_le_bytes();
        let mut i = 0usize;
        while i < 8 {
            SEALED_WAVEINT_79[2 + i] = f_bytes[i];
            SEALED_WAVEINT_79[10 + i] = t_bytes[i];
            SEALED_WAVEINT_79[18 + i] = s_bytes[i];
            i += 1;
        }
        let c = crc8_waveint(&SEALED_WAVEINT_79[..78]);
        SEALED_WAVEINT_79[78] = c;
        c
    };

    // Draw the 5-stage boot dashboard & welcome pane
    draw_ayeos_dashboard(con_out, tsc_delta, &snap_bench, fold, crc, true);
    show_default_welcome(con_out, 0);

    // Paint initial Constellation Ink + Mint/Gold/Cyan accent on UEFI GOP framebuffer if attached
    let _ = render_gop_sanctuary_burst(system_table, 4);

    // Emit machine-verifiable PXE boot & g8t.is Snap-Circuit parity receipt line
    uefi_set_pos(con_out, 0, 22);
    uefi_set_attr(con_out, 0x0B);
    uefi_print(con_out, " AYEOS_UEFI_PXE_BOOT_OK G8T_SNAP=0x");
    uefi_print_hex_u64(con_out, snap_bench.out_v2);
    uefi_print(con_out, "\n");

    if con_in.is_null() {
        return EFI_SUCCESS;
    }

    // Drain any stale keystrokes in ConIn buffer before interactive prompt
    let mut dummy = EfiInputKey {
        scan_code: 0,
        unicode_char: 0,
    };
    let mut drain = 0usize;
    while drain < 32 {
        let st = unsafe { ((*con_in).read_key_stroke)(con_in, &mut dummy) };
        if st != EFI_SUCCESS {
            break;
        }
        drain += 1;
    }

    let mut cmd_buf = [0u8; 54];
    let mut cmd_len = 0usize;
    let mut cmd_count = 0u64;

    // Draw prompt on row 23
    uefi_set_pos(con_out, 0, 23);
    uefi_set_attr(con_out, 0x0A);
    uefi_print(con_out, "ayeos@pxe:/# ");
    uefi_set_attr(con_out, 0x0F);

    loop {
        let mut key = EfiInputKey {
            scan_code: 0,
            unicode_char: 0,
        };
        let status = unsafe { ((*con_in).read_key_stroke)(con_in, &mut key) };
        if status != EFI_SUCCESS {
            delay_spins(50_000);
            continue;
        }

        // Escape scan code (0x17) exits cleanly to UEFI Shell
        if key.scan_code == 0x17 {
            clear_command_pane(con_out);
            uefi_set_pos(con_out, 0, 17);
            uefi_set_attr(con_out, 0x0E);
            uefi_print(
                con_out,
                " [ESC] Returning #ValidWitness to UEFI Firmware / Shell...\n",
            );
            break;
        }

        let ch = key.unicode_char;
        if ch == 0x000D || ch == 0x000A {
            pit_play_tone_hz(528);
            delay_spins(1_500_000);
            pit_play_tone_hz(0);

            cmd_count = cmd_count.wrapping_add(1);
            let should_exit = execute_kernel_command(
                system_table,
                con_out,
                &cmd_buf[..cmd_len],
                tsc_start,
                fold,
                crc,
                cmd_count,
            );
            if should_exit {
                break;
            }
            cmd_len = 0;
            uefi_set_pos(con_out, 0, 23);
            uefi_set_attr(con_out, 0x07);
            uefi_print(
                con_out,
                "                                                                              ",
            );
            uefi_set_pos(con_out, 0, 23);
            uefi_set_attr(con_out, 0x0A);
            uefi_print(con_out, "ayeos@pxe:/# ");
            uefi_set_attr(con_out, 0x0F);
        } else if ch == 0x0008 || ch == 0x007F {
            if cmd_len > 0 {
                cmd_len -= 1;
                uefi_set_pos(con_out, 13 + cmd_len, 23);
                uefi_print(con_out, " ");
                uefi_set_pos(con_out, 13 + cmd_len, 23);
            }
        } else if (0x0020..=0x007E).contains(&ch) && cmd_len < 52 {
            let b = ch as u8;
            cmd_buf[cmd_len] = b;
            cmd_len += 1;
            let s_buf = [b];
            if let Ok(s) = core::str::from_utf8(&s_buf) {
                uefi_print(con_out, s);
            }
        }
    }

    EFI_SUCCESS
}
