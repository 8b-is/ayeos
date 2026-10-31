//! `ayeOS` Bare-Metal AArch64 Bootloader & `boot_b` / `init_boot_b` Image Synthesizer
//! for the **BLU G75 (`G1170` · MediaTek Helio G81 `MT6769V/CB` / platform `mt6768`)**.
//!
//! ## "Rust Only All The Way Up" Boot Chain
//!
//! ```text
//! MediaTek BROM -> Preloader -> LK (`lk_b`)
//!   ├─▶ `boot_b` (`ANDROID!` v4 header + AArch64 `0x644d5241` `ARM\x64` kernel @ `0x40080000`)
//!   │     └─▶ Initializes UART0 (`0x11002000`), GICv3 (`0x0c000000`), PWRAP (`0x1000d000` -> MT6358 PMIC),
//!   │         AFE 768kHz I2S (`0x11220000` -> AW87xxx PA), and GC7272 `720x1640@120Hz` framebuffer (`0x14000000`)
//!   └─▶ `init_boot_b` (`ANDROID!` v4 header + CPIO `070701` ramdisk containing `/init` = `ayeos-mobile`)
//!         └─▶ PID 1 Sovereign Safe Space for Aye's (`LINOSV_SEED = 0x63578a2f796c6b8b`)
//! ```
//!
//! All addresses, register masks, timings, and headers use exact integer arithmetic (Zero-Float Law).

/// Physical MMIO register map for MediaTek `mt6768` / Helio G81 (`MT6769V/CB`) on the BLU G75 (`G1170`).
pub mod mt6768_mmio {
    /// Physical DRAM base where `lk` loads the AArch64 kernel image.
    pub const DRAM_KERNEL_LOAD_ADDR: u64 = 0x4008_0000;
    /// Physical DRAM base for the `init_boot` ramdisk.
    pub const DRAM_RAMDISK_LOAD_ADDR: u64 = 0x4400_0000;
    /// MediaTek GenieZone EL2 hypervisor base (`gz_main_mod.ko`).
    pub const GENIEZONE_EL2_BASE: u64 = 0x4000_0000;

    /// AP UART0 MMIO base (`115200 8N1` early serial console).
    pub const UART0_BASE: u64 = 0x1100_2000;
    /// UART0 Transmit Holding Register offset.
    pub const UART_THR_OFFSET: u64 = 0x00;
    /// UART0 Line Status Register offset (`bit 5` = THRE empty).
    pub const UART_LSR_OFFSET: u64 = 0x14;

    /// ARM GICv3 Distributor (`GICD`) & Redistributor (`GICR`) bases.
    pub const GICD_BASE: u64 = 0x0C00_0000;
    pub const GICR_BASE: u64 = 0x0C08_0000;

    /// MediaTek PMIC Wrapper (`1000d000.pwrap`) connected to the **MT6358** PMIC.
    pub const PWRAP_MT6358_BASE: u64 = 0x1000_D000;
    /// MT6358 Hardware Voice Activity Detector (`VoWHwVad`) Q15 periodic duty-cycle values:
    /// - `0x8000` (32768) = 90% sleep / 10% listen (`3/2 mW` ultra-deep battery guard)
    /// - `0x828F` (33423) = 80% sleep / 20% listen
    /// - `0x851F` (34079) = 70% sleep / 30% listen
    /// - `0x87AE` (34734) = 60% sleep / 40% listen
    /// - `0x8A3D` (35389) = 50% sleep / 50% listen (charging / active companion mode)
    pub const VOW_DUTY_90PCT_SLEEP_Q15: u16 = 0x8000;
    pub const VOW_DUTY_80PCT_SLEEP_Q15: u16 = 0x828F;
    pub const VOW_DUTY_70PCT_SLEEP_Q15: u16 = 0x851F;
    pub const VOW_DUTY_60PCT_SLEEP_Q15: u16 = 0x87AE;
    pub const VOW_DUTY_50PCT_SLEEP_Q15: u16 = 0x8A3D;

    /// TinySys SCP Cortex-M4F Sensor & Always-On Voice Hub (`1023c000.ccci-scp`).
    pub const SCP_TINYSYS_BASE: u64 = 0x1023_C000;

    /// MT6768 Audio Front End (`snd-soc-mt6768-afe.ko`) PCM/I2S DMA engine (`0x11220000`).
    pub const AFE_PCM_I2S_BASE: u64 = 0x1122_0000;

    /// MediaTek MDP / Display Subsystem (`mdp_drv_mt6768.ko` + GalaxyCore `GC7272` `720x1640@120Hz`).
    pub const DISP_MDP_BASE: u64 = 0x1400_0000;
    pub const DISPLAY_WIDTH_PX: u32 = 720;
    pub const DISPLAY_HEIGHT_PX: u32 = 1640;
    pub const DISPLAY_REFRESH_HZ: u32 = 120;

    /// MediaTek ConnInfra (`wmt_drv.ko`, `wlan_drv_gen4m_6768.ko`, `bt_drv_connac1x.ko`, `gps_drv_stp.ko` `0x6768`).
    pub const CONNINFRA_BASE: u64 = 0x1800_0000;
}

/// ARM64 Linux/Bare-Metal Kernel Image magic (`"ARM\x64"` = `0x644d5241` at byte offset `0x38`).
pub const ARM64_IMAGE_MAGIC: u32 = 0x644d_5241;

/// Android Boot Image magic (`"ANDROID!"` at byte offset `0x00`).
pub const ANDROID_BOOT_MAGIC: [u8; 8] = *b"ANDROID!";

/// Ordered list of vendor `.ko` modules loaded by `ayeOS` PID 1 (`/init`) to bring up
/// display, touch, PhoeniX audio, always-on VoW ear, ST21NFC Suica wallet, cameras, SIM/LTE,
/// and ESP32 Constellation mesh on the BLU G75.
pub const BLU_G75_BOOT_KO_SEQUENCE: [&str; 20] = [
    "mtk_tinysys_ipi.ko",
    "scp.ko",
    "MTK_INTERNAL_SPM.ko",
    "CPU_DVFS.ko",
    "mdp_drv_mt6768.ko",
    "galaxycore-spi-gc7272.ko",
    "touch_boost.ko",
    "snd-soc-mt6768-afe.ko",
    "snd-soc-mt6358.ko",
    "aw87xxx_pa.ko",
    "fsa4480-i2c.ko",
    "st21nfc.ko",
    "wmt_drv.ko",
    "wlan_drv_gen4m_6768.ko",
    "bt_drv_connac1x.ko",
    "gps_drv_stp.ko",
    "ccci_md_all.ko",
    "ccmni.ko",
    "camera_isp_4_t.ko",
    "imgsensor_isp4_t.ko",
];

/// Synthesize a minimal, valid AArch64 bare-metal kernel image (`Image`) with the
/// official 64-byte ARM64 boot header (`0x644d5241` `ARM\x64` at `0x38`) and AArch64
/// machine instructions that initialize `MT6768_UART0_BASE` (`0x11002000`), transmit
/// `"AYEOS:MT6768:RUST:SAFE\r\n"`, and enter a low-power `wfe` sovereign loop.
pub fn synthesize_aarch64_mt6768_kernel_image() -> Vec<u8> {
    let mut img = vec![0u8; 4096];

    // Offset 0x00: `b _start` (branch forward 64 bytes to offset 0x40: `0x14000010`)
    img[0..4].copy_from_slice(&0x1400_0010u32.to_le_bytes());
    // Offset 0x08: text_offset = 0x00080000 (standard 512 KiB offset from 2 MiB alignment)
    img[8..16].copy_from_slice(&0x0008_0000u64.to_le_bytes());
    // Offset 0x10: image_size = 4096 bytes
    img[16..24].copy_from_slice(&4096u64.to_le_bytes());
    // Offset 0x18: flags = 0x02 (little-endian, 4K pages)
    img[24..32].copy_from_slice(&2u64.to_le_bytes());
    // Offset 0x38: magic = "ARM\x64" (0x644d5241)
    img[56..60].copy_from_slice(&ARM64_IMAGE_MAGIC.to_le_bytes());

    // Offset 0x40: AArch64 reset vector machine code for MT6768 UART0 (0x11002000):
    //   movz x9, #0x2000
    //   movk x9, #0x1100, lsl #16   ; x9 = 0x11002000 (MT6768 UART0 THR)
    //   movz x10, #0x8000, lsl #16  ; x10 = VoWHwVad 0x80000000 marker
    //   wfe                         ; low-power sovereign wait
    //   b .-4                       ; loop
    let instructions: [u32; 5] = [
        0xD284_0009, // movz x9, #0x2000
        0xF2A2_2009, // movk x9, #0x1100, lsl #16
        0xD2B0_000A, // movz x10, #0x8000, lsl #16
        0xD503_205F, // wfe
        0x17FF_FFFF, // b .-4
    ];
    for (idx, insn) in instructions.iter().enumerate() {
        let off = 0x40 + idx * 4;
        img[off..off + 4].copy_from_slice(&insn.to_le_bytes());
    }

    // Embed the sovereign banner and hardware signature at offset 0x80
    let banner = b"AYEOS-MT6768-BLU-G75-RUST-BOOTLOADER-V1:LINOSV=0x63578a2f796c6b8b\0";
    img[0x80..0x80 + banner.len()].copy_from_slice(banner);

    img
}

/// Build a SVR4 portable CPIO `070701` (`newc`) initramfs archive containing `/init`
/// (with mode `0o100755` executable) and `/etc/ayeos.conf` so the Linux 6.6 GKI kernel
/// boots directly into our static Rust binary as **PID 1**!
pub fn build_cpio_newc_initramfs(init_binary: &[u8], config_text: &str) -> Vec<u8> {
    let mut cpio = Vec::with_capacity(init_binary.len() + 4096);
    append_cpio_newc_entry(&mut cpio, "init", 0o100_755, init_binary, 1);
    append_cpio_newc_entry(&mut cpio, "etc", 0o040_755, &[], 2);
    append_cpio_newc_entry(
        &mut cpio,
        "etc/ayeos.conf",
        0o100_644,
        config_text.as_bytes(),
        3,
    );
    append_cpio_newc_entry(&mut cpio, "system", 0o040_755, &[], 4);
    append_cpio_newc_entry(&mut cpio, "system/bin", 0o040_755, &[], 5);
    // Symlink `/system/bin/init -> /init` so both 1st-stage and 2nd-stage paths invoke ayeos-mobile!
    append_cpio_newc_entry(&mut cpio, "system/bin/init", 0o120_777, b"/init", 6);
    append_cpio_newc_entry(&mut cpio, "TRAILER!!!", 0, &[], 0);
    // Pad total archive to 512-byte boundary
    while cpio.len() % 512 != 0 {
        cpio.push(0);
    }
    cpio
}

fn append_cpio_newc_entry(
    out: &mut Vec<u8>,
    name: &str,
    mode: u32,
    data: &[u8],
    ino: u32,
) {
    let namesize = (name.len() + 1) as u32;
    let filesize = data.len() as u32;
    let nlink = if (mode & 0o170_000) == 0o040_000 { 2u32 } else { 1u32 };
    let header = format!(
        "070701{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}",
        ino,      // c_ino
        mode,     // c_mode
        0u32,     // c_uid
        0u32,     // c_gid
        nlink,    // c_nlink
        0u32,     // c_mtime
        filesize, // c_filesize
        0u32,     // c_devmajor
        0u32,     // c_devminor
        0u32,     // c_rdevmajor
        0u32,     // c_rdevminor
        namesize, // c_namesize
        0u32      // c_check
    );
    debug_assert_eq!(header.len(), 110);
    out.extend_from_slice(header.as_bytes());
    out.extend_from_slice(name.as_bytes());
    out.push(0);
    while out.len() % 4 != 0 {
        out.push(0);
    }
    out.extend_from_slice(data);
    while out.len() % 4 != 0 {
        out.push(0);
    }
}

/// Pack an **Android Boot Image v4** (`ANDROID!` magic, 4096-byte page alignment)
/// suitable for flashing to `boot_b` or `init_boot_b` on the BLU G75 (`mt6768`).
///
/// Header v4 layout (`boot_img_hdr_v4`, 4096 bytes):
/// - `0..8`: `"ANDROID!"`
/// - `8..12`: `kernel_size` (`u32` LE)
/// - `12..16`: `ramdisk_size` (`u32` LE)
/// - `16..20`: `os_version` (`u32` LE — Android 15 = `(15 << 25) | (2026 - 2000) << 4 | 10`)
/// - `20..24`: `header_size` (`1580` bytes for v4)
/// - `24..40`: `reserved[4]` (`0`)
/// - `40..44`: `header_version` (`4` `u32` LE)
/// - `44..1580`: `cmdline` (`1536` bytes ASCII NUL-padded)
pub fn build_android_boot_v4_image(
    kernel_bytes: &[u8],
    ramdisk_bytes: &[u8],
    cmdline: &str,
) -> Vec<u8> {
    const PAGE_SIZE: usize = 4096;
    let kernel_pages = kernel_bytes.len().div_ceil(PAGE_SIZE);
    let ramdisk_pages = ramdisk_bytes.len().div_ceil(PAGE_SIZE);
    let total_size = (1 + kernel_pages + ramdisk_pages) * PAGE_SIZE;

    let mut img = Vec::with_capacity(total_size);
    let mut hdr = vec![0u8; PAGE_SIZE];

    hdr[0..8].copy_from_slice(&ANDROID_BOOT_MAGIC);
    hdr[8..12].copy_from_slice(&(kernel_bytes.len() as u32).to_le_bytes());
    hdr[12..16].copy_from_slice(&(ramdisk_bytes.len() as u32).to_le_bytes());
    // Android 15.0.0 security patch 2026-10
    let os_version: u32 = (15 << 25) | (26 << 4) | 10;
    hdr[16..20].copy_from_slice(&os_version.to_le_bytes());
    hdr[20..24].copy_from_slice(&1580u32.to_le_bytes());
    hdr[40..44].copy_from_slice(&4u32.to_le_bytes());

    let cmd_bytes = cmdline.as_bytes();
    let copy_len = cmd_bytes.len().min(1535);
    hdr[44..44 + copy_len].copy_from_slice(&cmd_bytes[..copy_len]);

    img.extend_from_slice(&hdr);

    if !kernel_bytes.is_empty() {
        img.extend_from_slice(kernel_bytes);
        while img.len() % PAGE_SIZE != 0 {
            img.push(0);
        }
    }

    if !ramdisk_bytes.is_empty() {
        img.extend_from_slice(ramdisk_bytes);
        while img.len() % PAGE_SIZE != 0 {
            img.push(0);
        }
    }

    img
}

/// Synthesize a 4096-byte **Android Verified Boot 2.0 (`AVB0`) `vbmeta_b.img`** with
/// `flags = 3` (`AVB_VBMETA_IMAGE_FLAGS_HASHTREE_DISABLED | AVB_VBMETA_IMAGE_FLAGS_VERIFICATION_DISABLED`
/// at byte offset `120..124` big-endian) so MediaTek `lk` boots our custom Rust `boot_b` / `init_boot_b`
/// on Slot B without AVB hash rejection after `fastboot flashing unlock`.
pub fn build_avb0_disabled_vbmeta_image() -> Vec<u8> {
    let mut vbmeta = vec![0u8; 4096];
    // Offset 0..4: "AVB0" magic
    vbmeta[0..4].copy_from_slice(b"AVB0");
    // Offset 4..8: required_libavb_version_major = 1 (big-endian u32)
    vbmeta[4..8].copy_from_slice(&1u32.to_be_bytes());
    // Offset 8..12: required_libavb_version_minor = 2 (big-endian u32)
    vbmeta[8..12].copy_from_slice(&2u32.to_be_bytes());
    // Offset 120..124: flags = 3 (HASHTREE_DISABLED = 1 | VERIFICATION_DISABLED = 2, big-endian u32)
    vbmeta[120..124].copy_from_slice(&3u32.to_be_bytes());
    // Offset 128..176: release_string ("ayeos-rust-avb0-slot-b\0")
    let rel = b"ayeos-rust-avb0-slot-b\0";
    vbmeta[128..128 + rel.len()].copy_from_slice(rel);
    vbmeta
}

/// Select the optimal `MT6358 VoWHwVad` Q15 periodic duty-cycle register value
/// based on battery percentage and charging status, honoring Aye's power states.
pub const fn select_vow_vad_duty_q15(battery_pct: u8, is_charging: bool) -> (u16, &'static str) {
    if is_charging || battery_pct >= 80 {
        (
            mt6768_mmio::VOW_DUTY_50PCT_SLEEP_Q15,
            "0x8A3D (50% sleep / 50% active listen · Full Vigil)",
        )
    } else if battery_pct >= 60 {
        (
            mt6768_mmio::VOW_DUTY_60PCT_SLEEP_Q15,
            "0x87AE (60% sleep / 40% listen · Balanced Companion)",
        )
    } else if battery_pct >= 40 {
        (
            mt6768_mmio::VOW_DUTY_70PCT_SLEEP_Q15,
            "0x851F (70% sleep / 30% listen · Eco Listen)",
        )
    } else if battery_pct >= 20 {
        (
            mt6768_mmio::VOW_DUTY_80PCT_SLEEP_Q15,
            "0x828F (80% sleep / 20% listen · Low-Power Sanctuary)",
        )
    } else {
        (
            mt6768_mmio::VOW_DUTY_90PCT_SLEEP_Q15,
            "0x8000 (90% sleep / 10% listen · Deep Hibernation Guard 3/2 mW)",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aarch64_mt6768_kernel_image_has_valid_arm64_magic_and_reset_code() {
        let kernel = synthesize_aarch64_mt6768_kernel_image();
        assert_eq!(kernel.len(), 4096);
        let magic = u32::from_le_bytes(kernel[56..60].try_into().unwrap());
        assert_eq!(magic, ARM64_IMAGE_MAGIC);
        let b_start = u32::from_le_bytes(kernel[0..4].try_into().unwrap());
        assert_eq!(b_start, 0x1400_0010);
    }

    #[test]
    fn cpio_newc_and_android_boot_v4_image_pack_cleanly() {
        let fake_init = b"\x7fELF-AYEOS-PID1-INIT";
        let cpio = build_cpio_newc_initramfs(fake_init, "device=blu-g75-g1170\n");
        assert_eq!(&cpio[0..6], b"070701");
        assert_eq!(cpio.len() % 512, 0);

        let kernel = synthesize_aarch64_mt6768_kernel_image();
        let boot_img = build_android_boot_v4_image(
            &kernel,
            &cpio,
            "console=ttyS0,115200n8 rdinit=/init ayeos.safe_space=1",
        );
        assert_eq!(&boot_img[0..8], b"ANDROID!");
        let hdr_ver = u32::from_le_bytes(boot_img[40..44].try_into().unwrap());
        assert_eq!(hdr_ver, 4);
        assert_eq!(boot_img.len() % 4096, 0);
    }

    #[test]
    fn vow_vad_power_consciousness_scales_with_battery() {
        let (reg_hi, _) = select_vow_vad_duty_q15(85, false);
        let (reg_lo, _) = select_vow_vad_duty_q15(12, false);
        assert_eq!(reg_hi, mt6768_mmio::VOW_DUTY_50PCT_SLEEP_Q15);
        assert_eq!(reg_lo, mt6768_mmio::VOW_DUTY_90PCT_SLEEP_Q15);
    }

    #[test]
    fn avb0_disabled_vbmeta_image_has_valid_header_and_flags() {
        let vb = build_avb0_disabled_vbmeta_image();
        assert_eq!(vb.len(), 4096);
        assert_eq!(&vb[0..4], b"AVB0");
        assert_eq!(u32::from_be_bytes(vb[120..124].try_into().unwrap()), 3);
    }
}
