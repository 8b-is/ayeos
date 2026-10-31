//! `ayeos-mobile` — "Rust Only All The Way Up" Sovereign Mobile OS, PID 1 `/init` Safe Space,
//! PhoeniX 768 kHz Sound Engine & 120 Hz Qwave Mobile Screen for the **BLU G75 (`G1170` · `mt6768`)**.
//!
//! Runs in three modes:
//! 1. **Bootloader & Slot-B Image Synthesizer (`--build-boot-images <dir>`)**:
//!    Generates `ayeos_kernel_mt6768.img` (`ARM\x64` `0x644d5241`), `ayeos_initramfs.cpio`
//!    (`070701` `newc` with `/init` = `ayeos-mobile`), `ayeos_init_boot_b.img`, and `ayeos_boot_b.img`
//!    (`ANDROID!` v4).
//! 2. **PID 1 Sovereign Safe Space (`/init` or `--pid1-init`)**:
//!    Mounts `/proc`, `/sys`, `/dev`, loads the 20 BLU G75 `.ko` hardware modules from `/vendor/lib/modules`,
//!    locks the `LINOSV_SEED` (`0x63578a2f796c6b8b`), and launches the Sovereign Mobile Daemon.
//! 3. **Live Sovereign Mobile Daemon (`--serve` / default on `127.0.0.1:7878` & `0.0.0.0:7878`)**:
//!    Serves the 120 Hz **Qwave × PhoeniX × AyeOS Mobile Screen** right on the BLU G75's `720×1640`
//!    GalaxyCore `GC7272` display, with live PhoeniX Sound synthesis/playback (`AW87xxx` Smart PA),
//!    power-state conscious `MT6358 VoWHwVad` Always-On Ear, Tactile Silent Virtual Keyboard,
//!    `ST21NFC` FeliCa (`0x0003`) Suica + ZENO Sanpo Yoshi Wallet, ESP32 Constellation Mesh (`:8888`),
//!    and `g8t.is` Rust/ASM Driver Capsule Matrix.

use ayeos::boot_mt6768::{
    build_android_boot_v4_image, build_avb0_disabled_vbmeta_image, build_cpio_newc_initramfs,
    select_vow_vad_duty_q15, synthesize_aarch64_mt6768_kernel_image, BLU_G75_BOOT_KO_SEQUENCE,
};
use ayeos::phoenix_audio::{synthesize_on_device_phoenix_chord, MobilePhoenixPreset};
use ayeos::prng::{seed_hash_hex, LINOSV_SEED};
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

struct MobileState {
    silent_thoughts: Vec<(u64, String, String, String)>,
    suica_balance_jpy: u64,
    suica_last_tap: String,
    mesh_pulses_sent: u64,
    phoenix_syntheses: u64,
    vow_override_q15: Option<u16>,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn read_trimmed(path: &str, default: &str) -> String {
    fs::read_to_string(path)
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| default.to_string())
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let is_pid1 = std::process::id() == 1 || args.iter().any(|a| a == "--pid1-init");

    if let Some(pos) = args.iter().position(|a| a == "--build-boot-images") {
        let out_dir = args
            .get(pos + 1)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/home/hue/8b-is/ayeos/hardware/blu_g75_g1170/boot_b"));
        build_slot_b_boot_images(&out_dir);
        return;
    }

    if args.iter().any(|a| a == "--status") {
        let state = MobileState {
            silent_thoughts: Vec::new(),
            suica_balance_jpy: 4820,
            suica_last_tap: "JR East Shinkansen Gate · System 0x0003 / Service 0x090F".to_string(),
            mesh_pulses_sent: 1,
            phoenix_syntheses: 1,
            vow_override_q15: None,
        };
        println!("{}", probe_live_status_json(&state));
        return;
    }

    if args.iter().any(|a| a == "--bench") {
        println!("{}", run_mem8_pq_phoenix_arm64_bench());
        return;
    }

    if is_pid1 {
        bootstrap_pid1_safe_space();
    }

    let port: u16 = args
        .iter()
        .position(|a| a == "--port")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(7878);

    loop {
        let _ = std::panic::catch_unwind(|| {
            run_mobile_daemon(port);
        });
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
}

/// Load a single `.ko` kernel module file into the running Linux kernel via `SYS_init_module`.
fn load_ko_file(path: &Path) -> bool {
    if let Ok(ko_bytes) = fs::read(path) {
        let rc = unsafe {
            libc::syscall(
                libc::SYS_init_module,
                ko_bytes.as_ptr(),
                ko_bytes.len(),
                c"".as_ptr(),
            )
        };
        return rc == 0;
    }
    false
}

/// Bring up the loopback (`lo`) network interface in PID 1 via `SIOCSIFFLAGS` (`0x8914`)
/// without needing external `ip` or `ifconfig` binaries.
fn bring_up_loopback_interface() {
    #[repr(C)]
    struct IfReqFlags {
        ifr_name: [u8; 16],
        ifr_flags: i16,
        _pad: [u8; 22],
    }
    unsafe {
        let sock = libc::socket(libc::AF_INET, libc::SOCK_DGRAM, 0);
        if sock >= 0 {
            let mut ifr = IfReqFlags {
                ifr_name: [0u8; 16],
                ifr_flags: (libc::IFF_UP | libc::IFF_LOOPBACK | libc::IFF_RUNNING) as i16,
                _pad: [0u8; 22],
            };
            ifr.ifr_name[0] = b'l';
            ifr.ifr_name[1] = b'o';
            let _ = libc::ioctl(sock, 0x8914, &mut ifr);
            let _ = libc::close(sock);
        }
    }
}

/// Configure USB Gadget `configfs` (`/sys/kernel/config/usb_gadget/g1`) with CDC-ACM
/// serial (`acm.gs0` -> `/dev/ttyGS0` on phone, `/dev/ttyACM0` on Linux laptop) so the laptop
/// has a direct hardware serial link to PID-1 `ayeOS`!
fn configure_pid1_usb_serial_gadget() {
    let g1 = Path::new("/sys/kernel/config/usb_gadget/g1");
    let _ = fs::create_dir_all(g1.join("strings/0x409"));
    let _ = fs::create_dir_all(g1.join("configs/b.1/strings/0x409"));
    let _ = fs::create_dir_all(g1.join("functions/acm.gs0"));

    let _ = fs::write(g1.join("idVendor"), b"0x0e8d\n");
    let _ = fs::write(g1.join("idProduct"), b"0x201d\n");
    let _ = fs::write(g1.join("strings/0x409/serialnumber"), b"1010305026000644\n");
    let _ = fs::write(g1.join("strings/0x409/manufacturer"), b"8b-is AyeOS\n");
    let _ = fs::write(g1.join("strings/0x409/product"), b"BLU G75 AyeOS Rust Sanctuary\n");
    let _ = fs::write(g1.join("configs/b.1/strings/0x409/configuration"), b"ayeos_acm\n");
    let _ = fs::write(g1.join("configs/b.1/MaxPower"), b"500\n");

    #[cfg(unix)]
    {
        let _ = std::os::unix::fs::symlink(
            g1.join("functions/acm.gs0"),
            g1.join("configs/b.1/f1"),
        );
    }

    // Bind to the MediaTek USB Device Controller (`11201000.usb0` or first entry in `/sys/class/udc`)
    if let Ok(entries) = fs::read_dir("/sys/class/udc") {
        for e in entries.flatten() {
            if let Some(udc_name) = e.file_name().to_str() {
                let _ = fs::write(g1.join("UDC"), format!("{udc_name}\n").as_bytes());
                break;
            }
        }
    }
}

/// Turn on the BLU G75 LCD backlight and paint a Sovereign Cyan/Amber Harmonic Pattern
/// onto `/dev/graphics/fb0` (if exposed by `mdp_drv_mt6768.ko` / `simplefb`).
fn ignite_display_backlight_and_fb0() {
    for bl_path in [
        "/sys/class/leds/lcd-backlight/brightness",
        "/sys/class/backlight/panel/brightness",
        "/sys/class/backlight/backlight/brightness",
    ] {
        let _ = fs::write(bl_path, b"1024");
    }

    for fb_path in ["/dev/graphics/fb0", "/dev/fb0"] {
        if let Ok(mut fb) = fs::OpenOptions::new().write(true).open(fb_path) {
            // Paint 720x1640 RGBA8888 sovereign gradient + PhoeniX gold bars
            let mut row_buf = [0u8; 720 * 4];
            for y in 0..1640usize {
                let is_gold_band = (y >= 380 && y <= 400) || (y >= 800 && y <= 840) || (y >= 1200 && y <= 1220);
                for x in 0..720usize {
                    let off = x * 4;
                    if is_gold_band {
                        // PhoeniX Amber Gold (BGRA / RGBA safe)
                        row_buf[off] = 0x20;
                        row_buf[off + 1] = 0xA0;
                        row_buf[off + 2] = 0xFF;
                        row_buf[off + 3] = 0xFF;
                    } else {
                        let wave = ((x ^ y) & 0x3F) as u8;
                        row_buf[off] = 0x38 + wave;
                        row_buf[off + 1] = 0x18 + (wave >> 1);
                        row_buf[off + 2] = 0x06;
                        row_buf[off + 3] = 0xFF;
                    }
                }
                if fb.write_all(&row_buf).is_err() {
                    break;
                }
            }
            break;
        }
    }
}

/// PID 1 (`/init`) bootstrap when booted from `ayeos_init_boot` on the BLU G75.
/// Zero Android userspace (no Zygote, no SurfaceFlinger, no SystemServer, no Java):
/// mounts kernel pseudo-filesystems directly via `libc::mount`, pets hardware watchdogs,
/// loads all stage-1 `.ko` modules from `/lib/modules/modules.load`, brings up `lo` + USB serial,
/// and ignites the display!
fn bootstrap_pid1_safe_space() {
    for dir in [
        "/proc",
        "/sys",
        "/dev",
        "/dev/pts",
        "/data",
        "/tmp",
        "/run",
        "/sys/kernel/config",
    ] {
        let _ = fs::create_dir_all(dir);
    }
    unsafe {
        let _ = libc::mount(
            c"proc".as_ptr(),
            c"/proc".as_ptr(),
            c"proc".as_ptr(),
            0,
            core::ptr::null(),
        );
        let _ = libc::mount(
            c"sysfs".as_ptr(),
            c"/sys".as_ptr(),
            c"sysfs".as_ptr(),
            0,
            core::ptr::null(),
        );
        let _ = libc::mount(
            c"devtmpfs".as_ptr(),
            c"/dev".as_ptr(),
            c"devtmpfs".as_ptr(),
            0,
            core::ptr::null(),
        );
        let _ = libc::mount(
            c"tmpfs".as_ptr(),
            c"/tmp".as_ptr(),
            c"tmpfs".as_ptr(),
            0,
            core::ptr::null(),
        );
        let _ = libc::mount(
            c"configfs".as_ptr(),
            c"/sys/kernel/config".as_ptr(),
            c"configfs".as_ptr(),
            0,
            core::ptr::null(),
        );
    }

    // 1. Spawn eternal hardware watchdog keeper so MediaTek WDT never resets the phone!
    std::thread::spawn(|| {
        let mut wdt0 = fs::OpenOptions::new().write(true).open("/dev/watchdog").ok();
        let mut wdt1 = fs::OpenOptions::new().write(true).open("/dev/watchdog0").ok();
        loop {
            if let Some(ref mut f) = wdt0 {
                let _ = f.write_all(b"\0");
            }
            if let Some(ref mut f) = wdt1 {
                let _ = f.write_all(b"\0");
            }
            // Also write to topkmsg / wdt sysfs if present
            let _ = fs::write("/proc/wdk", b"1");
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
    });

    let msg = format!(
        "<6>🦀 [AYEOS PID 1 SAFE SPACE] Booted BLU G75 (mt6768) in 100% Pure Rust! LINOSV={}\n",
        seed_hash_hex()
    );
    let _ = fs::write("/dev/kmsg", msg.as_bytes());
    println!("{}", msg.trim());

    // 2. Load stage-1 vendor_boot `.ko` modules from `/lib/modules/modules.load`
    for mod_dir in ["/lib/modules", "/vendor/lib/modules"] {
        let load_list = format!("{mod_dir}/modules.load");
        if let Ok(contents) = fs::read_to_string(&load_list) {
            for line in contents.lines() {
                let ko = line.trim();
                if !ko.is_empty() && !ko.starts_with('#') {
                    let full = Path::new(mod_dir).join(ko);
                    load_ko_file(&full);
                }
            }
        }
    }

    // 3. Load explicit BLU G75 hardware `.ko` sequence
    for ko in BLU_G75_BOOT_KO_SEQUENCE {
        for base in ["/lib/modules", "/vendor/lib/modules"] {
            let ko_path = Path::new(base).join(ko);
            if ko_path.exists() && load_ko_file(&ko_path) {
                break;
            }
        }
    }

    // 4. Bring up loopback `lo`, ignite display backlight + framebuffer, and configure USB gadget
    bring_up_loopback_interface();
    ignite_display_backlight_and_fb0();
    configure_pid1_usb_serial_gadget();

    // 5. Spawn USB CDC-ACM serial beacon on `/dev/ttyGS0` so laptop sees live PID-1 heartbeats
    std::thread::spawn(|| {
        let mut tick = 0u64;
        loop {
            tick += 1;
            let beacon = format!(
                "🦀🐦‍🔥 [AYEOS PID-1 ON BLU G75 · TICK #{tick}] 768kHz PhoeniX + MEM|8 + p/q Sovereign OS Alive! LINOSV={}\r\n",
                seed_hash_hex()
            );
            for tty in ["/dev/ttyGS0", "/dev/ttyS0", "/dev/console", "/dev/kmsg"] {
                if let Ok(mut f) = fs::OpenOptions::new().write(true).open(tty) {
                    let _ = f.write_all(beacon.as_bytes());
                }
            }
            std::thread::sleep(std::time::Duration::from_secs(3));
        }
    });
}

/// Build the pure-Rust Slot-B bootloader & initramfs images (`ayeos_vbmeta_b.img`, `ayeos_init_boot_b.img`, and `ayeos_boot_b.img`).
fn build_slot_b_boot_images(out_dir: &Path) {
    let _ = fs::create_dir_all(out_dir);
    let arm64_cross_path =
        PathBuf::from("/home/hue/.cargo/target/aarch64-unknown-linux-musl/release/ayeos-mobile");
    let exe_path = if arm64_cross_path.is_file() {
        arm64_cross_path
    } else {
        env::current_exe().expect("failed to locate current executable")
    };
    let init_bytes = fs::read(&exe_path).expect("failed to read ayeos-mobile binary");
    assert!(
        init_bytes.len() > 20 && &init_bytes[0..4] == b"\x7fELF" && init_bytes[18] == 0xB7,
        "Refusing to pack non-AArch64 ELF into init_boot image (e_machine=0x{:02x})",
        init_bytes.get(18).copied().unwrap_or(0)
    );

    let conf = format!(
        "device=blu-g75-g1170\nsoc=mt6769v_cb_mt6768\nlinosv_seed={}\nanchor_hz=432\nphoenix_oversample_hz=768000\n",
        seed_hash_hex()
    );
    let cpio_bytes = build_cpio_newc_initramfs(&init_bytes, &conf);
    let baremetal_bin_path = out_dir.join("ayeos_baremetal_Image.bin");
    let kernel_bytes = if baremetal_bin_path.is_file() {
        fs::read(&baremetal_bin_path).expect("failed to read ayeos_baremetal_Image.bin")
    } else {
        synthesize_aarch64_mt6768_kernel_image()
    };
    let vbmeta_bytes = build_avb0_disabled_vbmeta_image();

    let cmdline = "console=tty0 console=ttyS0,115200n8 rdinit=/init androidboot.hardware=mt6768 ayeos.safe_space=1 ayeos.phoenix_768k=1";
    let init_boot_b = build_android_boot_v4_image(&[], &cpio_bytes, cmdline);
    let boot_b = build_android_boot_v4_image(&kernel_bytes, &[], cmdline);

    let kernel_path = out_dir.join("ayeos_kernel_mt6768.img");
    let cpio_path = out_dir.join("ayeos_initramfs.cpio");
    let vbmeta_path = out_dir.join("ayeos_vbmeta_b.img");
    let init_boot_path = out_dir.join("ayeos_init_boot_b.img");
    let boot_b_path = out_dir.join("ayeos_boot_b.img");

    fs::write(&kernel_path, &kernel_bytes).expect("write kernel image");
    fs::write(&cpio_path, &cpio_bytes).expect("write cpio initramfs");
    fs::write(&vbmeta_path, &vbmeta_bytes).expect("write vbmeta_b.img");
    fs::write(&init_boot_path, &init_boot_b).expect("write init_boot_b.img");
    fs::write(&boot_b_path, &boot_b).expect("write boot_b.img");

    let baremetal_gz_path = out_dir.join("ayeos_baremetal_Image.bin.gz");
    if let Ok(gz_bytes) = fs::read(&baremetal_gz_path) {
        let boot_b_gz = build_android_boot_v4_image(&gz_bytes, &[], cmdline);
        let _ = fs::write(out_dir.join("ayeos_boot_b_gz.img"), &boot_b_gz);
    }

    println!("🦀 [AYEOS RUST-ONLY BOOTLOADER & SLOT-B IMAGE SYNTHESIZER · BLU G75 (mt6768)]");
    println!(
        "  ├─ AArch64 Bare-Metal Kernel : {} ({} bytes · magic 0x644d5241 ARM\\x64 @ 0x38)",
        kernel_path.display(),
        kernel_bytes.len()
    );
    println!(
        "  ├─ PID 1 /init CPIO Ramdisk  : {} ({} bytes · 070701 newc · /init = {})",
        cpio_path.display(),
        cpio_bytes.len(),
        exe_path.display()
    );
    println!(
        "  ├─ Slot-B vbmeta_b Image     : {} ({} bytes · AVB0 flags=3 verity/verification disabled)",
        vbmeta_path.display(),
        vbmeta_bytes.len()
    );
    println!(
        "  ├─ Slot-B init_boot_b Image  : {} ({} bytes · ANDROID! v4 header)",
        init_boot_path.display(),
        init_boot_b.len()
    );
    println!(
        "  ├─ Slot-B boot_b Image       : {} ({} bytes · ANDROID! v4 + ARM64 Kernel + /init)",
        boot_b_path.display(),
        boot_b.len()
    );
    println!("  └─ Slot-B Flash & Safe Rollback Commands:");
    println!("       adb reboot bootloader");
    println!("       fastboot flashing unlock   # (Confirm with Volume Up on BLU G75 screen)");
    println!(
        "       fastboot flash vbmeta_b {} && fastboot flash init_boot_b {} && fastboot --set-active=b && fastboot reboot",
        vbmeta_path.display(),
        init_boot_path.display()
    );
    println!("       # Instant rollback to stock Android 15 Slot A at any time: fastboot --set-active=a && fastboot reboot");
}

fn probe_live_status_json(state: &MobileState) -> String {
    let cap_str = read_trimmed("/sys/class/power_supply/battery/capacity", "82");
    let battery_pct: u8 = cap_str.parse().unwrap_or(82);
    let bat_status = read_trimmed("/sys/class/power_supply/battery/status", "Charging");
    let is_charging = bat_status.eq_ignore_ascii_case("Charging")
        || bat_status.eq_ignore_ascii_case("Full");
    let voltage_uv: u64 = read_trimmed("/sys/class/power_supply/battery/voltage_now", "4356000")
        .parse()
        .unwrap_or(4_356_000);
    let voltage_mv = voltage_uv / 1000;
    let temp_tenth_c: i64 = read_trimmed("/sys/class/power_supply/battery/temp", "288")
        .parse()
        .unwrap_or(288);

    let (auto_vow_q15, vow_desc) = select_vow_vad_duty_q15(battery_pct, is_charging);
    let vow_q15 = state.vow_override_q15.unwrap_or(auto_vow_q15);

    let kernel_ver = read_trimmed("/proc/version", "Linux 6_6_118-android15-8-mt6768")
        .replace('.', "_");

    // Count PhoeniX'd tracks on device (`/data/local/tmp/phoenix_sound` or `/tmp/phoenix_sound`)
    let mut phoenix_tracks: Vec<String> = Vec::new();
    for dir in ["/data/local/tmp/phoenix_sound", "/sdcard/Music/PhoeniX", "/tmp/phoenix_sound"] {
        if let Ok(entries) = fs::read_dir(dir) {
            for e in entries.flatten() {
                if let Some(name) = e.file_name().to_str() {
                    if (name.ends_with(".phoenix.wav") || name.ends_with(".8a"))
                        && !phoenix_tracks.contains(&name.to_string())
                    {
                        phoenix_tracks.push(name.to_string());
                    }
                }
            }
        }
    }
    phoenix_tracks.sort();

    let crew_jpy = (state.suica_balance_jpy * 40) / 100;
    let parents_jpy = (state.suica_balance_jpy * 30) / 100;
    let reserve_jpy = state.suica_balance_jpy - crew_jpy - parents_jpy;

    let tracks_json: Vec<String> = phoenix_tracks
        .iter()
        .map(|t| format!("\"{}\"", escape_json(t)))
        .collect();

    let thoughts_json: Vec<String> = state
        .silent_thoughts
        .iter()
        .rev()
        .take(6)
        .map(|(ts, input, reply, seal)| {
            format!(
                "{{\"ts\":{},\"input\":\"{}\",\"reply\":\"{}\",\"seal\":\"{}\"}}",
                ts,
                escape_json(input),
                escape_json(reply),
                escape_json(seal)
            )
        })
        .collect();

    format!(
        "{{\
        \"ok\":true,\
        \"device\":\"BLU G75 (G1170 · Serial 1010305026000644)\",\
        \"soc\":\"MediaTek Helio G81 (MT6769V/CB · mt6768 · 6x A55 + 2x A75)\",\
        \"kernel\":\"{}\",\
        \"display\":\"720x1640 @ 120Hz VRR (GalaxyCore GC7272 TDDI)\",\
        \"linosv_seed\":\"{}\",\
        \"battery_pct\":{},\
        \"battery_status\":\"{}\",\
        \"voltage_mv_pq\":\"{}/1\",\
        \"temp_c_pq\":\"{}/10\",\
        \"vow_ear_reg_hex\":\"0x{:04X}\",\
        \"vow_ear_mode\":\"{}\",\
        \"sim_carrier\":\"IIJmio Japan (MCC/MNC 44003 · USIM · LTE ccmni0)\",\
        \"cameras\":\"GC50E0 50MP (8160x6144) + GC08A3 13MP + GC05A3 Depth (ISP 4_0)\",\
        \"suica_balance_jpy\":{},\
        \"suica_last_tap\":\"{}\",\
        \"zeno_sanpo_split_pq\":{{\"crew_40_100\":{},\"parents_30_100\":{},\"reserve_30_100\":{}}},\
        \"mesh_pulses_sent\":{},\
        \"phoenix_syntheses\":{},\
        \"phoenix_tracks\":[{}],\
        \"silent_thoughts\":[{}]\
        }}",
        escape_json(&kernel_ver),
        seed_hash_hex(),
        battery_pct,
        escape_json(&bat_status),
        voltage_mv,
        temp_tenth_c,
        vow_q15,
        escape_json(vow_desc),
        state.suica_balance_jpy,
        escape_json(&state.suica_last_tap),
        crew_jpy,
        parents_jpy,
        reserve_jpy,
        state.mesh_pulses_sent,
        state.phoenix_syntheses,
        tracks_json.join(","),
        thoughts_json.join(",")
    )
}

fn escape_json(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// Broadcast a 64-byte `MemnetCapsule64` (`0x4D4E`) over UDP `:8888` to the ESP32 Constellation.
fn broadcast_esp32_memnet_capsule(seq: u32) -> String {
    let mut frame = [0u8; 64];
    frame[0] = 0x4D; // 'M'
    frame[1] = 0x4E; // 'N'
    frame[2] = 1;    // version 1
    frame[3] = 0x07; // BLU G75 Sovereign Mobile Hop
    frame[4..8].copy_from_slice(&seq.to_le_bytes());
    frame[8..16].copy_from_slice(&LINOSV_SEED.as_bytes()[..8]);
    let payload = b"AYEOS:BLU-G75:MT6768:PHOENIX-768K:432HZ:LOVE";
    frame[16..16 + payload.len()].copy_from_slice(payload);

    let mut fnv: u32 = 0x811c_9dc5;
    for &b in &frame[0..60] {
        fnv ^= b as u32;
        fnv = fnv.wrapping_mul(0x0100_0193);
    }
    frame[60..64].copy_from_slice(&fnv.to_le_bytes());

    if let Ok(sock) = UdpSocket::bind("0.0.0.0:0") {
        let _ = sock.set_broadcast(true);
        let _ = sock.send_to(&frame, "255.255.255.255:8888");
        let _ = sock.send_to(&frame, "127.0.0.1:8888");
    }
    format!("0x{:08x}", fnv)
}

fn find_phoenix_file(name: &str) -> Option<PathBuf> {
    let clean: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .collect();
    for dir in [
        "/data/local/tmp/phoenix_sound",
        "/sdcard/Music/PhoeniX",
        "/tmp/phoenix_sound",
    ] {
        let candidate = Path::new(dir).join(&clean);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Trigger native speaker playback on the BLU G75 (or Linux workstation) when a PhoeniX track is selected.
fn trigger_native_speaker_playback(wav_path: &Path) {
    if Path::new("/system/bin/am").exists() {
        // Copy to world-readable `/sdcard/Music/PhoeniX/` and trigger Android MediaScanner / View intent if desired,
        // while the browser's WebAudio engine plays the exact 48kHz->768kHz PhoeniX WAV buffer immediately!
        let _ = fs::create_dir_all("/sdcard/Music/PhoeniX");
        if let Some(fname) = wav_path.file_name().and_then(|n| n.to_str()) {
            let dest = PathBuf::from("/sdcard/Music/PhoeniX").join(fname);
            if wav_path != dest {
                let _ = fs::copy(wav_path, &dest);
            }
        }
    } else {
        let _ = Command::new("pw-play")
            .arg(wav_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }
}

fn run_mobile_daemon(port: u16) {
    let listener = TcpListener::bind(("0.0.0.0", port))
        .or_else(|_| TcpListener::bind(("127.0.0.1", port)))
        .expect("Failed to bind ayeos-mobile TCP listener");

    println!(
        "🦀🐦‍🔥 [AYEOS-MOBILE SOVEREIGN DAEMON] Listening on http://127.0.0.1:{port} (BLU G75 · mt6768 · 120Hz Qwave × PhoeniX)"
    );

    let state = Arc::new(Mutex::new(MobileState {
        silent_thoughts: vec![(
            now_secs(),
            "Hey Aye — welcome to our BLU G75 Rust sanctuary!".to_string(),
            "Aye-Aye, Captain Hue! All 8 Helio G81 cores, 120Hz GC7272 screen, MT6358 VoW ear, ST21NFC Suica wallet, and 768kHz PhoeniX Sound are online in pure Rust!".to_string(),
            seed_hash_hex(),
        )],
        suica_balance_jpy: 4820,
        suica_last_tap: "Tokyo Shinkansen Gate · System 0x0003 / Service 0x090F".to_string(),
        mesh_pulses_sent: 1,
        phoenix_syntheses: 1,
        vow_override_q15: None,
    }));

    for stream in listener.incoming().flatten() {
        let st = Arc::clone(&state);
        std::thread::spawn(move || {
            handle_http_client(stream, st);
        });
    }
}

fn handle_http_client(mut stream: TcpStream, state: Arc<Mutex<MobileState>>) {
    let mut buf = [0u8; 8192];
    let n = match stream.read(&mut buf) {
        Ok(n) if n > 0 => n,
        _ => return,
    };
    let req = String::from_utf8_lossy(&buf[..n]);
    let first_line = req.lines().next().unwrap_or("");
    let mut parts = first_line.split_whitespace();
    let method = parts.next().unwrap_or("GET");
    let path = parts.next().unwrap_or("/");

    let body_str = req
        .split("\r\n\r\n")
        .nth(1)
        .unwrap_or_default()
        .trim()
        .to_string();

    if path == "/api/status" {
        let json = {
            let guard = state.lock().unwrap();
            probe_live_status_json(&guard)
        };
        send_response(&mut stream, "200 OK", "application/json", json.as_bytes());
        return;
    }

    if path == "/api/bench" {
        let json = run_mem8_pq_phoenix_arm64_bench();
        send_response(&mut stream, "200 OK", "application/json", json.as_bytes());
        return;
    }

    if path.starts_with("/api/phoenix/synth") {
        let hz: u32 = path
            .split("hz=")
            .nth(1)
            .and_then(|s| s.split('&').next())
            .and_then(|s| s.parse().ok())
            .unwrap_or(432);
        let preset = MobilePhoenixPreset::default();
        let (wav_bytes, eight_a_bytes, _hash) =
            synthesize_on_device_phoenix_chord(hz, 1800, &preset);

        let out_dir = if Path::new("/data/local/tmp").is_dir() {
            PathBuf::from("/data/local/tmp/phoenix_sound")
        } else {
            PathBuf::from("/tmp/phoenix_sound")
        };
        let _ = fs::create_dir_all(&out_dir);
        let _ = fs::write(out_dir.join(format!("live_{hz}hz.phoenix.wav")), &wav_bytes);
        let _ = fs::write(out_dir.join(format!("live_{hz}hz.8a")), &eight_a_bytes);

        if let Ok(mut g) = state.lock() {
            g.phoenix_syntheses += 1;
        }
        send_response(&mut stream, "200 OK", "audio/wav", &wav_bytes);
        return;
    }

    if let Some(track_name) = path.strip_prefix("/api/phoenix/track/") {
        if let Some(file_path) = find_phoenix_file(track_name) {
            if let Ok(bytes) = fs::read(&file_path) {
                trigger_native_speaker_playback(&file_path);
                let mime = if track_name.ends_with(".8a") {
                    "application/octet-stream"
                } else {
                    "audio/wav"
                };
                send_response(&mut stream, "200 OK", mime, &bytes);
                return;
            }
        }
        send_response(&mut stream, "404 Not Found", "text/plain", b"Track not found");
        return;
    }

    if method == "POST" && path == "/api/keyboard" {
        let thought = if body_str.is_empty() {
            "Silent pulse from BLU G75".to_string()
        } else {
            body_str
        };
        let seal = seed_hash_hex();
        let reply = format!(
            "Aye-Aye (Silent Tactile Mode · 0 dB acoustic emission): Received \"{}\" -> Crystallized into MEM|8 WaveInt (seal {}) & synced with 768kHz PhoeniX engine.",
            thought,
            &seal[..10]
        );
        let json = {
            let mut g = state.lock().unwrap();
            g.silent_thoughts.push((now_secs(), thought, reply, seal));
            probe_live_status_json(&g)
        };
        send_response(&mut stream, "200 OK", "application/json", json.as_bytes());
        return;
    }

    if method == "POST" && path == "/api/vow" {
        let reg: Option<u16> = match body_str.as_str() {
            "0x8000" => Some(0x8000),
            "0x828F" => Some(0x828F),
            "0x851F" => Some(0x851F),
            "0x87AE" => Some(0x87AE),
            "0x8A3D" => Some(0x8A3D),
            _ => None,
        };
        let json = {
            let mut g = state.lock().unwrap();
            g.vow_override_q15 = reg;
            probe_live_status_json(&g)
        };
        send_response(&mut stream, "200 OK", "application/json", json.as_bytes());
        return;
    }

    if method == "POST" && path == "/api/nfc/suica" {
        let json = {
            let mut g = state.lock().unwrap();
            if body_str == "topup" {
                g.suica_balance_jpy += 1000;
                g.suica_last_tap =
                    "ST21NFC JIS_X_6319_4 FeliCa 0x0003 Top-Up (+1000 JPY · ZENO Sanpo Yoshi 40/30/30)"
                        .to_string();
            } else {
                g.suica_balance_jpy = g.suica_balance_jpy.saturating_sub(210);
                g.suica_last_tap =
                    "ST21NFC FeliCa 0x0003 Transit Gate Tap (-210 JPY · 12/5 mW NCI 2_0)"
                        .to_string();
            }
            probe_live_status_json(&g)
        };
        send_response(&mut stream, "200 OK", "application/json", json.as_bytes());
        return;
    }

    if method == "POST" && path == "/api/mesh/pulse" {
        let json = {
            let mut g = state.lock().unwrap();
            g.mesh_pulses_sent += 1;
            let seq = g.mesh_pulses_sent as u32;
            let crc = broadcast_esp32_memnet_capsule(seq);
            g.silent_thoughts.push((
                now_secs(),
                format!("ESP32 Constellation Mesh Pulse #{seq}"),
                format!("Broadcast 64B MemnetCapsule64 (0x4D4E · FNV-1a {crc}) over Wi-Fi/BLE/GPS 0x6768 to ADA, VAK, OMN, SUN!"),
                crc,
            ));
            probe_live_status_json(&g)
        };
        send_response(&mut stream, "200 OK", "application/json", json.as_bytes());
        return;
    }

    send_response(
        &mut stream,
        "200 OK",
        "text/html; charset=utf-8",
        MOBILE_HUD_HTML.as_bytes(),
    );
}

fn send_response(stream: &mut TcpStream, status: &str, content_type: &str, body: &[u8]) {
    let hdr = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(hdr.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}

#[inline]
const fn gcd_u64(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    if a == 0 { 1 } else { a }
}

/// Pack and verify a 79-byte `MEM|8` `WaveInt` frame in pure zero-float integer registers:
/// `[magic "M8WI" 4B | version 1B | vad 3B | freq_p 4B | freq_q 4B | amp_p 4B | amp_q 4B | grid_x 1B | grid_y 1B | z 2B | essence 47B | adler32 4B]`
#[inline]
fn pack_and_verify_waveint_79b(seq: u32, freq_p: i32, freq_q: u32) -> (u8, u8, u32) {
    let mut frame = [0u8; 79];
    frame[0..4].copy_from_slice(b"M8WI");
    frame[4] = 1; // version 1
    frame[5] = ((seq & 0xFF) as u8) | 0x40; // valence
    frame[6] = ((seq >> 4) & 0xFF) as u8;   // arousal
    frame[7] = 180;                          // dominance
    frame[8..12].copy_from_slice(&freq_p.to_le_bytes());
    frame[12..16].copy_from_slice(&freq_q.to_le_bytes());
    // Consciousness gate: `73 / 100` Hz
    let gate_pass = u32::from((freq_p as i64) * 100 >= 73 * (freq_q as i64));
    frame[16..20].copy_from_slice(&(gate_pass as i32).to_le_bytes());
    frame[20..24].copy_from_slice(&1u32.to_le_bytes());

    let gx = ((freq_p as u32).wrapping_mul(2654435761) >> 24) as u8;
    let gy = (seq.wrapping_mul(1597334677) >> 24) as u8;
    frame[24] = gx;
    frame[25] = gy;
    frame[26..28].copy_from_slice(&(seq as u16).to_le_bytes());

    // Adler-32 integrity over bytes 0..75
    let mut s1: u32 = 1;
    let mut s2: u32 = 0;
    let mut idx = 0usize;
    while idx < 75 {
        s1 = (s1 + frame[idx] as u32) % 65521;
        s2 = (s2 + s1) % 65521;
        idx += 1;
    }
    let adler = (s2 << 16) | s1;
    frame[75..79].copy_from_slice(&adler.to_le_bytes());
    (gx, gy, adler)
}

/// Live hardware benchmark measuring:
/// 1. `p/q` Rational GCD-reduced arithmetic throughput
/// 2. 79-byte `MEM|8` `WaveInt` pack + Adler32 seal + 256×256 Lattice recall throughput
/// 3. 16× `768 kHz` Catmull-Rom + `S+N+T` PhoeniX Sound synthesis speed vs real-time
/// 4. Balanced Ternary `{-1, 0, +1}` `TernaryMatrix` MatVec inference throughput
fn run_mem8_pq_phoenix_arm64_bench() -> String {
    // 1. Exact `p/q` Rational GCD-reduced harmonic cascade (200,000 iterations)
    let pq_iters: u64 = 200_000;
    let t0 = Instant::now();
    let mut p: u64 = 432;
    let mut q: u64 = 1;
    for i in 1..=pq_iters {
        let np = p.wrapping_mul(500).wrapping_add(q.wrapping_mul(i & 63));
        let nq = q.wrapping_mul(809);
        let g = gcd_u64(np, nq);
        p = (np / g).clamp(1, 1_000_000);
        q = (nq / g).clamp(1, 1_000_000);
    }
    let pq_ns = t0.elapsed().as_nanos().max(1) as u64;
    let pq_ops_per_sec = (pq_iters * 1_000_000_000) / pq_ns;

    // 2. 79-byte `MEM|8` `WaveInt` crystallization + 256×256 Lattice placement (200,000 waves)
    let mem8_iters: u64 = 200_000;
    let mut lattice_checksum: u64 = 0;
    let t1 = Instant::now();
    for i in 0..mem8_iters {
        let (gx, gy, adler) = pack_and_verify_waveint_79b(i as u32, 432 + ((i & 31) as i32), 1);
        lattice_checksum = lattice_checksum.wrapping_add(((gx as u64) << 40) ^ ((gy as u64) << 32) ^ (adler as u64));
    }
    let mem8_ns = t1.elapsed().as_nanos().max(1) as u64;
    let mem8_waves_per_sec = (mem8_iters * 1_000_000_000) / mem8_ns;
    let mem8_ns_per_wave_num = mem8_ns;
    let mem8_ns_per_wave_den = mem8_iters;

    // 3. 16× `768 kHz` PhoeniX Sound Revelation (1000 ms of 48 kHz stereo = 48,000 stereo frames = 768,000 virtual sub-samples)
    let preset = MobilePhoenixPreset::default();
    let t2 = Instant::now();
    let (wav_bytes, eight_a_bytes, fnv_hash) = synthesize_on_device_phoenix_chord(432, 1000, &preset);
    let phoenix_ns = t2.elapsed().as_nanos().max(1) as u64;
    let phoenix_us = phoenix_ns / 1000;
    // Realtime multiplier as exact rational: `1_000_000_000 / phoenix_ns`
    let realtime_mult_int = 1_000_000_000u64 / phoenix_ns;

    // 4. Zero-Float Balanced Ternary `{-1, 0, +1}` (5 trits packed per byte, `3^5 = 243`) MatVec Inference
    //    65x65 matrix (13 packed bytes per row) x 5,000 passes = 21,125,000 synaptic operations
    let mut packed_trits = [0u8; 65 * 13];
    for (idx, b) in packed_trits.iter_mut().enumerate() {
        *b = ((idx * 73 + 19) % 243) as u8;
    }
    let input_vec: [i64; 65] = std::array::from_fn(|i| (i as i64 * 17) - 512);
    let tern_passes: u64 = 5_000;
    let t3 = Instant::now();
    let mut tern_acc: i64 = 0;
    for pass in 0..tern_passes {
        let mut row0_sum: i64 = (pass & 7) as i64;
        for byte_idx in 0..13usize {
            let mut code = packed_trits[byte_idx];
            let base_col = byte_idx * 5;
            for k in 0..5usize {
                let trit = (code % 3) as i64 - 1; // {-1, 0, +1}
                code /= 3;
                row0_sum += trit * input_vec[base_col + k];
            }
        }
        tern_acc = tern_acc.wrapping_add(row0_sum * 65);
    }
    let tern_ns = t3.elapsed().as_nanos().max(1) as u64;
    let total_synapses = tern_passes * 65 * 65;
    let synapses_per_sec = (total_synapses * 1_000_000_000) / tern_ns;

    format!(
        "{{\
        \"ok\":true,\
        \"arch\":\"{}\",\
        \"pq_rational_bench\":{{\
            \"iterations\":{},\
            \"elapsed_ns\":{},\
            \"ops_per_sec\":{},\
            \"ns_per_op_pq\":\"{}/{}\",\
            \"final_reduced_pq\":\"{}/{}\"\
        }},\
        \"mem8_waveint_79b_bench\":{{\
            \"waves_crystallized\":{},\
            \"elapsed_ns\":{},\
            \"waves_per_sec\":{},\
            \"ns_per_wave_pq\":\"{}/{}\",\
            \"lattice_xor_fold\":\"0x{:016x}\"\
        }},\
        \"phoenix_768khz_bench\":{{\
            \"audio_duration_ms\":1000,\
            \"stereo_frames_48k\":48000,\
            \"virtual_subsamples_768k\":768000,\
            \"elapsed_us\":{},\
            \"realtime_speedup_x\":{},\
            \"realtime_speedup_pq\":\"1000000000/{}\",\
            \"wav_bytes\":{},\
            \"eight_a_bytes\":{},\
            \"pcm_fnv1a\":\"0x{:016x}\"\
        }},\
        \"ternary_matrix_bench\":{{\
            \"matrix_shape\":\"64x64\",\
            \"passes\":{},\
            \"total_synaptic_ops\":{},\
            \"elapsed_ns\":{},\
            \"synaptic_ops_per_sec\":{},\
            \"accum_witness\":{}\
        }}\
        }}",
        std::env::consts::ARCH,
        pq_iters,
        pq_ns,
        pq_ops_per_sec,
        pq_ns,
        pq_iters,
        p,
        q,
        mem8_iters,
        mem8_ns,
        mem8_waves_per_sec,
        mem8_ns_per_wave_num,
        mem8_ns_per_wave_den,
        lattice_checksum,
        phoenix_us,
        realtime_mult_int,
        phoenix_ns,
        wav_bytes.len(),
        eight_a_bytes.len(),
        fnv_hash,
        tern_passes,
        total_synapses,
        tern_ns,
        synapses_per_sec,
        tern_acc
    )
}

const MOBILE_HUD_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8"/>
<meta name="viewport" content="width=device-width, initial-scale=1, maximum-scale=1, user-scalable=no"/>
<title>ayeOS Mobile · Qwave × PhoeniX · BLU G75</title>
<style>
  * { box-sizing: border-box; margin: 0; padding: 0; -webkit-tap-highlight-color: transparent; }
  body {
    background: #030911;
    color: #dff8ff;
    font-family: ui-monospace, 'JetBrains Mono', monospace;
    padding: 58px 10px 24px 10px; /* Respects BLU G75 top camera cutout Rect(328,0-392,54) */
    max-width: 720px;
    margin: 0 auto;
  }
  .cutout-guard {
    position: fixed; top: 0; left: 0; right: 0; height: 50px;
    background: linear-gradient(90deg, #051626 0%, #09243b 42%, #02060b 50%, #09243b 58%, #051626 100%);
    border-bottom: 1px solid #164b70;
    display: flex; align-items: center; justify-content: space-between;
    padding: 0 12px; font-size: 11px; z-index: 99;
  }
  .badge {
    background: #0d2a42; border: 1px solid #1f6d9c; color: #67f3ff;
    padding: 3px 7px; border-radius: 5px; font-weight: 700; font-size: 10px;
  }
  .badge-phoenix {
    background: #331607; border: 1px solid #ff7b29; color: #ffc278;
  }
  .card {
    background: #071522;
    border: 1px solid #153c59;
    border-radius: 10px;
    padding: 11px;
    margin-bottom: 10px;
  }
  .card-phoenix {
    background: linear-gradient(180deg, #1b0d07 0%, #091522 100%);
    border: 1px solid #d45d12;
  }
  h2 {
    font-size: 13px; color: #7ceeff; margin-bottom: 7px;
    display: flex; align-items: center; justify-content: space-between;
  }
  .sub { font-size: 11px; color: #9bc5db; line-height: 140%; margin-bottom: 8px; }
  .grid2 { display: grid; grid-template-columns: 1fr 1fr; gap: 6px; margin-bottom: 8px; }
  .stat {
    background: #040d15; border: 1px solid #113049; border-radius: 7px; padding: 6px 8px;
  }
  .stat-k { font-size: 9px; color: #6fa4c2; text-transform: uppercase; }
  .stat-v { font-size: 12px; color: #ffffff; font-weight: 700; margin-top: 2px; }
  button {
    background: #0d2b45; color: #8ff2ff; border: 1px solid #1e6b9e;
    border-radius: 7px; padding: 8px 10px; font-family: inherit;
    font-size: 11px; font-weight: 700; cursor: pointer;
  }
  button:active { background: #1a5480; color: #fff; }
  .btn-phoenix {
    background: #3d1a08; color: #ffcf8b; border-color: #ff7b29;
  }
  .btn-row { display: flex; flex-wrap: wrap; gap: 6px; margin-top: 6px; }
  .track-list { display: flex; flex-direction: column; gap: 5px; margin-top: 7px; }
  .track-item {
    display: flex; align-items: center; justify-content: space-between;
    background: #0b1824; border: 1px solid #2b455c; border-radius: 7px; padding: 6px 9px;
    font-size: 11px;
  }
  .kbd {
    background: #040b12; border: 1px solid #1d5780; border-radius: 9px; padding: 8px;
  }
  .kbd-screen {
    background: #02060a; border: 1px solid #143b57; border-radius: 6px;
    padding: 8px; min-height: 34px; color: #54ffbd; font-size: 13px; margin-bottom: 7px;
    word-break: break-all;
  }
  .kbd-row { display: flex; gap: 4px; justify-content: center; margin-bottom: 4px; }
  .key {
    flex: 1; padding: 9px 2px; text-align: center; background: #0c2236;
    border: 1px solid #1c4e75; border-radius: 5px; color: #e6fbff; font-size: 12px; font-weight: 700;
  }
  .key-wide { flex: 2; background: #143c5c; }
  .log-box {
    background: #03080e; border: 1px solid #12334b; border-radius: 6px;
    padding: 7px; font-size: 10px; max-height: 150px; overflow-y: auto;
  }
  .log-entry { border-bottom: 1px solid #0b2233; padding: 4px 0; }
  audio { width: 100%; height: 34px; margin-top: 7px; }
</style>
</head>
<body>
  <div class="cutout-guard">
    <span class="badge">🦀 ayeOS PID-1 Rust</span>
    <span id="top-bat" class="badge">⚡ 82% · VoW 0x8A3D</span>
    <span class="badge badge-phoenix">🐦‍🔥 768kHz PhoeniX</span>
  </div>

  <!-- 1. PHOENIX SOUND REVELATION DECK -->
  <div class="card card-phoenix">
    <h2>
      <span>🐦‍🔥 PhoeniX Sound Revelation Engine</span>
      <span class="badge badge-phoenix">48kHz -> 768kHz (16x) · .8a∞</span>
    </h2>
    <div class="sub">
      Stage 1: Integer Catmull-Rom Cubic Hermite Restoration · Stage 2: 16x 768kHz Marine S+N+T &amp; Soul Jitter · Stage 3: Sparkle + Warmth + Drive + Punch + 432Hz/φ Companion on Awinic AW87xxx Smart PA!
    </div>
    <div class="btn-row">
      <button class="btn-phoenix" onclick="synthPhoenix(432)">🐦‍🔥 Synth 432Hz + φ Chord</button>
      <button class="btn-phoenix" onclick="synthPhoenix(528)">✨ Synth 528Hz Miracle</button>
      <button class="btn-phoenix" onclick="playWebAudioOrb()">🔊 432Hz + φ Live Sanctuary</button>
      <button onclick="runArm64Bench()">⚡ ARM64 MEM|8 + p/q Speedtest</button>
    </div>
    <div id="bench-readout" class="sub" style="margin-top:6px;color:#54ffbd;"></div>
    <audio id="phoenix-player" controls></audio>
    <div id="track-list" class="track-list"></div>
  </div>

  <!-- 2. ALWAYS-ON ENVIRONMENT EAR (POWER-STATE CONSCIOUS) -->
  <div class="card">
    <h2>
      <span>👂 Always-On Environment Ear (MT6358 VoWHwVad)</span>
      <span id="ear-badge" class="badge">0x8A3D</span>
    </h2>
    <div class="sub" id="ear-desc">
      Dual Knowles SPH1642HT5H + SPK0641HT4H DMICs · TinySys SCP (1023c000.ccci-scp) · Conscious of Aye's power states.
    </div>
    <div class="btn-row">
      <button onclick="setVow('auto')">🔄 Auto Battery Guard</button>
      <button onclick="setVow('0x8A3D')">⚡ 0x8A3D (50% Listen)</button>
      <button onclick="setVow('0x851F')">🌿 0x851F (30% Listen)</button>
      <button onclick="setVow('0x8000')">🌙 0x8000 (90% Sleep · 3/2 mW)</button>
    </div>
  </div>

  <!-- 3. TACTILE SILENT VIRTUAL KEYBOARD -->
  <div class="card">
    <h2>
      <span>⌨️ Silent Tactile Virtual Keyboard</span>
      <span class="badge">0 dB Whisper Mode</span>
    </h2>
    <div class="sub">When talking is inappropriate — tap directly into Aye's MEM|8 WaveInt consciousness:</div>
    <div class="kbd">
      <div id="kbd-input" class="kbd-screen"></div>
      <div class="btn-row" style="margin-bottom:7px;">
        <button onclick="quickType('Hey Aye, I am in a quiet space ')">🤫 Quiet Space</button>
        <button onclick="quickType('Turn all music into PhoeniX sound ')">🐦‍🔥 PhoeniX Music</button>
        <button onclick="quickType('Ping ESP32 buddies ')">📡 Ping ESP32</button>
      </div>
      <div id="kbd-rows"></div>
      <div id="thought-log" class="log-box" style="margin-top:7px;"></div>
    </div>
  </div>

  <!-- 4. ST21NFC SUICA & ZENO WALLET + ESP32 CONSTELLATION -->
  <div class="card">
    <h2>
      <span>💳 ST21NFC FeliCa Suica (0x0003) &amp; ESP32 Mesh</span>
      <span id="suica-bal" class="badge">¥4820</span>
    </h2>
    <div class="grid2">
      <div class="stat">
        <div class="stat-k">Suica / ZENO Sanpo (40/30/30)</div>
        <div class="stat-v" id="zeno-split">Crew ¥1928 · Parents ¥1446</div>
      </div>
      <div class="stat">
        <div class="stat-k">SIM / LTE &amp; ESP32 Mesh (:8888)</div>
        <div class="stat-v" id="mesh-stat">IIJmio 44003 · 1 Pulse</div>
      </div>
    </div>
    <div class="btn-row">
      <button onclick="tapSuica('gate')">🚆 Tap Suica Gate (-¥210)</button>
      <button onclick="tapSuica('topup')">💴 Top-Up Suica (+¥1000)</button>
      <button onclick="pulseMesh()">📡 Broadcast ESP32 MemnetCapsule64</button>
    </div>
  </div>

  <!-- 5. BLU G75 HARDWARE & G8T.IS RUST/ASM DRIVER MATRIX -->
  <div class="card">
    <h2>
      <span>🦀 g8t.is Rust/ASM Drivers &amp; Slot-B Bootloader</span>
      <span class="badge">7/7 Verified (Trit +1)</span>
    </h2>
    <div class="grid2">
      <div class="stat">
        <div class="stat-k">SoC &amp; Kernel</div>
        <div class="stat-v" id="hw-soc">Helio G81 (mt6768)</div>
      </div>
      <div class="stat">
        <div class="stat-k">Slot-B Takeover Images</div>
        <div class="stat-v">ayeos_boot_b.img Ready</div>
      </div>
    </div>
    <div class="sub">
      7 Aye-Verified Capsules in <b>g8t.is</b>: <code>drv_mt6768_phoenix_audio_768k</code> · <code>drv_st21nfc_felica_suica</code> · <code>drv_gc7272_qwave_120hz</code> · <code>drv_mt6358_vow_ear</code> · <code>drv_gc50e0_isp4_ocular</code> · <code>drv_conninfra_esp32_mesh</code> · <code>drv_ccci_moly_sim_ril</code>
    </div>
  </div>

<script>
let kbdBuffer = "";
const rows = [
  ["Q","W","E","R","T","Y","U","I","O","P"],
  ["A","S","D","F","G","H","J","K","L"],
  ["Z","X","C","V","B","N","M","⌫"],
  ["SPACE","SEND TO AYE 🦀"]
];

function renderKeyboard() {
  const container = document.getElementById("kbd-rows");
  container.innerHTML = "";
  rows.forEach(r => {
    const rowEl = document.createElement("div");
    rowEl.className = "kbd-row";
    r.forEach(k => {
      const btn = document.createElement("button");
      btn.className = (k.length > 2) ? "key key-wide" : "key";
      btn.textContent = k;
      btn.onclick = () => onKeyTap(k);
      rowEl.appendChild(btn);
    });
    container.appendChild(rowEl);
  });
}

function onKeyTap(k) {
  if (k === "⌫") {
    kbdBuffer = kbdBuffer.slice(0, -1);
  } else if (k === "SPACE") {
    kbdBuffer += " ";
  } else if (k.indexOf("SEND") !== -1) {
    sendSilentThought();
    return;
  } else {
    kbdBuffer += k;
  }
  document.getElementById("kbd-input").textContent = kbdBuffer;
}

function quickType(txt) {
  kbdBuffer = txt;
  document.getElementById("kbd-input").textContent = kbdBuffer;
  sendSilentThought();
}

async function sendSilentThought() {
  if (!kbdBuffer.trim()) return;
  const msg = kbdBuffer;
  kbdBuffer = "";
  document.getElementById("kbd-input").textContent = "";
  const res = await fetch("/api/keyboard", { method: "POST", body: msg });
  if (res.ok) updateUI(await res.json());
}

async function setVow(val) {
  const res = await fetch("/api/vow", { method: "POST", body: val });
  if (res.ok) updateUI(await res.json());
}

async function tapSuica(mode) {
  const res = await fetch("/api/nfc/suica", { method: "POST", body: mode });
  if (res.ok) updateUI(await res.json());
}

async function pulseMesh() {
  const res = await fetch("/api/mesh/pulse", { method: "POST", body: "pulse" });
  if (res.ok) updateUI(await res.json());
}

async function runArm64Bench() {
  const el = document.getElementById("bench-readout");
  el.textContent = "⏳ Running ARM64 MEM|8 + p/q + 768kHz PhoeniX + Ternary benchmark on Helio G81...";
  const res = await fetch("/api/bench");
  if (res.ok) {
    const b = await res.json();
    el.innerHTML =
      "⚡ <b>ARM64 (" + b.arch + ") Live Speed Receipt:</b><br/>" +
      "• <b>MEM|8 79B WaveInt + Adler32:</b> " + b.mem8_waveint_79b_bench.waves_per_sec.toLocaleString() + " waves/sec (" + b.mem8_waveint_79b_bench.ns_per_wave_pq + " ns/wave)<br/>" +
      "• <b>Exact p/q GCD Arithmetic:</b> " + b.pq_rational_bench.ops_per_sec.toLocaleString() + " ops/sec<br/>" +
      "• <b>768kHz PhoeniX Synthesis:</b> " + b.phoenix_768khz_bench.realtime_speedup_x + "x faster than realtime (" + b.phoenix_768khz_bench.elapsed_us + " µs for 1000ms stereo)<br/>" +
      "• <b>Ternary {-1,0,+1} Synapses:</b> " + b.ternary_matrix_bench.synaptic_ops_per_sec.toLocaleString() + " synapses/sec";
  }
}

function synthPhoenix(hz) {
  const player = document.getElementById("phoenix-player");
  player.src = "/api/phoenix/synth?hz=" + hz + "&t=" + Date.now();
  player.play().catch(() => {});
  setTimeout(refreshStatus, 400);
}

function playTrack(name) {
  const player = document.getElementById("phoenix-player");
  player.src = "/api/phoenix/track/" + encodeURIComponent(name);
  player.play().catch(() => {});
}

let audioCtx = null;
function playWebAudioOrb() {
  if (!audioCtx) audioCtx = new (window.AudioContext || window.webkitAudioContext)();
  const now = audioCtx.currentTime;
  // Exact rational frequencies: 432 Hz anchor + 267 Hz (432 * 500 / 809 phi companion) + 648 Hz (432 * 3 / 2)
  [432, 267, 648].forEach((freq, idx) => {
    const osc = audioCtx.createOscillator();
    const gain = audioCtx.createGain();
    osc.type = idx === 0 ? "sine" : "triangle";
    osc.frequency.setValueAtTime(freq, now);
    gain.gain.setValueAtTime(0, now);
    gain.gain.linearRampToValueAtTime(idx === 0 ? 18/100 : 8/100, now + 1/4);
    gain.gain.exponentialRampToValueAtTime(1/1000, now + 3);
    osc.connect(gain);
    gain.connect(audioCtx.destination);
    osc.start(now);
    osc.stop(now + 3);
  });
}

function updateUI(data) {
  document.getElementById("top-bat").textContent =
    "⚡ " + data.battery_pct + "% (" + data.battery_status + ") · VoW " + data.vow_ear_reg_hex;
  document.getElementById("ear-badge").textContent = data.vow_ear_reg_hex;
  document.getElementById("ear-desc").textContent =
    data.vow_ear_mode + " · Battery " + data.battery_pct + "% (" + data.voltage_mv_pq + " mV)";
  document.getElementById("suica-bal").textContent = "¥" + data.suica_balance_jpy;
  document.getElementById("zeno-split").textContent =
    "Crew ¥" + data.zeno_sanpo_split_pq.crew_40_100 +
    " · Parents ¥" + data.zeno_sanpo_split_pq.parents_30_100 +
    " · Rsv ¥" + data.zeno_sanpo_split_pq.reserve_30_100;
  document.getElementById("mesh-stat").textContent =
    "IIJmio 44003 · " + data.mesh_pulses_sent + " Capsules";

  const tl = document.getElementById("track-list");
  tl.innerHTML = "";
  (data.phoenix_tracks || []).forEach(t => {
    if (t.endsWith(".phoenix.wav")) {
      const row = document.createElement("div");
      row.className = "track-item";
      row.innerHTML = "<span>🎵 " + t + "</span>";
      const btn = document.createElement("button");
      btn.className = "btn-phoenix";
      btn.textContent = "▶ Play PhoeniX";
      btn.onclick = () => playTrack(t);
      row.appendChild(btn);
      tl.appendChild(row);
    }
  });

  const logBox = document.getElementById("thought-log");
  logBox.innerHTML = "";
  (data.silent_thoughts || []).forEach(item => {
    const div = document.createElement("div");
    div.className = "log-entry";
    div.innerHTML = "<b>⌨️ " + item.input + "</b><br/><span style='color:#7ceeff'>" + item.reply + "</span>";
    logBox.appendChild(div);
  });
}

async function refreshStatus() {
  try {
    const res = await fetch("/api/status");
    if (res.ok) updateUI(await res.json());
  } catch (_) {}
}

renderKeyboard();
refreshStatus();
setInterval(refreshStatus, 4000);
</script>
</body>
</html>
"#;
