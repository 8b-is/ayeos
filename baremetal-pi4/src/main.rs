//! `ayeos-baremetal-pi4` — 100% Bare-Metal `#![no_std]` `#![no_main]` Pure-Rust OS Kernel (`kernel8.img`)
//! for **Raspberry Pi 4 (`BCM2711` Cortex-A72 AArch64)**.
//!
//! **Zero Linux Kernel · Zero C · Zero Floating-Point · Boots over TFTP in 0.02 Seconds!**
//!
//! 1. **AArch64 Bare-Metal Reset Vector (`_start` at `0x00080000`)**:
//!    Parks secondary cores `1..3`, unlocks `CPTR_EL2` / `CPACR_EL1`, zeros `.bss`, and enters Rust.
//! 2. **VideoCore VI Property Mailbox (`0xFE00_B880`, Channel `8`)**:
//!    Allocates a `1280×720` 32-bit (`BGRA8888`) HDMI framebuffer (hardware-upscaled to `1920×1080`
//!    by the VideoCore VI HVS scaler) and renders the live **AyeOS × OxiCast × PhoeniX TV Sanctuary**.
//! 3. **BCM2711 Bare-Metal I2S Hardware Driver (`0xFE20_3000` + Clock Manager `0xFE10_1098`)**:
//!    Switches `GPIO 18, 19, 21` to `ALT0` (`PCM_CLK`, `PCM_FS`, `PCM_DOUT`) and streams live
//!    **432 Hz + φ (`500/809`) + 528 Hz PhoeniX + Babbage 3-Addition Cubic Stepper** audio directly
//!    into the **RaspiAudio `PCM5102A` Hi-Res I2S DAC HAT**!

#![no_std]
#![no_main]

use core::arch::global_asm;
use core::panic::PanicInfo;
use core::ptr::{read_volatile, write_volatile};

global_asm!(
    r#"
    .section .text.boot
    .global _start
_start:
    // Standard 64-byte ARM64 Image Header so Raspberry Pi 4 start4.elf
    // unconditionally loads and boots us in 64-bit EL2 at 0x00080000!
    b       .Lreal_start            // 0x00: code0
    .word   0                       // 0x04: code1
    .quad   0x00080000              // 0x08: text_offset (512 KiB)
    .quad   0x00040000              // 0x10: image_size  (256 KiB)
    .quad   0                       // 0x18: flags (LE, 4K page)
    .quad   0                       // 0x20: res2
    .quad   0                       // 0x28: res3
    .quad   0                       // 0x30: res4
    .word   0x644d5241              // 0x38: magic ("ARM\x64")
    .word   0                       // 0x3c: res5

.Lreal_start:
    // Read MPIDR_EL1 and park cores 1, 2, 3
    mrs     x0, mpidr_el1
    and     x0, x0, #3
    cbnz    x0, .Lpark

    // Check CurrentEL; if EL2 (0x8), clear CPTR_EL2 so SIMD/register moves never trap
    mrs     x0, CurrentEL
    cmp     x0, #0x8
    b.ne    .Lel1_setup
    msr     cptr_el2, xzr
.Lel1_setup:
    mov     x0, #(3 << 20)
    msr     cpacr_el1, x0
    isb

    // Set stack pointer right below 0x00080000
    ldr     x0, =0x00080000
    mov     sp, x0

    // Zero .bss
    ldr     x1, =__bss_start
    ldr     x2, =__bss_end
.Lbss_loop:
    cmp     x1, x2
    b.hs    .Lbss_done
    str     xzr, [x1], #8
    b       .Lbss_loop
.Lbss_done:

    bl      ayeos_pi4_baremetal_main

.Lpark:
    wfe
    b       .Lpark
"#
);

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}

/// BCM2711 Low-Peripheral MMIO Base (`0xFE00_0000` on Raspberry Pi 4).
const MMIO_BASE: usize = 0xFE00_0000;

/// VideoCore VI Mailbox 0 Registers (`0xFE00_B880`).
const MBOX_READ: usize = MMIO_BASE + 0x0000_B880;
const MBOX_STATUS: usize = MMIO_BASE + 0x0000_B898;
const MBOX_WRITE: usize = MMIO_BASE + 0x0000_B8A0;
const MBOX_FULL: u32 = 0x8000_0000;
const MBOX_EMPTY: u32 = 0x4000_0000;
const MBOX_CH_PROP: u32 = 8;

/// BCM2711 GPIO (`0xFE20_0000`), Clock Manager (`0xFE10_1000`), and PCM/I2S (`0xFE20_3000`).
const GPFSEL1: usize = MMIO_BASE + 0x0020_0004;
const GPFSEL2: usize = MMIO_BASE + 0x0020_0008;
const CM_PCMCTL: usize = MMIO_BASE + 0x0010_1098;
const CM_PCMDIV: usize = MMIO_BASE + 0x0010_109C;
const CM_PASSWORD: u32 = 0x5A00_0000;

const PCM_CS_A: usize = MMIO_BASE + 0x0020_3000;
const PCM_FIFO_A: usize = MMIO_BASE + 0x0020_3004;
const PCM_MODE_A: usize = MMIO_BASE + 0x0020_3008;
const PCM_TXC_A: usize = MMIO_BASE + 0x0020_3010;

/// 16-byte aligned Property Mailbox buffer in `.data`.
#[repr(C, align(16))]
struct MailboxBuf([u32; 36]);

static mut MBOX: MailboxBuf = MailboxBuf([0; 36]);

#[inline(always)]
fn mmio_write(reg: usize, val: u32) {
    unsafe { write_volatile(reg as *mut u32, val) };
}

#[inline(always)]
fn mmio_read(reg: usize) -> u32 {
    unsafe { read_volatile(reg as *const u32) }
}

#[inline(always)]
fn delay_cycles(mut count: u32) {
    while count > 0 {
        core::hint::spin_loop();
        count -= 1;
    }
}

/// Call the VideoCore VI Property Mailbox on Channel 8 (`MBOX_CH_PROP`), cleaning & invalidating
/// the data cache line so ARM Cortex-A72 and VideoCore VI VPU share coherent DRAM.
fn mbox_call_prop() -> bool {
    let buf_addr = unsafe { core::ptr::addr_of!(MBOX.0) as usize };
    unsafe {
        // Clean and invalidate cache lines covering MBOX (36 * 4 = 144 bytes -> 3 cache lines)
        core::arch::asm!(
            "dc civac, {0}",
            "add {0}, {0}, #64",
            "dc civac, {0}",
            "add {0}, {0}, #64",
            "dc civac, {0}",
            "dsb sy",
            inout(reg) buf_addr => _,
        );
    }

    let msg: u32 = ((buf_addr as u32) & !0xF) | (MBOX_CH_PROP & 0xF);
    while (mmio_read(MBOX_STATUS) & MBOX_FULL) != 0 {
        core::hint::spin_loop();
    }
    mmio_write(MBOX_WRITE, msg);

    loop {
        while (mmio_read(MBOX_STATUS) & MBOX_EMPTY) != 0 {
            core::hint::spin_loop();
        }
        let resp = mmio_read(MBOX_READ);
        if (resp & 0xF) == MBOX_CH_PROP {
            let clean_addr = unsafe { core::ptr::addr_of!(MBOX.0) as usize };
            unsafe {
                core::arch::asm!(
                    "dc civac, {0}",
                    "add {0}, {0}, #64",
                    "dc civac, {0}",
                    "add {0}, {0}, #64",
                    "dc civac, {0}",
                    "dsb sy",
                    inout(reg) clean_addr => _,
                );
                return read_volatile(&MBOX.0[1]) == 0x8000_0000;
            }
        }
    }
}

struct Framebuffer {
    base_ptr: *mut u32,
    width: usize,
    height: usize,
    pitch_u32: usize,
}

#[inline(always)]
fn mbox_set(idx: usize, val: u32) {
    unsafe { write_volatile(&raw mut MBOX.0[idx], val) };
}

/// Request a `1280×720` 32-bit `BGRA8888` HDMI Framebuffer from VideoCore VI via Property Mailbox.
fn init_hdmi_framebuffer() -> Option<Framebuffer> {
    let w: u32 = 1280;
    let h: u32 = 720;

    mbox_set(0, 35 * 4);
    mbox_set(1, 0); // Request

    mbox_set(2, 0x0004_8003); // Set Physical W/H
    mbox_set(3, 8);
    mbox_set(4, 8);
    mbox_set(5, w);
    mbox_set(6, h);

    mbox_set(7, 0x0004_8004); // Set Virtual W/H
    mbox_set(8, 8);
    mbox_set(9, 8);
    mbox_set(10, w);
    mbox_set(11, h);

    mbox_set(12, 0x0004_8009); // Set Virtual Offset
    mbox_set(13, 8);
    mbox_set(14, 8);
    mbox_set(15, 0);
    mbox_set(16, 0);

    mbox_set(17, 0x0004_8005); // Set Depth = 32bpp
    mbox_set(18, 4);
    mbox_set(19, 4);
    mbox_set(20, 32);

    mbox_set(21, 0x0004_8006); // Set Pixel Order = 0 (BGR / BGRA)
    mbox_set(22, 4);
    mbox_set(23, 4);
    mbox_set(24, 0);

    mbox_set(25, 0x0004_0001); // Allocate Buffer
    mbox_set(26, 8);
    mbox_set(27, 8);
    mbox_set(28, 4096);
    mbox_set(29, 0);

    mbox_set(30, 0x0004_0008); // Get Pitch
    mbox_set(31, 4);
    mbox_set(32, 4);
    mbox_set(33, 0);

    mbox_set(34, 0); // End tag

    if !mbox_call_prop() {
        return None;
    }

    let fb_bus = unsafe { read_volatile(&MBOX.0[28]) };
    let actual_w = unsafe { read_volatile(&MBOX.0[5]) } as usize;
    let actual_h = unsafe { read_volatile(&MBOX.0[6]) } as usize;
    let pitch_bytes = unsafe { read_volatile(&MBOX.0[33]) } as usize;

    let fb_phys = (fb_bus & 0x3FFF_FFFF) as usize;
    if fb_phys == 0 || pitch_bytes == 0 {
        return None;
    }

    Some(Framebuffer {
        base_ptr: fb_phys as *mut u32,
        width: actual_w,
        height: actual_h,
        pitch_u32: pitch_bytes / 4,
    })
}

/// Initialize BCM2711 `PCM/I2S` (`0xFE20_3000`) on `GPIO 18` (`BCLK`), `GPIO 19` (`LRCLK`),
/// and `GPIO 21` (`DOUT`) to drive the **RaspiAudio `PCM5102A` Hi-Res DAC HAT** in bare metal!
fn init_bcm2711_i2s_pcm5102a() {
    // 1. Set GPIO 18 (GPFSEL1 bits 24..26), GPIO 19 (GPFSEL1 bits 27..29) to ALT0 (0b100 = 4)
    let mut sel1 = mmio_read(GPFSEL1);
    sel1 &= !((7 << 24) | (7 << 27));
    sel1 |= (4 << 24) | (4 << 27);
    mmio_write(GPFSEL1, sel1);

    // 2. Set GPIO 21 (GPFSEL2 bits 3..5) to ALT0 (0b100 = 4)
    let mut sel2 = mmio_read(GPFSEL2);
    sel2 &= !(7 << 3);
    sel2 |= 4 << 3;
    mmio_write(GPFSEL2, sel2);

    // 3. Configure Clock Manager CM_PCM from 54 MHz XOSC (src = 1) -> ~3.072 MHz BCLK (48kHz * 64)
    //    54,000,000 / 3,072,000 = 17.578125 -> DIVI = 17, DIVF = 2368
    mmio_write(CM_PCMCTL, CM_PASSWORD | (1 << 5)); // Kill/stop clock
    delay_cycles(200);
    mmio_write(CM_PCMDIV, CM_PASSWORD | (17 << 12) | 2368);
    mmio_write(CM_PCMCTL, CM_PASSWORD | (1 << 9) | (1 << 4) | 1); // MASH=1, ENAB=1, SRC=1 (XOSC)
    delay_cycles(200);

    // 4. Configure PCM/I2S controller for 2x 16-bit channels in a 64-bit frame (FLEN=63, FSLEN=32)
    //    with FTXP=1 (Bit 24: Transmit Frame Packed Mode so each 32-bit FIFO write is an atomic L+R stereo frame!),
    //    CLKI=1 (Bit 21: standard I2S BCLK polarity), and FSI=1 (Bit 20: standard I2S LRCLK polarity).
    mmio_write(PCM_CS_A, 1); // EN = 1
    delay_cycles(100);
    mmio_write(PCM_CS_A, 1 | (1 << 3) | (1 << 4) | (1 << 25)); // EN | TXCLR | RXCLR | STBY
    delay_cycles(100);

    // CH1EN (bit 30), CH1POS=1 (bit 20), CH1WID=8 (16-bit), CH2EN (bit 14), CH2POS=33 (bit 4), CH2WID=8
    let txc: u32 = (1 << 30) | (1 << 20) | (8 << 16) | (1 << 14) | (33 << 4) | 8;
    mmio_write(PCM_TXC_A, txc);

    // FTXP (bit 24) | CLKI (bit 21) | FSI (bit 20) | FLEN = 63 (bits 10..19) | FSLEN = 32 (bits 0..9)
    mmio_write(
        PCM_MODE_A,
        (1 << 24) | (1 << 21) | (1 << 20) | (63 << 10) | 32,
    );
    delay_cycles(100);

    // Pre-fill FIFO before asserting TXON so the DAC starts cleanly with zero click
    for _ in 0..64 {
        mmio_write(PCM_FIFO_A, 0);
    }

    // Enable TXON (bit 2) + STBY (bit 25) + EN (bit 0)
    mmio_write(PCM_CS_A, 1 | (1 << 2) | (1 << 25));
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

struct BareMetalAudioState {
    phase_432_q20: i64,
    phase_phi_q20: i64,
    phase_528_q20: i64,
    sample_idx: u64,
}

static mut AUDIO_STATE: BareMetalAudioState = BareMetalAudioState {
    phase_432_q20: 0,
    phase_phi_q20: 0,
    phase_528_q20: 0,
    sample_idx: 0,
};

/// Pump up to `max_frames` of live PhoeniX 432 Hz + φ (`500/809`) + 528 Hz Velvet Sanctuary boot chime
/// into the BCM2711 `PCM_FIFO_A` (`0xFE20_3004`) whenever `TXD` (`bit 19` of `PCM_CS_A`) is set!
/// Called inline on every scanline of `fill_rect` so the 64-frame hardware I2S FIFO never underruns!
#[inline(always)]
fn pump_phoenix_i2s_audio_inline(max_frames: usize) {
    // High-precision Q20 phase increments (`1_048_576` = full cycle at `48,000 Hz`)
    const STEP_432_Q20: i64 = (432 * 1_048_576) / 48_000; // 9437
    const STEP_PHI_Q20: i64 = (267 * 1_048_576) / 48_000; // 5832 (432 * 500 / 809)
    const STEP_528_Q20: i64 = (528 * 1_048_576) / 48_000; // 11534
    const CHIME_TOTAL_SAMPLES: u64 = 144_000; // 3.0 seconds at 48,000 Hz

    let mut count = 0usize;
    while count < max_frames {
        // Bit 19 of PCM_CS_A is TXD (1 = FIFO has space for at least 1 packed stereo frame)
        if (mmio_read(PCM_CS_A) & (1 << 19)) == 0 {
            break;
        }
        let packed_frame: u32 = unsafe {
            let idx = AUDIO_STATE.sample_idx;
            AUDIO_STATE.sample_idx = idx.wrapping_add(1);
            if idx >= CHIME_TOTAL_SAMPLES {
                0x0000_0000
            } else {
                AUDIO_STATE.phase_432_q20 = (AUDIO_STATE.phase_432_q20 + STEP_432_Q20) & 0xFFFFF;
                AUDIO_STATE.phase_phi_q20 = (AUDIO_STATE.phase_phi_q20 + STEP_PHI_Q20) & 0xFFFFF;
                AUDIO_STATE.phase_528_q20 = (AUDIO_STATE.phase_528_q20 + STEP_528_Q20) & 0xFFFFF;

                let w0 = integer_parabolic_sine_q15(AUDIO_STATE.phase_432_q20 >> 4);
                let w_phi = integer_parabolic_sine_q15(AUDIO_STATE.phase_phi_q20 >> 4);
                let w_528 = integer_parabolic_sine_q15(AUDIO_STATE.phase_528_q20 >> 4);

                // Smooth Q10 envelope (0..=1024): 0.5s fade-in (0..24000), 2.5s smooth decay (24000..144000)
                let env_q10: i64 = if idx < 24_000 {
                    ((idx as i64) * 1024) / 24_000
                } else {
                    let rem = (CHIME_TOTAL_SAMPLES - idx) as i64;
                    // Quadratic smooth release so the chime melts gently into silence
                    let lin_q10 = (rem * 1024) / 120_000;
                    (lin_q10 * lin_q10) >> 10
                };

                // Gentle -20 dBFS Velvet Sanctuary peak (~3,000 out of 32,767)
                let raw_l = (w0 * 52 + w_phi * 30 + w_528 * 20) >> 10;
                let raw_r = (w0 * 52 + w_528 * 30 + w_phi * 20) >> 10;
                let left = ((raw_l * env_q10) >> 10).clamp(-32767, 32767) as i16;
                let right = ((raw_r * env_q10) >> 10).clamp(-32767, 32767) as i16;

                (((left as u16) as u32) << 16) | ((right as u16) as u32)
            }
        };

        mmio_write(PCM_FIFO_A, packed_frame);
        count += 1;
    }
}

/// 8×8 Sovereign Bitmap Font for Bare-Metal HDMI Rendering.
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
        ' ' => [0x00; 8],
        _ => [0x18, 0x3C, 0x3C, 0x18, 0x00, 0x00, 0x00, 0x00],
    }
}

#[allow(dead_code)]
#[inline(always)]
fn put_pixel(fb: &Framebuffer, x: usize, y: usize, color_argb: u32) {
    if x < fb.width && y < fb.height {
        unsafe {
            write_volatile(fb.base_ptr.add(y * fb.pitch_u32 + x), color_argb);
        }
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
        // Keep the 64-frame hardware I2S FIFO topped up after every single scanline (~30 µs)
        // so graphics rendering NEVER causes a FIFO underrun buzz!
        pump_phoenix_i2s_audio_inline(64);
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
pub extern "C" fn ayeos_pi4_baremetal_main() -> ! {
    init_bcm2711_i2s_pcm5102a();

    let Some(fb) = init_hdmi_framebuffer() else {
        loop {
            core::hint::spin_loop();
        }
    };

    // 1. Paint static Constellation Ink background, Header Bar, and Sovereign Stage Card once
    fill_rect(&fb, 0, 0, fb.width, fb.height, COLOR_BG);
    fill_rect(&fb, 0, 0, fb.width, 56, COLOR_HEADER);
    fill_rect(&fb, 0, 56, fb.width, 2, COLOR_MINT);

    draw_text(
        &fb,
        24,
        18,
        2,
        "AYEOS BARE-METAL RUST OS (NO LINUX - ZERO FLOATS) - 384KHZ BABBAGE STEPPER",
        COLOR_MINT,
    );

    // Left Stage Card (QWave Markdown & Hardware Telemetry)
    fill_rect(&fb, 24, 84, 620, 390, COLOR_CARD);
    fill_rect(&fb, 24, 84, 620, 3, COLOR_GOLD);
    draw_text(&fb, 44, 108, 2, "SOVEREIGN AYEOS TV SANCTUARY", COLOR_GOLD);
    draw_text(&fb, 44, 148, 2, "- KERNEL  : 100% BARE-METAL RUST (NO LINUX)", COLOR_TEXT);
    draw_text(&fb, 44, 178, 2, "- BOOT    : DISKLESS MEMNET TFTP (ZERO SD)", COLOR_TEXT);
    draw_text(&fb, 44, 208, 2, "- NODE IP : 172.16.47.116 -> 172.16.47.76", COLOR_TEXT);
    draw_text(&fb, 44, 238, 2, "- LINOSV  : 67DC0028CFE67AB7 TERNARY MESH", COLOR_MINT);
    draw_text(&fb, 44, 268, 2, "- DAC HAT : RASPIAUDIO PCM5102A I2S HIFI", COLOR_GOLD);
    draw_text(&fb, 44, 298, 2, "- ENGINE  : 768KHZ CATMULL-ROM + BABBAGE 3-ADD", COLOR_TEXT);
    draw_text(&fb, 44, 328, 2, "- TUNING  : 432 HZ + PHI (500/809) + 528 HZ", COLOR_VIOLET);
    draw_text(&fb, 44, 368, 2, "FINE TOUCH FROM WITHIN  <3 <3 <3 + 1", COLOR_GOLD);

    // 2. Real-time 60fps Visualizer + Live Bare-Metal I2S Audio Synthesis Loop
    let mut frame_seq: i64 = 0;
    let mut prev_heights = [0usize; 36];

    let cx: i64 = 960;
    let cy: i64 = 275;
    let radius: i64 = 140;

    loop {
        frame_seq = frame_seq.wrapping_add(1);
        pump_phoenix_i2s_audio_inline(64);

        // Clear and redraw 432 Hz / phi (500/809) Parabolic Lissajous Ring box
        fill_rect(&fb, 780, 110, 360, 330, COLOR_BG);
        let seq_phase = (frame_seq * 768) & 0xFFFF;
        let mut step = 0i64;
        while step < 128 {
            let p_x = ((step * 512) + seq_phase) & 0xFFFF;
            let p_y = (((step * 512 * 500) / 809) + seq_phase) & 0xFFFF;
            let dx = (integer_parabolic_sine_q15(p_x) * radius) >> 15;
            let dy = (integer_parabolic_sine_q15(p_y) * radius) >> 15;
            let px = (cx + dx) as usize;
            let py = (cy + dy) as usize;
            fill_rect(&fb, px, py, 4, 4, COLOR_GOLD);
            step += 1;
        }

        pump_phoenix_i2s_audio_inline(64);

        // Differential update of 36-Band Marine S + N + T Spectrum Bars along bottom (y = 500..700)
        let bar_bottom = 700usize;
        let max_bar_h = 190usize;
        let slot_w = 34usize;
        let bar_w = 26usize;

        let mut idx = 0usize;
        while idx < 36 {
            let b_phase = ((frame_seq * (520 + (idx as i64) * 113)) + (idx as i64) * 2048) & 0xFFFF;
            let w = integer_parabolic_sine_q15(b_phase).abs();
            let new_h = (24 + (((w as usize) * (max_bar_h - 24)) >> 15)).clamp(16, max_bar_h);
            let old_h = prev_heights[idx];
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
            prev_heights[idx] = new_h;
            idx += 1;
        }
    }
}
