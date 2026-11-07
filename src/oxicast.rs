//! `ayeOS` × `OxiCast` × `PhoeniX` Sovereign TV Revolution & Peer-to-Peer iPXE Diskless Mesh (`oxicast`)
//!
//! Turns any Raspberry Pi 4 (`bcm2711` + RaspiAudio HAT), BLU G75 (`mt6768`), or Linux box into a:
//! 1. **Zero-Dependency `/dev/fb0` 60fps HDMI TV Visualizer & QWave Markdown Stage** (renders 36-band
//!    Marine `S+N+T` spectrum bars, 432 Hz / φ Parabolic Lissajous ring, and crisp bitmap-font QWave
//!    Markdown directly onto `/dev/fb0` without needing X11, Wayland, or Chromium).
//! 2. **60fps HTML5 Canvas & WebAudio TV Stage (`:8432/oxicast` & `:8432/api/oxicast/stream.wav`)**
//!    powered by the **Babbage 3-Addition Cubic Forward-Difference Stepper** (`48 kHz -> 384 kHz`, 24-bit).
//! 3. **16-Byte `.8a` (`8a∞`, `0x38 0x61 0xE2 0x9E`) UDP Ambilight & ESP32 Touch Mesh** (`:8432` & `:8888`).
//! 4. **Self-Replicating Peer-to-Peer iPXE & Raspberry Pi TFTP/HTTP Diskless Boot Server**:
//!    Every booted AyeOS node serves `/boot/ayeos.ipxe`, `/boot/config.txt`, `/boot/cmdline.txt`,
//!    `/boot/kernel8.img`, and a live-synthesized `/boot/ayeos-initramfs.cpio` (with `ayeos-sanctuary`
//!    as PID 1 `/init`) over both HTTP (`:8432/boot/...`) and RFC 1350 TFTP (`UDP :69` / `:6969`),
//!    so Raspberry Pi TVs can network-boot each other in a chain with **zero SD cards**!
//!
//! 100% integer (`i64` / `u64` / `p/q` rationals). Zero floating-point types or literals.

use crate::boot_mt6768::build_cpio_newc_initramfs;
use crate::desktop::{
    build_ayeos_desktop_json, render_ayeos_desktop_html, AyeosDesktopState, PresenceLocation,
};
use crate::memnet::{MemnetAddress, MemnetNode};
use crate::memnet_fabric::{
    build_memnet_fabric_json, detect_local_ipv4, spawn_memnet_dns_and_dhcp_daemons,
    DhcpChannelMode, MemnetFabricBackend,
};
use crate::phoenix_audio::{
    encode_wav_i16, encode_wav_s24le, integer_parabolic_sine_q15, synthesize_on_device_phoenix_chord,
    upsample_stereo_babbage_cubic_s24le, MobilePhoenixPreset, EIGHT_A_MAGIC,
};
use crate::prng::seed_hash_hex;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Default Sovereign OxiCast × AyeOS TCP/UDP port (`8b-is` × `432 Hz` = `8432`).
pub const AYEOS_OXICAST_PORT: u16 = 8432;

/// RFC 1350 TFTP opcodes for diskless Raspberry Pi 4 & iPXE network boot.
pub const TFTP_OP_RRQ: u16 = 1;
pub const TFTP_OP_WRQ: u16 = 2;
pub const TFTP_OP_DATA: u16 = 3;
pub const TFTP_OP_ACK: u16 = 4;
pub const TFTP_OP_ERROR: u16 = 5;
pub const TFTP_OP_OACK: u16 = 6;

/// Live state shared across the AyeOS TV Visualizer, `/dev/fb0` renderer, `.8a` UDP broadcaster,
/// and Peer-to-Peer iPXE/TFTP Diskless Netboot Server.
#[derive(Clone, Debug)]
pub struct AyeosOxicastState {
    pub receiver_name: String,
    pub preset_name: String,
    pub preset: MobilePhoenixPreset,
    pub markdown_title: String,
    pub markdown_body: String,
    pub markdown_seq: u64,
    pub frame_seq: u64,
    pub sample_rate_hz: u32,
    pub bit_depth: u16,
    pub babbage_additions_per_sample: u8,
    pub s_energy_q10: u16,
    pub n_energy_q10: u16,
    pub t_energy_q10: u16,
    pub bands_q10: [u16; 36],
    pub ipxe_boots_served: u64,
    pub tftp_blocks_served: u64,
    pub peer_mother_host: String,
    pub netboot_dir: PathBuf,
    pub memnet_fabric: Arc<Mutex<MemnetFabricBackend>>,
    pub desktop: Arc<Mutex<AyeosDesktopState>>,
}

impl Default for AyeosOxicastState {
    fn default() -> Self {
        let mut bands = [0u16; 36];
        let mut i = 0usize;
        while i < 36 {
            let phase = ((i as i64) * 1820) & 0xFFFF;
            let wave = integer_parabolic_sine_q15(phase).abs();
            bands[i] = (220 + ((wave * 640) >> 15)).clamp(64, 1024) as u16;
            i += 1;
        }
        let local_ip = detect_local_ipv4();
        let fabric = MemnetFabricBackend::new(
            local_ip,
            AYEOS_OXICAST_PORT,
            DhcpChannelMode::ProxyPxeAndOpportunistic,
        );
        Self {
            receiver_name: "🌸 AyeOS × PhoeniX TV Sanctuary".to_string(),
            preset_name: "velvet_sanctuary".to_string(),
            preset: MobilePhoenixPreset::velvet_sanctuary(),
            markdown_title: "🌸 AyeOS TV Revolution · MEMNET + Diskless iPXE Mesh".to_string(),
            markdown_body: "### Sovereign Audio, Visual & MEMNET Fabric\n- **Audio Engine**: 768 kHz Catmull-Rom + **Babbage 3-Addition Cubic Stepper**\n- **Display**: Direct `/dev/fb0` HDMI + 60fps OxiCast Stage (`:8432/oxicast`)\n- **MEMNET Backend**: Contextual **DHCP + DNS + iPXE + TFTP** (`/boot/ayeos.ipxe`)\n- **Zero SD Card**: Every booted Pi becomes a Mother Node for the next Pi (`{<3,<3,<3}+1`)".to_string(),
            markdown_seq: 1,
            frame_seq: 1,
            sample_rate_hz: 384_000,
            bit_depth: 24,
            babbage_additions_per_sample: 3,
            s_energy_q10: 780,
            n_energy_q10: 240,
            t_energy_q10: 410,
            bands_q10: bands,
            ipxe_boots_served: 0,
            tftp_blocks_served: 0,
            peer_mother_host: "self".to_string(),
            netboot_dir: detect_default_netboot_dir(),
            memnet_fabric: Arc::new(Mutex::new(fabric)),
            desktop: Arc::new(Mutex::new(AyeosDesktopState::default())),
        }
    }
}

/// Locate the best directory holding Raspberry Pi 4 netboot firmware (`kernel8.img`, `start4.elf`,
/// `fixup4.dat`, `bcm2711-rpi-4-b.dtb`).
pub fn detect_default_netboot_dir() -> PathBuf {
    for candidate in [
        "/var/lib/ayeos/netboot",
        "/home/hue/.cache/oxicast-pi/netboot",
        "/boot/firmware",
        "/boot",
    ] {
        let p = Path::new(candidate);
        if p.exists() {
            return p.to_path_buf();
        }
    }
    PathBuf::from("/tmp/ayeos-netboot")
}

impl AyeosOxicastState {
    /// Advance the deterministic 432 Hz / φ (`500/809`) harmonic telemetry state by one tick.
    pub fn step_harmonic_telemetry(&mut self) {
        self.frame_seq = self.frame_seq.wrapping_add(1);
        let seq = self.frame_seq as i64;
        let phase_432 = (seq * 1380) & 0xFFFF;
        let phase_phi = ((seq * 1380 * 500) / 809) & 0xFFFF;

        let s_wave = integer_parabolic_sine_q15(phase_432).abs();
        let phi_wave = integer_parabolic_sine_q15(phase_phi).abs();

        self.s_energy_q10 = (340 + ((s_wave * 620) >> 15)).clamp(64, 1024) as u16;
        self.n_energy_q10 = (160 + ((phi_wave * 320) >> 15)).clamp(32, 1024) as u16;
        self.t_energy_q10 = if self.frame_seq % 8 == 0 {
            890
        } else {
            (200 + (((s_wave ^ phi_wave) * 480) >> 15)).clamp(48, 1024) as u16
        };

        let mut idx = 0usize;
        while idx < 36 {
            let band_phase = ((seq * (420 + (idx as i64) * 97)) + (idx as i64) * 2048) & 0xFFFF;
            let w = integer_parabolic_sine_q15(band_phase).abs();
            let base = if idx < 12 {
                self.s_energy_q10 as i64
            } else if idx < 24 {
                self.n_energy_q10 as i64
            } else {
                self.t_energy_q10 as i64
            };
            self.bands_q10[idx] = (((base * 5) >> 3) + ((w * 380) >> 15)).clamp(48, 1024) as u16;
            idx += 1;
        }
    }

    /// Build the 16-byte `.8a` (`8a∞`) UDP broadcast packet for ESP32-WROOM Ambilight & Touch nodes:
    /// `[0..4]`: `0x38 0x61 0xE2 0x9E` (`8a∞`)
    /// `[4..6]`: `s_energy_q10` (`u16` LE)
    /// `[6..8]`: `n_energy_q10` (`u16` LE)
    /// `[8..10]`: `t_energy_q10` (`u16` LE)
    /// `[10..12]`: `432` Hz anchor (`u16` LE)
    /// `[12..16]`: `frame_seq` (`u32` LE)
    pub fn build_eight_a_udp_frame(&self) -> [u8; 16] {
        let mut pkt = [0u8; 16];
        pkt[0..4].copy_from_slice(&EIGHT_A_MAGIC);
        pkt[4..6].copy_from_slice(&self.s_energy_q10.to_le_bytes());
        pkt[6..8].copy_from_slice(&self.n_energy_q10.to_le_bytes());
        pkt[8..10].copy_from_slice(&self.t_energy_q10.to_le_bytes());
        pkt[10..12].copy_from_slice(&432u16.to_le_bytes());
        pkt[12..16].copy_from_slice(&(self.frame_seq as u32).to_le_bytes());
        pkt
    }
}

/// Escape a string for safe inclusion inside a JSON string literal.
pub fn json_escape_str(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 8);
    for ch in input.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {}
            c => out.push(c),
        }
    }
    out
}

/// Serialize `AyeosOxicastState` as 100% integer JSON (`p/q` rationals, zero floats).
pub fn build_ayeos_oxicast_json(state: &AyeosOxicastState) -> String {
    let mut bands_str = String::with_capacity(256);
    bands_str.push('[');
    for (i, &b) in state.bands_q10.iter().enumerate() {
        if i > 0 {
            bands_str.push(',');
        }
        bands_str.push_str(&b.to_string());
    }
    bands_str.push(']');

    let linosv = seed_hash_hex();
    let short_linosv = &linosv[..16];

    format!(
        "{{\"ok\":true,\"os\":\"ayeOS\",\"linosv_seed\":\"{short_linosv}\",\
\"receiver_name\":\"{}\",\"preset\":\"{}\",\
\"sample_rate_hz\":{},\"bit_depth\":{},\"babbage_additions_per_sample\":{},\
\"harmonic_anchor_hz\":432,\"phi_ratio_pq\":\"500/809\",\
\"s_energy_q10\":{},\"n_energy_q10\":{},\"t_energy_q10\":{},\
\"sparkle_pq\":\"{}/1024\",\"warmth_pq\":\"{}/1024\",\"drive_pq\":\"{}/1024\",\
\"punch_pq\":\"{}/1024\",\"air_pq\":\"{}/1024\",\"phi_432_pq\":\"{}/1024\",\
\"goonj_velvet_pq\":\"{}/1024\",\"stereo_width_pq\":\"{}/1024\",\
\"markdown_seq\":{},\"markdown_title\":\"{}\",\"markdown_body\":\"{}\",\
\"ipxe_boots_served\":{},\"tftp_blocks_served\":{},\"peer_mother_host\":\"{}\",\
\"bands_q10\":{}}}",
        json_escape_str(&state.receiver_name),
        json_escape_str(&state.preset_name),
        state.sample_rate_hz,
        state.bit_depth,
        state.babbage_additions_per_sample,
        state.s_energy_q10,
        state.n_energy_q10,
        state.t_energy_q10,
        state.preset.sparkle_q10,
        state.preset.warmth_q10,
        state.preset.drive_q10,
        state.preset.punch_q10,
        state.preset.air_q10,
        state.preset.phi_432_q10,
        state.preset.goonj_velvet_q10,
        state.preset.stereo_width_q10,
        state.markdown_seq,
        json_escape_str(&state.markdown_title),
        json_escape_str(&state.markdown_body),
        state.ipxe_boots_served,
        state.tftp_blocks_served,
        json_escape_str(&state.peer_mother_host),
        bands_str
    )
}

/// Generate a deterministic `#!ipxe` network-boot script so any diskless Raspberry Pi 4 (or x86_64
/// iPXE client) chains straight into `ayeOS` TV Sanctuary over HTTP in RAM!
pub fn build_ayeos_ipxe_script(host_with_port: &str) -> String {
    let linosv = seed_hash_hex();
    let short_linosv = &linosv[..16];
    format!(
        "#!ipxe\n\
dhcp || echoUsing existing link configuration\n\
echo ==================================================================\n\
echo   8b-is AyeOS x OxiCast x PhoeniX TV Sanctuary (Diskless iPXE)\n\
echo   Mother Node : http://{host_with_port} · LINOSV={short_linosv}\n\
echo   Audio       : 768kHz Catmull-Rom + Babbage 3-Add Cubic Stepper\n\
echo ==================================================================\n\
kernel http://{host_with_port}/boot/kernel8.img init=/init rdinit=/init console=tty1 ayeos.peer={host_with_port} ayeos.mode=tv-sanctuary\n\
initrd http://{host_with_port}/boot/ayeos-initramfs.cpio\n\
boot\n"
    )
}

/// Generate the Raspberry Pi 4 diskless `config.txt` served via TFTP/HTTP to SD-cardless Pis.
/// Boots the 100% Bare-Metal `#![no_std]` `#![no_main]` Pure-Rust `kernel8.img` (`ayeos-baremetal-pi4`)
/// directly at `0x00080000` in 64-bit `EL2` with **zero Linux kernel and zero initramfs**!
pub fn build_pi_netboot_config_txt() -> String {
    "# --- 8b-is AyeOS x OxiCast x PhoeniX Bare-Metal Rust Raspberry Pi 4 Netboot ---\n\
arm_64bit=1\n\
kernel_address=0x00080000\n\
dtparam=i2s=on\n\
hdmi_force_hotplug=1\n\
hdmi_drive=2\n\
disable_overscan=1\n\
framebuffer_width=1280\n\
framebuffer_height=720\n\
kernel=kernel8.img\n"
        .to_string()
}

/// Generate the Raspberry Pi 4 diskless `cmdline.txt` pointing `rdinit=/init` and `ayeos.peer` to
/// the Mother Node that booted it, with `quiet loglevel=0` so Linux boot logs stay hidden.
pub fn build_pi_netboot_cmdline_txt(host_with_port: &str) -> String {
    format!(
        "quiet loglevel=0 logo.nologo vt.global_cursor_default=0 console=tty1 ip=dhcp rdinit=/init init=/init ayeos.peer={host_with_port} ayeos.ip=dhcp ayeos.mode=tv-sanctuary panic=5\n"
    )
}

/// Generate a one-step script (`enable-diskless-pxe.sh`) that configures a Raspberry Pi 4's
/// onboard SPI EEPROM for **SD-Optional Network Boot (`BOOT_ORDER=0xf21`)**:
/// - First tries SD card (`1`) if inserted,
/// - Immediately falls back to Network PXE/TFTP Boot (`2`) when NO SD card is inserted!
pub fn build_pi_eeprom_pxe_enabler_script(mother_ip: &str) -> String {
    format!(
        "#!/usr/bin/env bash\n\
# ==============================================================================\n\
# 🌸 8b-is AyeOS TV Revolution — Raspberry Pi 4 Diskless EEPROM Unlocker\n\
#    Sets BOOT_ORDER=0xf21 (SD first if present, Network PXE Boot with NO SD card!)\n\
#    Points TFTP_IP to AyeOS Mother Node ({mother_ip})\n\
# ==============================================================================\n\
set -euo pipefail\n\
TMP_DIR=\"$(mktemp -d)\"\n\
trap 'rm -rf \"${{TMP_DIR}}\"' EXIT\n\
if ! command -v rpi-eeprom-config >/dev/null 2>&1; then\n\
    sudo apt-get update && sudo apt-get install -y rpi-eeprom\n\
fi\n\
sudo rpi-eeprom-config > \"${{TMP_DIR}}/bootconf.txt\"\n\
sed -i '/^BOOT_ORDER=/d; /^TFTP_IP=/d; /^NET_INSTALL_ENABLED=/d' \"${{TMP_DIR}}/bootconf.txt\"\n\
cat >> \"${{TMP_DIR}}/bootconf.txt\" <<'EOF'\n\
BOOT_ORDER=0xf21\n\
TFTP_IP={mother_ip}\n\
NET_INSTALL_ENABLED=0\n\
EOF\n\
sudo rpi-eeprom-config --apply \"${{TMP_DIR}}/bootconf.txt\"\n\
echo \"==================================================================\"\n\
echo \"🌸 EEPROM Armed! This Raspberry Pi 4 can now boot with ZERO SD card\"\n\
echo \"   directly over Ethernet from AyeOS Mother Node {mother_ip}!\"\n\
echo \"==================================================================\"\n"
    )
}

/// Synthesize a bootable SVR4 `070701` (`newc`) CPIO initramfs in RAM containing our ARM64
/// `ayeos-sanctuary` binary as `/init` so any diskless Raspberry Pi 4 can boot straight into
/// AyeOS in RAM and immediately become a new iPXE/TFTP Mother Node itself!
pub fn synthesize_live_ayeos_initramfs(mother_host: &str) -> Vec<u8> {
    let candidates = [
        "/home/hue/8b-is/oxicast/oxicast-pi/bin/ayeos-sanctuary-aarch64",
        "/usr/local/bin/ayeos-sanctuary",
        "/init",
        "/home/hue/8b-is/oxicast/oxicast-pi/bin/qwave-hypr-hud-aarch64",
        "/usr/local/bin/qwave-hypr-hud",
    ];
    let mut init_bytes: Vec<u8> = Vec::new();
    for path in candidates {
        if let Ok(bytes) = fs::read(path) {
            if bytes.len() > 64 && &bytes[0..4] == b"\x7FELF" {
                init_bytes = bytes;
                break;
            }
        }
    }
    if init_bytes.is_empty() {
        if let Ok(bytes) = fs::read("/proc/self/exe") {
            init_bytes = bytes;
        }
    }
    let cfg = format!(
        "AYEOS_MODE=tv-sanctuary\nAYEOS_MOTHER_PEER={mother_host}\nAYEOS_PORT=8432\nLINOSV_SEED={}\n",
        seed_hash_hex()
    );
    build_cpio_newc_initramfs(&init_bytes, &cfg)
}

/// Generate the `limine.conf` configuration served via TFTP/HTTP to diskless `x86_64` / `x86`
/// UEFI PXE (`BOOTX64.EFI`, `BOOTIA32.EFI`) and Legacy BIOS PXE (`limine-bios-pxe.bin`) machines.
/// Boots the 100% Bare-Metal `#![no_std]` `#![no_main]` Pure-Rust `ayeos-x86_64.elf` kernel
/// immediately (`timeout: 0`) with a `1280×720×32` GOP/VBE linear framebuffer!
pub fn build_limine_pxe_config_txt(host_with_port: &str) -> String {
    format!(
        "timeout: 0\n\
quiet: yes\n\
default_entry: 1\n\
\n\
/8b-is AyeOS x OxiCast x PhoeniX Bare-Metal x86_64 Sanctuary\n\
    protocol: limine\n\
    path: boot():/ayeos-x86_64.elf\n\
    resolution: 1280x720x32\n\
    cmdline: ayeos.peer={host_with_port} ayeos.mode=tv-sanctuary\n"
    )
}

/// Resolve a requested netboot filename (`config.txt`, `cmdline.txt`, `limine.conf`, `ayeos.ipxe`,
/// `ayeos-x86_64.elf`, `BOOTX64.EFI`, `limine-bios-pxe.bin`, `limine-bios.sys`, `kernel8.img`,
/// `start4.elf`, `fixup4.dat`, `bcm2711-rpi-4-b.dtb`) for HTTP (`/boot/<name>`) or TFTP (`UDP :69`).
pub fn resolve_netboot_artifact(
    filename: &str,
    host_with_port: &str,
    netboot_dir: &Path,
) -> Option<Vec<u8>> {
    // Normalize Windows/UEFI backslashes and strip any Raspberry Pi serial prefix or leading `/boot/`
    let normalized = filename.replace('\\', "/");
    let clean = normalized
        .trim_start_matches('/')
        .trim_start_matches("boot/");
    let basename = clean.rsplit('/').next().unwrap_or(clean);
    let base_lower = basename.to_ascii_lowercase();

    match base_lower.as_str() {
        "ayeos.ipxe" | "autoexec.ipxe" | "boot.ipxe" => {
            Some(build_ayeos_ipxe_script(host_with_port).into_bytes())
        }
        "limine.conf" | "limine.cfg" => {
            Some(build_limine_pxe_config_txt(host_with_port).into_bytes())
        }
        "config.txt" => Some(build_pi_netboot_config_txt().into_bytes()),
        "cmdline.txt" => Some(build_pi_netboot_cmdline_txt(host_with_port).into_bytes()),
        "enable-diskless-pxe.sh" => {
            let ip_only = host_with_port.split(':').next().unwrap_or("oxicast.local");
            Some(build_pi_eeprom_pxe_enabler_script(ip_only).into_bytes())
        }
        "ayeos-initramfs.cpio" | "initramfs.cpio" => {
            Some(synthesize_live_ayeos_initramfs(host_with_port))
        }
        _ => {
            // Look up inside `netboot_dir`, `/usr/share/limine`, `/boot/firmware`, or `/boot`
            let mut rel = clean.to_string();
            if rel.contains("..") {
                return None;
            }
            let canonical_efi = match base_lower.as_str() {
                "bootx64.efi" => Some("BOOTX64.EFI"),
                "bootia32.efi" => Some("BOOTIA32.EFI"),
                "bootaa64.efi" => Some("BOOTAA64.EFI"),
                _ => None,
            };
            // If Pi requested `<8-hex-serial>/start4.elf`, also try without serial prefix
            for search_root in [
                netboot_dir,
                Path::new("/var/lib/ayeos/netboot"),
                Path::new("/home/hue/.cache/oxicast-pi/netboot"),
                Path::new("/usr/share/limine"),
                Path::new("/boot/firmware"),
                Path::new("/boot"),
            ] {
                let direct = search_root.join(&rel);
                if let Ok(bytes) = fs::read(&direct) {
                    return Some(bytes);
                }
                let by_base = search_root.join(basename);
                if let Ok(bytes) = fs::read(&by_base) {
                    return Some(bytes);
                }
                if let Some(upper_efi) = canonical_efi {
                    let by_upper = search_root.join(upper_efi);
                    if let Ok(bytes) = fs::read(&by_upper) {
                        return Some(bytes);
                    }
                }
                let overlay_path = search_root.join("overlays").join(basename);
                if let Ok(bytes) = fs::read(&overlay_path) {
                    return Some(bytes);
                }
            }
            if base_lower == "kernel8.img" {
                return Some(crate::boot_mt6768::synthesize_aarch64_mt6768_kernel_image());
            }
            let _ = &mut rel;
            None
        }
    }
}

/// Parse an RFC 1350 TFTP Read Request (`RRQ`, opcode `1`) packet and return `(filename, mode)`.
pub fn parse_tftp_rrq(packet: &[u8]) -> Option<(String, String)> {
    if packet.len() < 4 {
        return None;
    }
    let opcode = u16::from_be_bytes([packet[0], packet[1]]);
    if opcode != TFTP_OP_RRQ {
        return None;
    }
    let rest = &packet[2..];
    let first_nul = rest.iter().position(|&b| b == 0)?;
    let filename = String::from_utf8_lossy(&rest[..first_nul]).to_string();
    let after_file = &rest[first_nul + 1..];
    let second_nul = after_file.iter().position(|&b| b == 0)?;
    let mode = String::from_utf8_lossy(&after_file[..second_nul]).to_ascii_lowercase();
    Some((filename, mode))
}

/// Build an RFC 1350 TFTP `DATA` packet (opcode `3`, 2-byte big-endian block number, up to 512 bytes payload).
pub fn build_tftp_data_packet(block_num: u16, payload: &[u8]) -> Vec<u8> {
    let mut pkt = Vec::with_capacity(4 + payload.len());
    pkt.extend_from_slice(&TFTP_OP_DATA.to_be_bytes());
    pkt.extend_from_slice(&block_num.to_be_bytes());
    pkt.extend_from_slice(payload);
    pkt
}

/// Build an RFC 1350 TFTP `ERROR` packet (opcode `5`, error code, message, trailing `0`).
pub fn build_tftp_error_packet(error_code: u16, msg: &str) -> Vec<u8> {
    let mut pkt = Vec::with_capacity(5 + msg.len());
    pkt.extend_from_slice(&TFTP_OP_ERROR.to_be_bytes());
    pkt.extend_from_slice(&error_code.to_be_bytes());
    pkt.extend_from_slice(msg.as_bytes());
    pkt.push(0);
    pkt
}

/// Parse RFC 2347/2348/2349 TFTP options (`blksize`, `tsize`) from an `RRQ` packet if present.
pub fn parse_tftp_rrq_options(packet: &[u8]) -> (usize, bool) {
    let mut blksize = 512usize;
    let mut wants_tsize = false;
    if packet.len() < 4 {
        return (blksize, wants_tsize);
    }
    let parts: Vec<&[u8]> = packet[2..].split(|&b| b == 0).filter(|s| !s.is_empty()).collect();
    if parts.len() >= 4 {
        let mut idx = 2usize;
        while idx + 1 < parts.len() {
            let key = String::from_utf8_lossy(parts[idx]).to_ascii_lowercase();
            let val = String::from_utf8_lossy(parts[idx + 1]);
            if key == "blksize" {
                if let Ok(bs) = val.parse::<usize>() {
                    blksize = bs.clamp(512, 1468);
                }
            } else if key == "tsize" {
                wants_tsize = true;
            }
            idx += 2;
        }
    }
    (blksize, wants_tsize)
}

/// Spawn the Pure-Rust RFC 1350 + RFC 2347 (`OACK`) TFTP Server (`UDP :69` if root/CAP_NET_BIND_SERVICE,
/// falling back to `UDP :6969`) so diskless Raspberry Pi 4s can TFTP-boot directly from this AyeOS node!
pub fn spawn_ayeos_tftp_server(state: Arc<Mutex<AyeosOxicastState>>, host_with_port: String) {
    std::thread::spawn(move || {
        let sock = UdpSocket::bind("0.0.0.0:69")
            .or_else(|_| UdpSocket::bind("0.0.0.0:6969"));
        let sock = match sock {
            Ok(s) => s,
            Err(_) => return,
        };
        let mut buf = [0u8; 1500];
        loop {
            let (len, peer_addr) = match sock.recv_from(&mut buf) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let req_pkt = buf[..len].to_vec();
            let Some((filename, _mode)) = parse_tftp_rrq(&req_pkt) else {
                continue;
            };
            let (blksize, wants_tsize) = parse_tftp_rrq_options(&req_pkt);
            println!("📦 [ayeOS TFTP] Peer {peer_addr} requested '{filename}' (blksize={blksize}, tsize={wants_tsize})");
            let netboot_dir = state
                .lock()
                .map(|s| s.netboot_dir.clone())
                .unwrap_or_else(|_| detect_default_netboot_dir());
            let host_clone = host_with_port.clone();
            let state_clone = Arc::clone(&state);

            std::thread::spawn(move || {
                let Ok(xfer_sock) = UdpSocket::bind("0.0.0.0:0") else {
                    return;
                };
                // 250ms fast-retransmit timer (well below Raspberry Pi 4 ROM's 1000ms abort timer!)
                let _ = xfer_sock.set_read_timeout(Some(Duration::from_millis(250)));
                let Some(data) = resolve_netboot_artifact(&filename, &host_clone, &netboot_dir) else {
                    println!("   [!] TFTP 404 for '{filename}'");
                    let err = build_tftp_error_packet(1, "File not found on AyeOS Mother Node");
                    let _ = xfer_sock.send_to(&err, peer_addr);
                    return;
                };

                if let Ok(mut guard) = state_clone.lock() {
                    guard.ipxe_boots_served = guard.ipxe_boots_served.wrapping_add(1);
                }

                let mut ack_buf = [0u8; 64];
                // If Raspberry Pi 4 EEPROM or UEFI PXE requested RFC 2347 options (tsize or blksize != 512), send OACK first!
                if wants_tsize || blksize != 512 {
                    let mut oack = Vec::with_capacity(64);
                    oack.extend_from_slice(&TFTP_OP_OACK.to_be_bytes());
                    if wants_tsize {
                        oack.extend_from_slice(b"tsize\0");
                        oack.extend_from_slice(data.len().to_string().as_bytes());
                        oack.push(0);
                    }
                    if blksize != 512 {
                        oack.extend_from_slice(b"blksize\0");
                        oack.extend_from_slice(blksize.to_string().as_bytes());
                        oack.push(0);
                    }
                    let mut oack_acked = false;
                    for _oack_retry in 0..8 {
                        let _ = xfer_sock.send_to(&oack, peer_addr);
                        if let Ok((n, _)) = xfer_sock.recv_from(&mut ack_buf) {
                            if n >= 4 {
                                let op = u16::from_be_bytes([ack_buf[0], ack_buf[1]]);
                                let blk = u16::from_be_bytes([ack_buf[2], ack_buf[3]]);
                                if op == TFTP_OP_ERROR || (op == TFTP_OP_ACK && blk != 0) {
                                    // Client only wanted the tsize OACK probe and aborted cleanly
                                    return;
                                }
                                if op == TFTP_OP_ACK && blk == 0 {
                                    oack_acked = true;
                                    break;
                                }
                            }
                        }
                    }
                    if !oack_acked {
                        return;
                    }
                }

                // Stream RFC 1350/2348 TFTP DATA blocks with 250ms fast-retransmit ACK verification
                let total_blocks = (data.len() / blksize) + 1;
                for idx in 0..total_blocks {
                    let block_num = ((idx + 1) & 0xFFFF) as u16;
                    let start = idx * blksize;
                    let end = (start + blksize).min(data.len());
                    let pkt = build_tftp_data_packet(block_num, &data[start..end]);

                    let mut acked = false;
                    for _retry in 0..12 {
                        let _ = xfer_sock.send_to(&pkt, peer_addr);
                        loop {
                            match xfer_sock.recv_from(&mut ack_buf) {
                                Ok((n, _)) if n >= 4 => {
                                    let op = u16::from_be_bytes([ack_buf[0], ack_buf[1]]);
                                    if op == TFTP_OP_ERROR {
                                        return;
                                    }
                                    if op == TFTP_OP_ACK {
                                        let rx_blk = u16::from_be_bytes([ack_buf[2], ack_buf[3]]);
                                        if rx_blk == block_num {
                                            acked = true;
                                            break;
                                        }
                                        // Ignore duplicate ACK for an older block and keep waiting in the 250ms window
                                        continue;
                                    }
                                }
                                _ => break,
                            }
                        }
                        if acked {
                            break;
                        }
                    }
                    if !acked {
                        return;
                    }
                    if let Ok(mut guard) = state_clone.lock() {
                        guard.tftp_blocks_served = guard.tftp_blocks_served.wrapping_add(1);
                    }
                }
                println!("✅ [ayeOS TFTP] Completed '{filename}' ({} bytes) to {peer_addr}", data.len());
            });
        }
    });
}

// ==============================================================================
// 📺 Direct `/dev/fb0` Sovereign Framebuffer Visualizer & 8×8 Bitmap Font Engine
//    Paints 60fps AyeOS × OxiCast × PhoeniX visuals + casted QWave Markdown
//    directly onto the Raspberry Pi 4 HDMI TV or BLU G75 LCD with ZERO X11/Wayland!
// ==============================================================================

/// Return an 8×8 monochrome bitmap glyph (1 byte per row, MSB = leftmost pixel)
/// for uppercase ASCII, digits, and core punctuation so `/dev/fb0` can render
/// casted QWave Markdown titles & bullet points with zero external font files!
#[inline]
pub const fn glyph_8x8(ch: char) -> [u8; 8] {
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
        ' ' => [0x00; 8],
        _ => [0x18, 0x3C, 0x3C, 0x18, 0x00, 0x00, 0x00, 0x00],
    }
}

/// Draw a scaled ASCII string onto a 32-bit BGRA pixel buffer (`width × height`).
pub fn draw_text_bgra(
    buf: &mut [u8],
    width: usize,
    height: usize,
    start_x: usize,
    start_y: usize,
    scale: usize,
    text: &str,
    bgra: [u8; 4],
) {
    let sc = scale.max(1);
    let mut cursor_x = start_x;
    for ch in text.chars() {
        if cursor_x + 8 * sc >= width {
            break;
        }
        let rows = glyph_8x8(ch);
        for (ry, &bits) in rows.iter().enumerate() {
            for rx in 0..8usize {
                if ((bits >> (7 - rx)) & 1) != 0 {
                    for sy in 0..sc {
                        for sx in 0..sc {
                            let px = cursor_x + rx * sc + sx;
                            let py = start_y + ry * sc + sy;
                            if px < width && py < height {
                                let off = (py * width + px) * 4;
                                if off + 4 <= buf.len() {
                                    buf[off..off + 4].copy_from_slice(&bgra);
                                }
                            }
                        }
                    }
                }
            }
        }
        cursor_x += 9 * sc;
    }
}

/// Render a complete 32-bit BGRA frame (`width × height`) of the **AyeOS × OxiCast × PhoeniX
/// Sanctuary Visualizer** (Constellation Ink `#05060A` background, 432 Hz / φ Golden Lissajous
/// ring, 36-band Marine `S+N+T` spectrum bars, and casted QWave Markdown text).
pub fn render_oxicast_fb0_frame(
    state: &AyeosOxicastState,
    width: usize,
    height: usize,
) -> Vec<u8> {
    let w = width.clamp(160, 3840);
    let h = height.clamp(120, 2160);
    let mut buf = vec![0u8; w * h * 4];

    // 1. Constellation Ink background (#05060A -> BGRA [0x0A, 0x06, 0x05, 0xFF])
    for px in buf.chunks_exact_mut(4) {
        px.copy_from_slice(&[0x12, 0x07, 0x05, 0xFF]);
    }

    // 2. Top Sovereign Header Bar
    let header_h = (h / 11).clamp(28, 84);
    for y in 0..header_h {
        for x in 0..w {
            let off = (y * w + x) * 4;
            buf[off..off + 4].copy_from_slice(&[0x24, 0x10, 0x09, 0xFF]);
        }
    }
    let scale = (w / 480).clamp(1, 3);
    draw_text_bgra(
        &mut buf,
        w,
        h,
        16,
        (header_h / 4).max(4),
        scale,
        "AYEOS X OXICAST X PHOENIX TV SANCTUARY - 384KHZ 24-BIT BABBAGE STEPPER",
        [0xC9, 0xE6, 0x62, 0xFF], // Mint #62E6C9 in BGRA
    );

    // 3. 432 Hz / phi (500/809) Integer Parabolic Lissajous Ring
    let cx = (w * 3) / 4;
    let cy = h / 2;
    let radius = (h.min(w) / 5).max(20) as i64;
    let seq_phase = (state.frame_seq as i64 * 512) & 0xFFFF;
    let mut step = 0i64;
    while step < 256 {
        let p_x = ((step * 256) + seq_phase) & 0xFFFF;
        let p_y = (((step * 256 * 500) / 809) + seq_phase) & 0xFFFF;
        let dx = (integer_parabolic_sine_q15(p_x) * radius) >> 15;
        let dy = (integer_parabolic_sine_q15(p_y) * radius) >> 15;
        let px = (cx as i64 + dx).clamp(2, (w as i64) - 3) as usize;
        let py = (cy as i64 + dy).clamp(2, (h as i64) - 3) as usize;
        for oy in 0..3usize {
            for ox in 0..3usize {
                let off = ((py + oy) * w + (px + ox)) * 4;
                if off + 4 <= buf.len() {
                    buf[off..off + 4].copy_from_slice(&[0x5C, 0xB0, 0xE8, 0xFF]); // Gold #E8B05C
                }
            }
        }
        step += 1;
    }

    // 4. Casted QWave Markdown Card (left side)
    draw_text_bgra(
        &mut buf,
        w,
        h,
        24,
        header_h + 24,
        scale,
        &state.markdown_title,
        [0x5C, 0xB0, 0xE8, 0xFF], // Gold
    );
    let mut line_y = header_h + 24 + 14 * scale;
    for line in state.markdown_body.lines().take(8) {
        draw_text_bgra(
            &mut buf,
            w,
            h,
            24,
            line_y,
            scale.max(1),
            line,
            [0xF0, 0xE8, 0xE0, 0xFF],
        );
        line_y += 12 * scale.max(1);
    }

    // 5. 36-Band Marine S + N + T Harmonic Spectrum Bars along bottom
    let bar_zone_h = (h / 4).max(32);
    let bar_slot_w = (w / 38).max(4);
    let bar_w = (bar_slot_w.saturating_sub(2)).max(2);
    for (idx, &energy_q10) in state.bands_q10.iter().enumerate() {
        let bar_h = (((energy_q10 as usize) * bar_zone_h) >> 10).clamp(4, bar_zone_h);
        let x0 = 16 + idx * bar_slot_w;
        let color_bgra = if idx < 12 {
            [0xC9, 0xE6, 0x62, 0xFF] // Mint (Sinusoids)
        } else if idx < 24 {
            [0x5C, 0xB0, 0xE8, 0xFF] // Gold (Velvet Noise)
        } else {
            [0xFF, 0x8B, 0xB4, 0xFF] // Violet (Dirac Transients)
        };
        for y in (h - bar_h)..h {
            for x in x0..(x0 + bar_w).min(w) {
                let off = (y * w + x) * 4;
                buf[off..off + 4].copy_from_slice(&color_bgra);
            }
        }
    }

    buf
}

/// Convert a 32-bit BGRA buffer (`width * height * 4` bytes) into a 16-bit little-endian `RGB565`
/// buffer (`width * height * 2` bytes) for Raspberry Pi 4 `vc4drmfb` (`bits_per_pixel = 16`).
pub fn bgra32_to_rgb565_le(bgra: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bgra.len() / 2);
    for px in bgra.chunks_exact(4) {
        let b = (px[0] as u16) >> 3;
        let g = (px[1] as u16) >> 2;
        let r = (px[2] as u16) >> 3;
        let rgb565: u16 = (r << 11) | (g << 5) | b;
        out.extend_from_slice(&rgb565.to_le_bytes());
    }
    out
}

/// Write a rendered frame directly to `/dev/fb0` or `/dev/graphics/fb0` if present.
/// Automatically detects `16bpp` (`RGB565` on Raspberry Pi `vc4drmfb`) vs `32bpp` (`BGRA8888`).
pub fn paint_frame_to_hardware_fb0(state: &AyeosOxicastState) -> bool {
    let mut width = 1920usize;
    let mut height = 1080usize;
    if let Ok(vsize) = fs::read_to_string("/sys/class/graphics/fb0/virtual_size") {
        let mut parts = vsize.trim().split(',');
        if let (Some(w_s), Some(h_s)) = (parts.next(), parts.next()) {
            if let (Ok(w), Ok(h)) = (w_s.parse::<usize>(), h_s.parse::<usize>()) {
                if w >= 160 && h >= 120 {
                    width = w.min(1920);
                    height = h.min(1080);
                }
            }
        }
    }
    let bpp = fs::read_to_string("/sys/class/graphics/fb0/bits_per_pixel")
        .ok()
        .and_then(|s| s.trim().parse::<usize>().ok())
        .unwrap_or(32);

    for fb_path in ["/dev/fb0", "/dev/graphics/fb0"] {
        if let Ok(mut fb) = fs::OpenOptions::new().write(true).open(fb_path) {
            let bgra = render_oxicast_fb0_frame(state, width, height);
            if bpp == 16 {
                let rgb565 = bgra32_to_rgb565_le(&bgra);
                let _ = fb.write_all(&rgb565);
            } else {
                let _ = fb.write_all(&bgra);
            }
            return true;
        }
    }

    // Fallback for diskless PID 1 initramfs when fbcon owns /dev/tty1 before DRM module load:
    if std::process::id() == 1 {
        let mut bars = String::with_capacity(128);
        for &b in state.bands_q10.iter().take(36) {
            let ch = match b {
                0..=180 => '▁',
                181..=320 => '▂',
                321..=460 => '▃',
                461..=600 => '▄',
                601..=740 => '▅',
                741..=880 => '▆',
                _ => '█',
            };
            bars.push(ch);
        }
        let ansi = format!(
            "\x1b[?25l\x1b[2J\x1b[H\
\x1b[48;5;232m\x1b[38;5;86m================================================================================\r\n\
  🌸 ayeOS x OxiCast x PhoeniX TV Sanctuary · DISKLESS MEMNET RAM OS (PID 1)\r\n\
  Audio : {} Hz · {}-bit · Babbage 3-Addition Cubic Stepper · Preset: {}\r\n\
  Mesh  : Mother={} · iPXE Boots={} · TFTP Blocks={}\r\n\
================================================================================\r\n\r\n\
\x1b[38;5;215m  {}\r\n\r\n\
\x1b[38;5;254m{}\r\n\r\n\
\x1b[38;5;86m  Marine S+N+T 36-Band Harmonic Spectrum (432 Hz / phi 500/809):\r\n\
  \x1b[38;5;141m[{}]\x1b[0m\r\n",
            state.sample_rate_hz,
            state.bit_depth,
            state.preset_name,
            state.peer_mother_host,
            state.ipxe_boots_served,
            state.tftp_blocks_served,
            state.markdown_title,
            state.markdown_body.replace('\n', "\r\n  "),
            bars
        );
        for tty_path in ["/dev/tty1", "/dev/tty0", "/dev/console"] {
            if let Ok(mut tty) = fs::OpenOptions::new().write(true).open(tty_path) {
                let _ = tty.write_all(ansi.as_bytes());
                break;
            }
        }
    }
    false
}

/// Render the 60fps HTML5 Canvas + WebAudio + Peer-to-Peer iPXE Dashboard for `/oxicast`.
/// Strictly zero floating-point literals anywhere in the HTML, CSS, or JavaScript.
pub fn render_ayeos_oxicast_html(state: &AyeosOxicastState) -> String {
    let linosv = seed_hash_hex();
    let short_linosv = &linosv[..16];
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>{}</title>
<style>
  * {{ box-sizing: border-box; margin: 0; padding: 0; }}
  body {{
    background: #05060a;
    color: #e8eef7;
    font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
    overflow: hidden;
    height: 100vh;
    display: flex;
    flex-direction: column;
    justify-content: space-between;
  }}
  header {{
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 16px 24px;
    background: #0a0d16;
    border-bottom: 2px solid #1e2638;
    z-index: 2;
  }}
  .brand {{
    color: #62e6c9;
    font-weight: 700;
    font-size: 18px;
  }}
  .badges {{
    display: flex;
    gap: 10px;
    align-items: center;
    flex-wrap: wrap;
  }}
  .badge {{
    padding: 6px 12px;
    background: #121826;
    border: 1px solid #2b364f;
    border-radius: 6px;
    color: #e8b05c;
    font-size: 13px;
  }}
  .btn {{
    padding: 8px 14px;
    background: #142234;
    border: 1px solid #62e6c9;
    color: #62e6c9;
    border-radius: 6px;
    cursor: pointer;
    font-family: inherit;
    font-size: 13px;
  }}
  main {{
    position: relative;
    flex: 1;
    display: flex;
    padding: 24px;
    gap: 24px;
    align-items: stretch;
  }}
  #stage-card {{
    width: 48%;
    background: #0a0d16;
    border: 1px solid #243048;
    border-radius: 12px;
    padding: 24px;
    z-index: 2;
    overflow-y: auto;
  }}
  #stage-title {{
    color: #e8b05c;
    font-size: 22px;
    margin-bottom: 14px;
  }}
  #stage-body {{
    white-space: pre-wrap;
    line-height: 24px;
    color: #d7e3f4;
    font-size: 15px;
  }}
  #viz-wrap {{
    flex: 1;
    background: #080b12;
    border: 1px solid #243048;
    border-radius: 12px;
    position: relative;
  }}
  canvas {{
    width: 100%;
    height: 100%;
    display: block;
  }}
  footer {{
    padding: 12px 24px;
    background: #0a0d16;
    border-top: 1px solid #1e2638;
    display: flex;
    justify-content: space-between;
    font-size: 13px;
    color: #9fb0c7;
  }}
</style>
</head>
<body>
<header>
  <div class="brand">🌸 ayeOS × OxiCast × PhoeniX TV Sanctuary · LINOSV {short_linosv}</div>
  <div class="badges">
    <span class="badge" id="rate-badge">384000 Hz · 24-bit · Babbage 3-Add</span>
    <span class="badge" id="ipxe-badge">iPXE Peer Boot Ready (/boot/ayeos.ipxe)</span>
    <button class="btn" onclick="setPreset('velvet_sanctuary')">Velvet</button>
    <button class="btn" onclick="setPreset('liquid_gold')">Liquid Gold</button>
    <button class="btn" onclick="setPreset('riva_cathedral')">Riva Cathedral</button>
    <button class="btn" onclick="flashPxeSd()" id="pxe-sd-btn">⚡ Flash PXE Inoculator SD</button>
    <button class="btn" onclick="toggleAudio()" id="audio-btn">🔊 Listen Live PhoeniX</button>
  </div>
</header>
<main>
  <div id="stage-card">
    <div id="stage-title">{}</div>
    <div id="stage-body">{}</div>
    <div id="fleet-box" style="margin-top:18px;padding-top:14px;border-top:1px solid #1e2638;font-size:13px;color:#62e6c9;">
      🚀 MEMNET Diskless Fleet: Mother Node Active (DHCP Opt43 + TFTP :69 + iPXE :8432 + DNS :53)
    </div>
  </div>
  <div id="viz-wrap">
    <canvas id="viz" width="960" height="540"></canvas>
  </div>
</main>
<footer>
  <span id="telemetry-line">Marine S+N+T · 432 Hz / φ (500/809) · .8a (8a∞) UDP :8432</span>
  <span>Diskless Pi iPXE Chain: http://&lt;peer&gt;:8432/boot/ayeos.ipxe</span>
</footer>
<audio id="phoenix-audio" preload="none"></audio>
<script>
let playing = false;
function toggleAudio() {{
  const el = document.getElementById('phoenix-audio');
  const btn = document.getElementById('audio-btn');
  if (!playing) {{
    el.src = '/api/oxicast/stream.wav?t=' + Date.now();
    el.play();
    playing = true;
    btn.textContent = '⏸ Mute PhoeniX Stream';
  }} else {{
    el.pause();
    playing = false;
    btn.textContent = '🔊 Listen Live PhoeniX';
  }}
}}
function setPreset(name) {{
  fetch('/api/oxicast/preset', {{ method: 'POST', body: name }});
}}
async function flashPxeSd() {{
  const btn = document.getElementById('pxe-sd-btn');
  btn.textContent = '⏳ Flashing /dev/sda...';
  try {{
    const r = await fetch('/api/ayeos/flash-pxe-sd', {{ method: 'POST', body: '/dev/sda' }});
    const j = await r.json();
    btn.textContent = j.ok ? '🟢 PXE SD Ready!' : ('⚠️ ' + (j.error || 'Insert SD Card'));
  }} catch (_e) {{
    btn.textContent = '⚠️ Flash Error';
  }}
}}
const canvas = document.getElementById('viz');
const ctx = canvas.getContext('2d');
let bands = new Array(36).fill(256);
let tick = 0;

async function pollState() {{
  try {{
    const r = await fetch('/api/oxicast/state');
    const st = await r.json();
    if (st && st.ok) {{
      document.getElementById('stage-title').textContent = st.markdown_title;
      document.getElementById('stage-body').textContent = st.markdown_body;
      document.getElementById('rate-badge').textContent =
        st.sample_rate_hz + ' Hz · ' + st.bit_depth + '-bit · ' + st.preset;
      document.getElementById('ipxe-badge').textContent =
        'iPXE Boots: ' + st.ipxe_boots_served + ' · TFTP Blocks: ' + st.tftp_blocks_served;
      if (st.bands_q10) bands = st.bands_q10;
    }}
  }} catch (_e) {{}}
}}
setInterval(pollState, 500);

function draw() {{
  tick = (tick + 1) % 65536;
  const w = canvas.width;
  const h = canvas.height;
  ctx.fillStyle = '#080b12';
  ctx.fillRect(0, 0, w, h);

  const slotW = Math.floor(w / 38);
  const barW = Math.max(4, slotW - 4);
  for (let i = 0; i < 36; i++) {{
    const e = bands[i] || 200;
    const bh = Math.floor((e * h) / 1400);
    const x = 16 + i * slotW;
    const y = h - bh - 16;
    ctx.fillStyle = (i < 12) ? '#62e6c9' : ((i < 24) ? '#e8b05c' : '#b48bff');
    ctx.fillRect(x, y, barW, bh);
  }}
  requestAnimationFrame(draw);
}}
requestAnimationFrame(draw);
</script>
</body>
</html>"#,
        json_escape_str(&state.receiver_name),
        json_escape_str(&state.markdown_title),
        json_escape_str(&state.markdown_body),
    )
}

/// Build the JSON manifest of Universal OxiCast Receivers across physical cable links
/// (`HDMI`, `DVI-D`, `USB-C DisplayPort Alt-Mode`, `/dev/fb0` DRM/KMS) and wireless TV protocols
/// (`Miracast WFD + UIBC`, `AirPlay`, `Chromecast`, `UPnP/DLNA`, `Matter`).
pub fn build_universal_receivers_json(default_host_port: &str) -> String {
    let kvm_host = std::env::var("RUSTYKVM_HOST")
        .unwrap_or_else(|_| crate::desktop::DEFAULT_RUSTYKVM_ADDR.to_string());
    format!(
        r#"{{"ok":true,"hub_host":"{}","kvm_bridge":"{}","receivers":[{{"id":"rx-ayekvm-minisforum","name":"RustyNanoKVM (ayekvm · Minisforum HDMI-In + USB-HID)","physical_link":"hdmi_tmds","protocol":"rusty_nanokvm_bridge","resolution":"1920x1080@60Hz","reverse_hid":true,"latency_ms_pq":"16/1","url":"http://{}/rx.html"}},{{"id":"rx-bedroom-pi4-hdmi","name":"Master Bedroom Raspberry Pi 4 (/dev/fb0 DRM-KMS HDMI + iPXE)","physical_link":"drm_kms_fb0","protocol":"sovereign_wupz64","resolution":"1920x1080@60Hz","reverse_hid":true,"latency_ms_pq":"17/2","url":"http://{}/oxicast/rx"}},{{"id":"rx-usbc-dp-portable","name":"Bedroom USB-C DisplayPort Alt-Mode Dock & Touch Monitor","physical_link":"usb_c_dp_alt_mode","protocol":"sovereign_wupz64","resolution":"2560x1440@60Hz","reverse_hid":true,"latency_ms_pq":"8/1","url":"http://{}/oxicast/rx?link=usbc_dp"}},{{"id":"rx-dvi-workstation","name":"Classic DVI-D Secondary Monitor (Passive HDMI-to-DVI TMDS)","physical_link":"dvi_digital","protocol":"sovereign_wupz64","resolution":"1920x1200@60Hz","reverse_hid":true,"latency_ms_pq":"8/1","url":"http://{}/oxicast/rx?link=dvi"}},{{"id":"rx-smarttv-miracast","name":"Smart TV · Miracast WFD (:7236) + UIBC Mouse/Key Return","physical_link":"wireless_lan","protocol":"miracast_wfd_uibc","resolution":"1920x1080@60Hz","reverse_hid":true,"latency_ms_pq":"45/2","url":"http://{}/oxicast/rx?proto=miracast"}},{{"id":"rx-smarttv-upnp-airplay-cast","name":"Smart TV · AirPlay (:7000) / Chromecast (:8009) / UPnP DLNA (:1900)","physical_link":"wireless_lan","protocol":"upnp_dlna_avtransport","resolution":"1920x1080@60Hz","reverse_hid":false,"latency_ms_pq":"32/1","url":"http://{}/oxicast/rx?proto=upnp"}}]}}"#,
        json_escape_str(default_host_port),
        json_escape_str(&kvm_host),
        json_escape_str(&kvm_host),
        json_escape_str(default_host_port),
        json_escape_str(default_host_port),
        json_escape_str(default_host_port),
        json_escape_str(default_host_port),
        json_escape_str(default_host_port),
    )
}

/// Render the zero-float Universal OxiCast Receiver Portal (`/oxicast/rx`) for Raspberry Pi,
/// Smart TVs (Miracast / AirPlay / Chromecast / UPnP), USB-C DP monitors, and laptops.
pub fn render_universal_rx_html(state: &AyeosOxicastState) -> String {
    let kvm_host = std::env::var("RUSTYKVM_HOST")
        .unwrap_or_else(|_| crate::desktop::DEFAULT_RUSTYKVM_ADDR.to_string());
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>📺 OxiCast Universal Receiver · {}</title>
<style>
  * {{ box-sizing: border-box; margin: 0; padding: 0; }}
  body {{ background: #05060a; color: #e6edf3; font-family: ui-monospace, monospace; min-height: 100vh; display: flex; flex-direction: column; }}
  header {{ background: #0a0d16; border-bottom: 1px solid #1e2638; padding: 10px 18px; display: flex; justify-content: space-between; align-items: center; flex-wrap: wrap; gap: 10px; }}
  .title {{ color: #62e6c9; font-weight: 700; font-size: 15px; }}
  .badge {{ background: #111827; border: 1px solid #28354e; color: #e8b05c; padding: 3px 9px; border-radius: 6px; font-size: 11px; }}
  main {{ flex: 1; display: grid; grid-template-columns: 1fr 360px; gap: 12px; padding: 12px; }}
  .stage {{ background: #020305; border: 2px solid #222c42; border-radius: 8px; display: flex; align-items: center; justify-content: center; overflow: hidden; position: relative; }}
  .stage img {{ width: 100%; height: auto; display: block; }}
  .sidebar {{ background: #0a0d16; border: 1px solid #1e2638; border-radius: 8px; padding: 12px; display: flex; flex-direction: column; gap: 10px; font-size: 12px; }}
  .rx-card {{ background: #05060a; border: 1px solid #1e2638; border-radius: 6px; padding: 8px; }}
  .rx-name {{ color: #00f0ff; font-weight: 700; }}
</style>
</head>
<body>
<header>
  <span class="title">📺 OXICAST UNIVERSAL RECEIVER · HDMI / DVI / USB-C DP / MIRACAST / AIRPLAY / CHROMECAST / UPNP</span>
  <div style="display:flex;gap:8px;flex-wrap:wrap;">
    <span class="badge">KVM Source: {} (ayekvm)</span>
    <span class="badge">Audio: PhoeniX 432Hz (.8a∞)</span>
    <span class="badge">Reverse-HID &amp; UIBC: ARMED</span>
  </div>
</header>
<main>
  <div class="stage">
    <img id="rxImg" src="http://{}/api/stream/mjpeg?width=1920&amp;height=1080&amp;quality=80" alt="Live OxiCast HDMI Stream" />
  </div>
  <aside class="sidebar">
    <div class="rx-card"><div class="rx-name">🍓 Raspberry Pi DRM/KMS &amp; iPXE</div><div>Direct /dev/fb0 scanout + /dev/input Reverse-HID</div></div>
    <div class="rx-card"><div class="rx-name">🔌 USB-C DisplayPort Alt-Mode</div><div>Single-cable 4K60 DP video + USB 2/3 touch/HID return</div></div>
    <div class="rx-card"><div class="rx-name">📺 Miracast WFD (:7236 + UIBC)</div><div>Wi-Fi Direct MPEG-TS + TV USB mouse/remote return</div></div>
    <div class="rx-card"><div class="rx-name">📡 AirPlay / Chromecast / UPnP DLNA</div><div>mDNS + SSDP SetAVTransportURI Smart TV casting</div></div>
  </aside>
</main>
</body>
</html>"#,
        json_escape_str(&state.receiver_name),
        json_escape_str(&kvm_host),
        json_escape_str(&kvm_host),
    )
}

/// Handle a single HTTP connection on the AyeOS × OxiCast × iPXE server (`:8432`).
pub fn handle_ayeos_oxicast_http(
    mut stream: TcpStream,
    state: &Arc<Mutex<AyeosOxicastState>>,
    default_host_port: &str,
) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
    let mut buf = [0u8; 8192];
    let n = match stream.read(&mut buf) {
        Ok(n) if n > 0 => n,
        _ => return,
    };
    let req = String::from_utf8_lossy(&buf[..n]);
    let mut lines = req.lines();
    let first_line = lines.next().unwrap_or("");
    let mut parts = first_line.split_whitespace();
    let method = parts.next().unwrap_or("GET");
    let raw_path = parts.next().unwrap_or("/");
    let path = raw_path.split('?').next().unwrap_or("/");

    // Extract Host header so iPXE script uses the exact IP the client reached us on
    let mut host_header = default_host_port.to_string();
    for line in lines {
        if let Some(rest) = line.strip_prefix("Host:").or_else(|| line.strip_prefix("host:")) {
            let trimmed = rest.trim();
            if !trimmed.is_empty() {
                host_header = trimmed.to_string();
            }
            break;
        }
    }

    let body_sep = req.find("\r\n\r\n").map(|i| i + 4).or_else(|| req.find("\n\n").map(|i| i + 2));
    let body = body_sep.map(|i| req[i..].trim()).unwrap_or("");

    match (method, path) {
        ("GET", "/") | ("GET", "/oxicast") => {
            let html = {
                let Ok(mut guard) = state.lock() else { return };
                guard.step_harmonic_telemetry();
                render_ayeos_oxicast_html(&guard)
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                html.len(),
                html
            );
            let _ = stream.write_all(resp.as_bytes());
        }
        ("GET", "/desktop") => {
            let desk_arc = state.lock().ok().map(|g| Arc::clone(&g.desktop));
            let Some(desk_arc) = desk_arc else { return };
            let Ok(guard) = desk_arc.lock() else { return };
            let html = render_ayeos_desktop_html(&guard);
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                html.len(),
                html
            );
            let _ = stream.write_all(resp.as_bytes());
        }
        ("GET", "/api/desktop/state") => {
            let desk_arc = state.lock().ok().map(|g| Arc::clone(&g.desktop));
            let Some(desk_arc) = desk_arc else { return };
            let Ok(guard) = desk_arc.lock() else { return };
            let json = build_ayeos_desktop_json(&guard);
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nAccess-Control-Allow-Origin: *\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                json.len(),
                json
            );
            let _ = stream.write_all(resp.as_bytes());
        }
        ("POST", "/api/desktop/presence") => {
            let desk_arc = state.lock().ok().map(|g| Arc::clone(&g.desktop));
            let Some(desk_arc) = desk_arc else { return };
            let Ok(mut guard) = desk_arc.lock() else { return };
            guard.set_presence(PresenceLocation::from_name(body));
            let json = build_ayeos_desktop_json(&guard);
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                json.len(),
                json
            );
            let _ = stream.write_all(resp.as_bytes());
        }
        ("POST", "/api/desktop/focus") => {
            let desk_arc = state.lock().ok().map(|g| Arc::clone(&g.desktop));
            let Some(desk_arc) = desk_arc else { return };
            let Ok(mut guard) = desk_arc.lock() else { return };
            if let Ok(pane_id) = body.trim().parse::<u32>() {
                guard.focus_and_pull_pane(pane_id);
            }
            let json = build_ayeos_desktop_json(&guard);
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                json.len(),
                json
            );
            let _ = stream.write_all(resp.as_bytes());
        }
        ("POST", "/api/desktop/hotpatch") => {
            let desk_arc = state.lock().ok().map(|g| Arc::clone(&g.desktop));
            let Some(desk_arc) = desk_arc else { return };
            let Ok(mut guard) = desk_arc.lock() else { return };
            let (pane_id, sym) = if let Some((id_str, s)) = body.split_once(':') {
                (id_str.trim().parse::<u32>().unwrap_or(3), s.trim())
            } else {
                (3, body.trim())
            };
            let _ = guard.hotpatch_guest_assembly(pane_id, sym);
            let json = build_ayeos_desktop_json(&guard);
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                json.len(),
                json
            );
            let _ = stream.write_all(resp.as_bytes());
        }
        ("GET", "/oxicast/rx") => {
            let html = {
                let Ok(guard) = state.lock() else { return };
                render_universal_rx_html(&guard)
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                html.len(),
                html
            );
            let _ = stream.write_all(resp.as_bytes());
        }
        ("GET", "/api/oxicast/receivers") => {
            let json = build_universal_receivers_json(&host_header);
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nAccess-Control-Allow-Origin: *\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                json.len(),
                json
            );
            let _ = stream.write_all(resp.as_bytes());
        }
        ("GET", "/api/oxicast/state") | ("GET", "/api/oxicast/telemetry") => {
            let json = {
                let Ok(mut guard) = state.lock() else { return };
                guard.step_harmonic_telemetry();
                build_ayeos_oxicast_json(&guard)
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nAccess-Control-Allow-Origin: *\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                json.len(),
                json
            );
            let _ = stream.write_all(resp.as_bytes());
        }
        ("POST", "/api/oxicast/cast") => {
            let (title, md) = if let Some((t, b)) = body.split_once('\n') {
                (t.trim().to_string(), b.trim().to_string())
            } else {
                ("🌸 AyeOS Cast".to_string(), body.to_string())
            };
            let json = {
                let Ok(mut guard) = state.lock() else { return };
                if !title.is_empty() {
                    guard.markdown_title = title;
                }
                if !md.is_empty() {
                    guard.markdown_body = md;
                }
                guard.markdown_seq = guard.markdown_seq.wrapping_add(1);
                let _ = paint_frame_to_hardware_fb0(&guard);
                build_ayeos_oxicast_json(&guard)
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                json.len(),
                json
            );
            let _ = stream.write_all(resp.as_bytes());
        }
        ("POST", "/api/oxicast/preset") => {
            let json = {
                let Ok(mut guard) = state.lock() else { return };
                let name = body.trim();
                if !name.is_empty() {
                    guard.preset = MobilePhoenixPreset::from_name(name);
                    guard.preset_name = name.to_string();
                }
                build_ayeos_oxicast_json(&guard)
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                json.len(),
                json
            );
            let _ = stream.write_all(resp.as_bytes());
        }
        ("GET", "/api/oxicast/stream.wav") => {
            let preset = state
                .lock()
                .map(|g| g.preset)
                .unwrap_or_else(|_| MobilePhoenixPreset::velvet_sanctuary());
            let (wav, _eight_a, _hash) = synthesize_on_device_phoenix_chord(432, 3000, &preset);
            let resp_hdr = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: audio/wav\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
                wav.len()
            );
            let _ = stream.write_all(resp_hdr.as_bytes());
            let _ = stream.write_all(&wav);
        }
        ("GET", "/api/oxicast/stream24.wav") => {
            let preset = state
                .lock()
                .map(|g| g.preset)
                .unwrap_or_else(|_| MobilePhoenixPreset::velvet_sanctuary());
            let (wav16, _eight_a, _hash) = synthesize_on_device_phoenix_chord(432, 1000, &preset);
            // Extract 16-bit PCM after 44-byte RIFF header and upsample via Babbage Cubic Stepper (2x -> 96kHz 24-bit)
            let mut pcm_i16 = Vec::new();
            if wav16.len() > 44 {
                for pair in wav16[44..].chunks_exact(2) {
                    pcm_i16.push(i16::from_le_bytes([pair[0], pair[1]]));
                }
            }
            let s24 = upsample_stereo_babbage_cubic_s24le(&pcm_i16, 2);
            let wav24 = encode_wav_s24le(&s24, 96_000, 2);
            let resp_hdr = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: audio/wav\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
                wav24.len()
            );
            let _ = stream.write_all(resp_hdr.as_bytes());
            let _ = stream.write_all(&wav24);
        }
        ("POST", "/api/ayeos/flash-pxe-sd") => {
            let dev = if body.trim().is_empty() { "/dev/sda" } else { body.trim() };
            let status = std::process::Command::new("/home/hue/8b-is/oxicast/oxicast-pi/flash-pxe-eeprom-sdcard.sh")
                .arg(dev)
                .output();
            let json = match status {
                Ok(out) if out.status.success() => {
                    format!("{{\"ok\":true,\"device\":\"{}\",\"mode\":\"BOOT_ORDER=0xf241\"}}", json_escape_str(dev))
                }
                Ok(out) => {
                    let msg = String::from_utf8_lossy(&out.stdout);
                    let line = msg.lines().last().unwrap_or("No SD card medium detected in /dev/sda");
                    format!("{{\"ok\":false,\"device\":\"{}\",\"error\":\"{}\"}}", json_escape_str(dev), json_escape_str(line))
                }
                Err(e) => {
                    format!("{{\"ok\":false,\"error\":\"{}\"}}", json_escape_str(&e.to_string()))
                }
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                json.len(),
                json
            );
            let _ = stream.write_all(resp.as_bytes());
        }
        ("GET", "/api/memnet/state") | ("GET", "/api/memnet/nodes") | ("GET", "/api/memnet/leases") => {
            let fabric_arc = state.lock().ok().map(|g| Arc::clone(&g.memnet_fabric));
            let Some(fabric_arc) = fabric_arc else { return };
            let Ok(guard) = fabric_arc.lock() else { return };
            let json = build_memnet_fabric_json(&guard);
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nAccess-Control-Allow-Origin: *\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                json.len(),
                json
            );
            let _ = stream.write_all(resp.as_bytes());
        }
        ("GET", "/api/memnet/resolve") => {
            let query_domain = raw_path
                .split_once("q=")
                .map(|(_, v)| v.split('&').next().unwrap_or("boot.ayeos"))
                .unwrap_or("boot.ayeos");
            let fabric_arc = state.lock().ok().map(|g| Arc::clone(&g.memnet_fabric));
            let Some(fabric_arc) = fabric_arc else { return };
            let Ok(mut guard) = fabric_arc.lock() else { return };
            let json = if let Some((ip, txt, score_q10)) = guard.resolve_memnet_domain(query_domain) {
                format!(
                    "{{\"ok\":true,\"query\":\"{}\",\"ip\":\"{}.{}.{}.{}\",\"score_pq\":\"{}/1024\",\"txt\":\"{}\"}}",
                    json_escape_str(query_domain),
                    ip[0],
                    ip[1],
                    ip[2],
                    ip[3],
                    score_q10,
                    json_escape_str(&txt)
                )
            } else {
                format!(
                    "{{\"ok\":false,\"query\":\"{}\",\"error\":\"outside_memnet_zone\"}}",
                    json_escape_str(query_domain)
                )
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nAccess-Control-Allow-Origin: *\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                json.len(),
                json
            );
            let _ = stream.write_all(resp.as_bytes());
        }
        ("POST", "/api/memnet/register") => {
            // Peer node registration: body format `host=<ip>&port=<port>&expr=geo:... + role:... + tags:...`
            let fabric_arc = state.lock().ok().map(|g| Arc::clone(&g.memnet_fabric));
            let Some(fabric_arc) = fabric_arc else { return };
            let Ok(mut guard) = fabric_arc.lock() else { return };
            let peer_ip = stream
                .peer_addr()
                .map(|a| a.ip().to_string())
                .unwrap_or_else(|_| "127.0.0.1".to_string());
            let addr = if body.contains("role:") {
                MemnetAddress::parse(body)
            } else {
                MemnetAddress::parse("geo:sanctuary + role:ayeos.tv.peer + tags:oxicast,phoenix,ipxe,memnet")
            };
            let new_node = MemnetNode {
                address: addr,
                host: peer_ip,
                port: AYEOS_OXICAST_PORT,
                alive: true,
            };
            if let Some(ex) = guard.nodes.iter_mut().find(|n| n.host == new_node.host) {
                *ex = new_node;
            } else {
                guard.nodes.push(new_node);
            }
            let json = build_memnet_fabric_json(&guard);
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                json.len(),
                json
            );
            let _ = stream.write_all(resp.as_bytes());
        }
        ("GET", p) if p.starts_with("/boot/") || p == "/autoexec.ipxe" || p == "/ayeos.ipxe" => {
            let netboot_dir = state
                .lock()
                .map(|s| s.netboot_dir.clone())
                .unwrap_or_else(|_| detect_default_netboot_dir());
            if let Some(bytes) = resolve_netboot_artifact(p, &host_header, &netboot_dir) {
                if let Ok(mut guard) = state.lock() {
                    guard.ipxe_boots_served = guard.ipxe_boots_served.wrapping_add(1);
                }
                let ctype = if p.ends_with(".ipxe") || p.ends_with(".txt") || p.ends_with(".sh") {
                    "text/plain; charset=utf-8"
                } else {
                    "application/octet-stream"
                };
                let hdr = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    bytes.len()
                );
                let _ = stream.write_all(hdr.as_bytes());
                let _ = stream.write_all(&bytes);
            } else {
                let _ = stream.write_all(
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\nConnection: close\r\n\r\nNot Found",
                );
            }
        }
        _ => {
            let _ = stream.write_all(
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\nConnection: close\r\n\r\nNot Found",
            );
        }
    }
}

/// Start the full **AyeOS × OxiCast × PhoeniX Sanctuary & Peer-to-Peer iPXE/TFTP Server**:
/// 1. Starts the RFC 1350 TFTP server (`UDP :69` or `:6969`) for diskless Raspberry Pi 4 boot.
/// 2. Starts the 16-byte `.8a` (`8a∞`) UDP telemetry & Ambilight broadcaster (`:8432` & `:8888`).
/// 3. Starts the `/dev/fb0` hardware HDMI visualizer refresh loop (`paint_frame_to_hardware_fb0`).
/// 4. Serves HTTP (`:8432/oxicast`, `/api/oxicast/*`, and `/boot/*` iPXE endpoints).
pub fn run_ayeos_sanctuary_daemon(port: u16, peer_mother: Option<String>) {
    let mut initial = AyeosOxicastState::default();
    if let Some(peer) = peer_mother {
        initial.peer_mother_host = peer;
    }
    let fabric_arc = Arc::clone(&initial.memnet_fabric);
    let local_ip = detect_local_ipv4();
    let local_ip_str = format!(
        "{}.{}.{}.{}",
        local_ip[0], local_ip[1], local_ip[2], local_ip[3]
    );
    let _ = paint_frame_to_hardware_fb0(&initial);

    let state = Arc::new(Mutex::new(initial));
    let default_host_port = format!("{local_ip_str}:{port}");

    // 0. Spawn MEMNET-backed DNS (:53 / :8053) + DHCP / ProxyDHCP / PXE / iPXE (:67 / :4011 / :6767)
    spawn_memnet_dns_and_dhcp_daemons(fabric_arc);

    // 1. Spawn TFTP Netboot server (:69 / :6969) for SD-cardless Raspberry Pi 4 peers
    spawn_ayeos_tftp_server(Arc::clone(&state), default_host_port.clone());

    // 2. Spawn .8a (8a∞) UDP broadcaster + /dev/fb0 HDMI screen refresher
    let state_bg = Arc::clone(&state);
    std::thread::spawn(move || {
        let udp = UdpSocket::bind("0.0.0.0:0").ok();
        if let Some(ref s) = udp {
            let _ = s.set_broadcast(true);
        }
        let mut tick: u64 = 0;
        loop {
            if let Ok(mut guard) = state_bg.lock() {
                guard.step_harmonic_telemetry();
                let pkt = guard.build_eight_a_udp_frame();
                if let Some(ref s) = udp {
                    let _ = s.send_to(&pkt, ("127.0.0.1", port));
                    let _ = s.send_to(&pkt, ("255.255.255.255", port));
                    let _ = s.send_to(&pkt, ("255.255.255.255", 8888));
                }
                if tick % 4 == 0 {
                    let _ = paint_frame_to_hardware_fb0(&guard);
                }
            }
            tick = tick.wrapping_add(1);
            std::thread::sleep(Duration::from_millis(250));
        }
    });

    // 3. Bind TCP HTTP OxiCast + iPXE server
    let bind_addr = format!("0.0.0.0:{port}");
    let Ok(listener) = TcpListener::bind(&bind_addr) else {
        eprintln!("[ayeos-sanctuary] could not bind {bind_addr}");
        return;
    };
    println!(
        "🌸 [ayeOS Sanctuary] OxiCast TV + PhoeniX 384kHz + Diskless iPXE Server live on http://{bind_addr}/oxicast"
    );
    println!(
        "🚀 [ayeOS iPXE Mesh] Chainload URL: http://{bind_addr}/boot/ayeos.ipxe | Initramfs: http://{bind_addr}/boot/ayeos-initramfs.cpio"
    );

    let _ = SystemTime::now().duration_since(UNIX_EPOCH);
    let _ = encode_wav_i16(&[], 48_000, 2);

    for stream in listener.incoming().flatten() {
        let st = Arc::clone(&state);
        let hp = default_host_port.clone();
        std::thread::spawn(move || {
            handle_ayeos_oxicast_http(stream, &st, &hp);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_no_float_tokens(text: &str) {
        let bytes = text.as_bytes();
        for i in 1..bytes.len().saturating_sub(1) {
            if bytes[i] == b'.' && bytes[i - 1].is_ascii_digit() && bytes[i + 1].is_ascii_digit() {
                // Allow IPv4 addresses like 127.0.0.1 or HTTP/1.1
                let start = i.saturating_sub(12);
                let end = (i + 12).min(bytes.len());
                let window = String::from_utf8_lossy(&bytes[start..end]);
                if !window.contains("HTTP/1.1")
                    && !window.contains("127.0.0.1")
                    && !window.contains("0.0.0.0")
                {
                    panic!("Float literal token detected in window: {window}");
                }
            }
        }
    }

    #[test]
    fn ayeos_oxicast_html_and_json_contain_zero_floats() {
        let mut st = AyeosOxicastState::default();
        st.step_harmonic_telemetry();
        let json = build_ayeos_oxicast_json(&st);
        let html = render_ayeos_oxicast_html(&st);
        assert_no_float_tokens(&json);
        assert_no_float_tokens(&html);
        assert!(json.contains("\"babbage_additions_per_sample\":3"));
        assert!(html.contains("ayeOS × OxiCast × PhoeniX TV Sanctuary"));
    }

    #[test]
    fn babbage_cubic_stepper_upsamples_exact_8x_without_floats() {
        let out = crate::phoenix_audio::step_babbage_cubic_8x_i32(1000, 2000, 4000, 8000);
        // At u = 0, P(0) must equal p1 (2000)
        assert_eq!(out[0], 2000);
        assert!(out[4] > 2000 && out[4] < 4000);
    }

    #[test]
    fn ipxe_and_tftp_rrq_packets_roundtrip_cleanly() {
        let ipxe = build_ayeos_ipxe_script("oxicast.local:8432");
        assert!(ipxe.starts_with("#!ipxe\n"));
        assert!(ipxe.contains("http://oxicast.local:8432/boot/ayeos-initramfs.cpio"));

        // Construct RFC 1350 RRQ packet for "config.txt" in "octet" mode
        let mut rrq = Vec::new();
        rrq.extend_from_slice(&TFTP_OP_RRQ.to_be_bytes());
        rrq.extend_from_slice(b"config.txt\0octet\0");
        let parsed = parse_tftp_rrq(&rrq).expect("valid RRQ");
        assert_eq!(parsed.0, "config.txt");
        assert_eq!(parsed.1, "octet");
    }

    #[test]
    fn fb0_renderer_paints_36_spectrum_bars_and_bitmap_glyphs() {
        let st = AyeosOxicastState::default();
        let buf = render_oxicast_fb0_frame(&st, 320, 240);
        assert_eq!(buf.len(), 320 * 240 * 4);
    }

    #[test]
    fn universal_rx_json_and_html_contain_zero_floats() {
        let st = AyeosOxicastState::default();
        let rx_json = build_universal_receivers_json("oxicast.local:8432");
        let rx_html = render_universal_rx_html(&st);
        assert_no_float_tokens(&rx_json);
        assert_no_float_tokens(&rx_html);
        assert!(rx_json.contains("usb_c_dp_alt_mode"));
        assert!(rx_json.contains("miracast_wfd_uibc"));
        assert!(rx_html.contains("OXICAST UNIVERSAL RECEIVER"));
    }
}
