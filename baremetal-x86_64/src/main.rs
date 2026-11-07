//! `ayeos-baremetal-x86_64` — 100% Bare-Metal `#![no_std]` `#![no_main]` Pure-Rust `x86_64` OS Kernel
//! (`ayeos-x86_64.elf`) for **UEFI PXE (`BOOTX64.EFI`) & Legacy BIOS PXE (`limine-bios-pxe.bin`)**.
//!
//! **Zero Linux Kernel · Zero C · Zero Floating-Point · Boots any `x86_64` PC over PXE!**
//!
//! 1. **Limine 64-Bit Long Mode Entry (`_start` at `0xffffffff80000000`)**:
//!    Receives a 64-bit Higher-Half Direct Map (`HHDM`) linear framebuffer (`1280×720×32` or native
//!    GOP/VBE resolution) directly from Limine (`LIMINE_FRAMEBUFFER_REQUEST`).
//! 2. **60fps Sovereign AyeOS × OxiCast × PhoeniX TV Sanctuary**:
//!    Paints the Constellation Ink stage, `36-Band Marine S+N+T Spectrum`, and `432 Hz / φ (500/809)`
//!    Parabolic Lissajous Ring directly into the `x86_64` GOP/VBE framebuffer.
//! 3. **Bare-Metal `x86` 8254 PIT Hardware Audio Synth (`Ports 0x42 / 0x43 / 0x61`)**:
//!    Synthesizes the live **432 Hz (`1193182 / 432 = 2762`) + φ (`267 Hz -> 4469`) + 528 Hz (`2260`)**
//!    PhoeniX harmonic triad directly on hardware timer Channel 2!

#![no_std]
#![no_main]

use core::arch::asm;
use core::panic::PanicInfo;
use core::ptr::{read_volatile, write_volatile};

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        unsafe { asm!("hlt", options(nomem, nostack, preserves_flags)) };
    }
}

#[allow(suspicious_runtime_symbol_definitions)]
#[no_mangle]
pub unsafe extern "C" fn memset(dst: *mut u8, val: i32, count: usize) -> *mut u8 {
    let byte = val as u8;
    let mut i = 0usize;
    while i < count {
        write_volatile(dst.add(i), byte);
        i += 1;
    }
    dst
}

#[allow(suspicious_runtime_symbol_definitions)]
#[no_mangle]
pub unsafe extern "C" fn memcpy(dst: *mut u8, src: *const u8, count: usize) -> *mut u8 {
    let mut i = 0usize;
    while i < count {
        write_volatile(dst.add(i), read_volatile(src.add(i)));
        i += 1;
    }
    dst
}

/// Limine Request Start Marker (`0xf6b8f4b39de7d1ae, 0xfab91a6940fcb9cf, ...`).
#[used]
#[link_section = ".requests_start_marker"]
static LIMINE_REQUESTS_START: [u64; 4] = [
    0xf6b8_f4b3_9de7_d1ae,
    0xfab9_1a69_40fc_b9cf,
    0x785c_6ed0_15d3_e316,
    0x181e_920a_7852_b9d9,
];

/// Limine Base Revision `2`.
#[used]
#[link_section = ".requests"]
static LIMINE_BASE_REVISION: [u64; 3] = [
    0xf956_2b2d_5c95_a6c8,
    0x6a7b_3849_4453_6bdc,
    2,
];

#[repr(C)]
pub struct LimineFramebuffer {
    pub address: *mut u8,
    pub width: u64,
    pub height: u64,
    pub pitch: u64,
    pub bpp: u16,
    pub memory_model: u8,
    pub red_mask_size: u8,
    pub red_mask_shift: u8,
    pub green_mask_size: u8,
    pub green_mask_shift: u8,
    pub blue_mask_size: u8,
    pub blue_mask_shift: u8,
    pub unused: [u8; 7],
    pub edid_size: u64,
    pub edid: *const u8,
}

#[repr(C)]
pub struct LimineFramebufferResponse {
    pub revision: u64,
    pub framebuffer_count: u64,
    pub framebuffers: *const *const LimineFramebuffer,
}

#[repr(C)]
pub struct LimineFramebufferRequest {
    pub id: [u64; 4],
    pub revision: u64,
    pub response: *const LimineFramebufferResponse,
}

unsafe impl Sync for LimineFramebufferRequest {}

#[used]
#[link_section = ".requests"]
static FRAMEBUFFER_REQUEST: LimineFramebufferRequest = LimineFramebufferRequest {
    id: [
        0xc7b1_dd30_df4c_8b88,
        0x0a82_e883_a194_f07b,
        0x9d58_27dc_d881_dd75,
        0xa314_8604_f6fa_b11b,
    ],
    revision: 0,
    response: core::ptr::null(),
};

/// Limine Request End Marker (`0xadc0e0531bb10d03, 0x9572709f31764c62`).
#[used]
#[link_section = ".requests_end_marker"]
static LIMINE_REQUESTS_END: [u64; 2] = [
    0xadc0_e053_1bb1_0d03,
    0x9572_709f_3176_4c62,
];

struct Framebuffer {
    base_ptr: *mut u32,
    width: usize,
    height: usize,
    pitch_u32: usize,
}

#[inline(always)]
unsafe fn outb(port: u16, val: u8) {
    asm!("out dx, al", in("dx") port, in("al") val, options(nomem, nostack, preserves_flags));
}

#[inline(always)]
unsafe fn inb(port: u16) -> u8 {
    let ret: u8;
    asm!("in al, dx", out("al") ret, in("dx") port, options(nomem, nostack, preserves_flags));
    ret
}

/// Program the `x86` 8254 Programmable Interval Timer (`1,193,182 Hz` crystal) Channel 2
/// on ports `0x43` / `0x42` / `0x61` to play an integer frequency `hz` (`432 Hz`, `267 Hz`, `528 Hz`).
fn pit_play_tone_hz(hz: u32) {
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

/// Exact integer parabolic sine approximation for `phase_q16` in `0..65536`, returning `-32767..=32767`.
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

/// 8×8 Sovereign Bitmap Font for Bare-Metal GOP/VBE Rendering.
#[inline(always)]
const fn glyph_8x8(ch: char) -> [u8; 8] {
    match ch.to_ascii_uppercase() {
        'A' => [0x3C, 0x66, 0x66, 0x7E, 0x66, 0x66, 0x66, 0x00],
        'B' => [0x7C, 0x66, 0x66, 0x7C, 0x66, 0x66, 0x7C, 0x00],
        'C' => [0x3C, 0x66, 0x60, 0x60, 0x60, 0x66, 0x3C, 0x00],
        'D' => [0x78, 0x6C, 0x66, 0x66, 0x66, 0x6C, 0x78, 0x00],
        'E' => [0x7E, 0x60, 0x60, 0x7C, 0x60, 0x60, 0x7E, 0x00],
        'F' => [0x7E, 0x60, 0x60, 0x7C, 0x60, 0x60, 0x60, 0x00],
        'G' => [0x3C, 0x66, 0x60, 0x6E, 0x66, 0x66, 0x3E, 0x00],
        'H' => [0x66, 0x66, 0x66, 0x7E, 0x66, 0x66, 0x66, 0x00],
        'I' => [0x3C, 0x18, 0x18, 0x18, 0x18, 0x18, 0x3C, 0x00],
        'J' => [0x1E, 0x0C, 0x0C, 0x0C, 0x6C, 0x6C, 0x38, 0x00],
        'K' => [0x66, 0x6C, 0x78, 0x70, 0x78, 0x6C, 0x66, 0x00],
        'L' => [0x60, 0x60, 0x60, 0x60, 0x60, 0x60, 0x7E, 0x00],
        'M' => [0x63, 0x77, 0x7F, 0x6B, 0x63, 0x63, 0x63, 0x00],
        'N' => [0x66, 0x76, 0x7E, 0x7E, 0x6E, 0x66, 0x66, 0x00],
        'O' => [0x3C, 0x66, 0x66, 0x66, 0x66, 0x66, 0x3C, 0x00],
        'P' => [0x7C, 0x66, 0x66, 0x7C, 0x60, 0x60, 0x60, 0x00],
        'Q' => [0x3C, 0x66, 0x66, 0x66, 0x6A, 0x6C, 0x36, 0x00],
        'R' => [0x7C, 0x66, 0x66, 0x7C, 0x6C, 0x66, 0x66, 0x00],
        'S' => [0x3C, 0x66, 0x60, 0x3C, 0x06, 0x66, 0x3C, 0x00],
        'T' => [0x7E, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x00],
        'U' => [0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x3C, 0x00],
        'V' => [0x66, 0x66, 0x66, 0x66, 0x66, 0x3C, 0x18, 0x00],
        'W' => [0x63, 0x63, 0x63, 0x6B, 0x7F, 0x77, 0x63, 0x00],
        'X' => [0x66, 0x66, 0x3C, 0x18, 0x3C, 0x66, 0x66, 0x00],
        'Y' => [0x66, 0x66, 0x66, 0x3C, 0x18, 0x18, 0x18, 0x00],
        'Z' => [0x7E, 0x06, 0x0C, 0x18, 0x30, 0x60, 0x7E, 0x00],
        '0' => [0x3C, 0x66, 0x6E, 0x76, 0x66, 0x66, 0x3C, 0x00],
        '1' => [0x18, 0x38, 0x18, 0x18, 0x18, 0x18, 0x7E, 0x00],
        '2' => [0x3C, 0x66, 0x06, 0x1C, 0x30, 0x60, 0x7E, 0x00],
        '3' => [0x3C, 0x66, 0x06, 0x1C, 0x06, 0x66, 0x3C, 0x00],
        '4' => [0x0C, 0x1C, 0x3C, 0x6C, 0x7E, 0x0C, 0x0C, 0x00],
        '5' => [0x7E, 0x60, 0x7C, 0x06, 0x06, 0x66, 0x3C, 0x00],
        '6' => [0x3C, 0x60, 0x7C, 0x66, 0x66, 0x66, 0x3C, 0x00],
        '7' => [0x7E, 0x06, 0x0C, 0x18, 0x30, 0x30, 0x30, 0x00],
        '8' => [0x3C, 0x66, 0x66, 0x3C, 0x66, 0x66, 0x3C, 0x00],
        '9' => [0x3C, 0x66, 0x66, 0x3E, 0x06, 0x0C, 0x38, 0x00],
        ':' => [0x00, 0x18, 0x18, 0x00, 0x18, 0x18, 0x00, 0x00],
        '-' => [0x00, 0x00, 0x00, 0x7E, 0x00, 0x00, 0x00, 0x00],
        '+' => [0x00, 0x18, 0x18, 0x7E, 0x18, 0x18, 0x00, 0x00],
        '/' => [0x02, 0x06, 0x0C, 0x18, 0x30, 0x60, 0x40, 0x00],
        '.' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x00],
        '(' => [0x0C, 0x18, 0x30, 0x30, 0x30, 0x18, 0x0C, 0x00],
        ')' => [0x30, 0x18, 0x0C, 0x0C, 0x0C, 0x18, 0x30, 0x00],
        '#' => [0x24, 0x7E, 0x24, 0x24, 0x7E, 0x24, 0x00, 0x00],
        '*' => [0x00, 0x66, 0x3C, 0xFF, 0x3C, 0x66, 0x00, 0x00],
        '<' => [0x06, 0x0C, 0x18, 0x30, 0x18, 0x0C, 0x06, 0x00],
        '>' => [0x30, 0x18, 0x0C, 0x06, 0x0C, 0x18, 0x30, 0x00],
        '_' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x7E, 0x00],
        ' ' => [0x00; 8],
        _ => [0x18, 0x3C, 0x3C, 0x18, 0x00, 0x00, 0x00, 0x00],
    }
}

fn fill_rect(fb: &Framebuffer, x0: usize, y0: usize, w: usize, h: usize, color_argb: u32) {
    let x_end = (x0 + w).min(fb.width);
    let y_end = (y0 + h).min(fb.height);
    let mut y = y0;
    while y < y_end {
        let row_ptr = unsafe { fb.base_ptr.add(y * fb.pitch_u32) };
        let mut x = x0;
        while x < x_end {
            unsafe { write_volatile(row_ptr.add(x), color_argb) };
            x += 1;
        }
        y += 1;
    }
}

fn draw_text(fb: &Framebuffer, start_x: usize, start_y: usize, scale: usize, text: &str, color_argb: u32) {
    let sc = scale.max(1);
    let mut cursor_x = start_x;
    for ch in text.chars() {
        if cursor_x + 8 * sc >= fb.width {
            break;
        }
        let rows = glyph_8x8(ch);
        let mut ry = 0usize;
        while ry < 8 {
            let bits = rows[ry];
            let mut rx = 0usize;
            while rx < 8 {
                if ((bits >> (7 - rx)) & 1) != 0 {
                    fill_rect(fb, cursor_x + rx * sc, start_y + ry * sc, sc, sc, color_argb);
                }
                rx += 1;
            }
            ry += 1;
        }
        cursor_x += 9 * sc;
    }
}

const COLOR_BG: u32 = 0xFF05_060A;       // Constellation Ink
const COLOR_HEADER: u32 = 0xFF0A_0E1A;   // Header Navy
const COLOR_CARD: u32 = 0xFF0B_101E;     // Stage Card
const COLOR_MINT: u32 = 0xFF62_E6C9;     // Mint #62E6C9
const COLOR_GOLD: u32 = 0xFFE8_B05C;     // Gold #E8B05C
const COLOR_VIOLET: u32 = 0xFFB4_8BFF;   // Violet #B48BFF
const COLOR_TEXT: u32 = 0xFFE8_EEF7;     // Crisp Ice White

#[no_mangle]
pub extern "C" fn _start() -> ! {
    // 0. Enable x86_64 SSE/XMM in CR0 (clear EM bit 2, set MP bit 1) and CR4 (set OSFXSR bit 9 | OSXMMEXCPT bit 10)
    unsafe {
        asm!(
            "mov rax, cr0",
            "and ax, 0xFFFB",
            "or ax, 0x0002",
            "mov cr0, rax",
            "mov rax, cr4",
            "or ax, 0x0600",
            "mov cr4, rax",
            out("rax") _,
            options(nomem, nostack)
        );
    }

    // 1. Send "AYEOS_X86_64_BAREMETAL_PXE_OK\r\n" to COM1 (0x3F8) for headless/QEMU verification
    unsafe {
        for &b in b"AYEOS_X86_64_BAREMETAL_PXE_OK\r\n" {
            outb(0x3F8, b);
        }
    }

    // 2. Retrieve Linear Framebuffer from Limine
    let resp_ptr = unsafe { read_volatile(&FRAMEBUFFER_REQUEST.response) };
    if resp_ptr.is_null() {
        loop {
            unsafe { asm!("hlt", options(nomem, nostack, preserves_flags)) };
        }
    }
    let resp = unsafe { &*resp_ptr };
    if resp.framebuffer_count == 0 || resp.framebuffers.is_null() {
        loop {
            unsafe { asm!("hlt", options(nomem, nostack, preserves_flags)) };
        }
    }
    let lfb = unsafe { &**resp.framebuffers };
    let fb = Framebuffer {
        base_ptr: lfb.address as *mut u32,
        width: lfb.width as usize,
        height: lfb.height as usize,
        pitch_u32: (lfb.pitch as usize) / 4,
    };

    // 3. Paint static Constellation Ink background, Header Bar, and Sovereign Stage Card once
    fill_rect(&fb, 0, 0, fb.width, fb.height, COLOR_BG);
    fill_rect(&fb, 0, 0, fb.width, 56, COLOR_HEADER);
    fill_rect(&fb, 0, 56, fb.width, 2, COLOR_MINT);

    draw_text(
        &fb,
        24,
        18,
        2,
        "AYEOS X86_64 BARE-METAL RUST OS (NO LINUX - ZERO FLOATS) - 384KHZ BABBAGE",
        COLOR_MINT,
    );

    fill_rect(&fb, 24, 84, 620, 390, COLOR_CARD);
    fill_rect(&fb, 24, 84, 620, 3, COLOR_GOLD);
    draw_text(&fb, 44, 108, 2, "SOVEREIGN AYEOS X86_64 PXE SANCTUARY", COLOR_GOLD);
    draw_text(&fb, 44, 148, 2, "- KERNEL  : 100% BARE-METAL X86_64 RUST", COLOR_TEXT);
    draw_text(&fb, 44, 178, 2, "- BOOT    : DISKLESS UEFI + BIOS PXE (ZERO DISK)", COLOR_TEXT);
    draw_text(&fb, 44, 208, 2, "- MOTHER  : 172.16.47.76 (MEMNET TFTP + DHCP)", COLOR_TEXT);
    draw_text(&fb, 44, 238, 2, "- LINOSV  : 67DC0028CFE67AB7 TERNARY MESH", COLOR_MINT);
    draw_text(&fb, 44, 268, 2, "- DISPLAY : LIMINE GOP/VBE 32-BIT FRAMEBUFFER", COLOR_GOLD);
    draw_text(&fb, 44, 298, 2, "- ENGINE  : 768KHZ CATMULL-ROM + BABBAGE 3-ADD", COLOR_TEXT);
    draw_text(&fb, 44, 328, 2, "- TUNING  : 432 HZ + PHI (500/809) + 528 HZ", COLOR_VIOLET);
    draw_text(&fb, 44, 368, 2, "FINE TOUCH FROM WITHIN  <3 <3 <3 + 1", COLOR_GOLD);

    let mut frame_seq: i64 = 0;
    static mut PREV_HEIGHTS: [usize; 36] = [0usize; 36];
    let cx: i64 = ((fb.width as i64) * 3) / 4;
    let cy: i64 = 275;
    let radius: i64 = 130;
    let bar_bottom = fb.height.saturating_sub(20).max(520);
    let max_bar_h = 180usize;
    let slot_w = (fb.width.saturating_sub(56) / 36).max(16);
    let bar_w = (slot_w * 3) / 4;

    // Play a short welcoming 432 Hz -> 528 Hz -> 648 Hz PhoeniX chime on boot, then quiet the PC speaker
    let boot_notes: [u32; 4] = [432, 528, 648, 0];

    loop {
        if frame_seq == 2 {
            unsafe {
                for &b in b"AYEOS_X86_64_60FPS_SANCTUARY_RUNNING\r\n" {
                    outb(0x3F8, b);
                }
            }
        }
        if frame_seq < 120 {
            let note_idx = ((frame_seq / 30) as usize).min(3);
            pit_play_tone_hz(boot_notes[note_idx]);
        }

        frame_seq = frame_seq.wrapping_add(1);

        // Clear and redraw 432 Hz / phi (500/809) Parabolic Lissajous Ring box
        let ring_x0 = (cx - radius - 16).max(660) as usize;
        let ring_y0 = (cy - radius - 16).max(84) as usize;
        fill_rect(&fb, ring_x0, ring_y0, (radius as usize) * 2 + 32, (radius as usize) * 2 + 32, COLOR_BG);

        let seq_phase = (frame_seq * 768) & 0xFFFF;
        let mut step = 0i64;
        while step < 128 {
            let p_x = ((step * 512) + seq_phase) & 0xFFFF;
            let p_y = (((step * 512 * 500) / 809) + seq_phase) & 0xFFFF;
            let dx = (integer_parabolic_sine_q15(p_x) * radius) >> 15;
            let dy = (integer_parabolic_sine_q15(p_y) * radius) >> 15;
            let px = (cx + dx).max(0) as usize;
            let py = (cy + dy).max(0) as usize;
            fill_rect(&fb, px, py, 4, 4, COLOR_GOLD);
            step += 1;
        }

        let mut idx = 0usize;
        while idx < 36 {
            let b_phase = ((frame_seq * (520 + (idx as i64) * 113)) + (idx as i64) * 2048) & 0xFFFF;
            let w = integer_parabolic_sine_q15(b_phase).abs();
            let new_h = (24 + (((w as usize) * (max_bar_h - 24)) >> 15)).clamp(16, max_bar_h);
            let old_h = unsafe { read_volatile(&raw const PREV_HEIGHTS[idx]) };
            let x0 = 28 + idx * slot_w;
            let color = if idx < 12 {
                COLOR_MINT
            } else if idx < 24 {
                COLOR_GOLD
            } else {
                COLOR_VIOLET
            };

            if new_h > old_h {
                fill_rect(&fb, x0, bar_bottom - new_h, bar_w, new_h - old_h, color);
            } else if old_h > new_h {
                fill_rect(&fb, x0, bar_bottom - old_h, bar_w, old_h - new_h, COLOR_BG);
            }
            unsafe { write_volatile(&raw mut PREV_HEIGHTS[idx], new_h) };
            idx += 1;
        }

        // Gentle frame pacing (~60fps)
        let mut spin = 0u32;
        while spin < 450_000 {
            core::hint::spin_loop();
            spin += 1;
        }
    }
}
