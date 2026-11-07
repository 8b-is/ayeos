//! `ayeos-sanctuary` — Sovereign AyeOS TV Revolution, PhoeniX 384 kHz Babbage Audio,
//! `/dev/fb0` HDMI Visualizer & MEMNET-Backed Diskless iPXE / TFTP / DHCP / DNS Node.
//!
//! Runs as:
//! 1. **PID 1 Diskless RAM OS (`/init` or `--pid1-init`)** on SD-cardless Raspberry Pi 4s booted
//!    via iPXE / TFTP from another AyeOS peer. Mounts `/proc`, `/sys`, `/dev`, `/tmp`, `/run`,
//!    replicates the netboot bundle from `ayeos.peer=<mother_ip>:8432` into RAM (`/tmp/ayeos-netboot`),
//!    registers itself into the `MEMNET` mesh (`/api/memnet/register`), and immediately becomes
//!    a new **AyeOS Mother Node** so the next diskless Raspberry Pi can iPXE-boot from IT!
//! 2. **Sovereign Sanctuary Daemon (`--serve` / default on `:8432`)**:
//!    - Answers **MEMNET DNS** (`:53` / `:8053`) for `.ayeos`, `.memnet`, `.local`, `.8b.is`
//!    - Answers **MEMNET DHCP / ProxyDHCP / PXE / iPXE** (`:67` / `:4011` / `:6767`) with Option 43
//!      (`"Raspberry Pi Boot"`) and Option 67 (`http://<ip>:8432/boot/ayeos.ipxe`)
//!    - Answers **RFC 1350 TFTP** (`:69` / `:6969`) and **HTTP iPXE** (`:8432/boot/*`)
//!    - Renders the **Zero-Dependency `/dev/fb0` HDMI TV Visualizer** + **60fps HTML5 OxiCast Stage**
//!    - Streams **Beast Least-Cost Babbage 3-Addition Cubic Stepper PhoeniX Audio** (`/api/oxicast/stream.wav`
//!      & `/api/oxicast/stream24.wav`) + **16-byte `.8a` (`8a∞`) UDP Ambilight/Touch Sync** (`:8432` & `:8888`).

use ayeos::desktop::{build_ayeos_desktop_json, AyeosDesktopState, PresenceLocation};
use ayeos::memnet_fabric::{
    build_memnet_fabric_json, detect_local_ipv4, DhcpChannelMode, MemnetFabricBackend,
};
use ayeos::oxicast::{
    build_ayeos_ipxe_script, build_ayeos_oxicast_json, build_pi_eeprom_pxe_enabler_script,
    build_pi_netboot_cmdline_txt, build_pi_netboot_config_txt, paint_frame_to_hardware_fb0,
    run_ayeos_sanctuary_daemon, synthesize_live_ayeos_initramfs, AyeosOxicastState,
    AYEOS_OXICAST_PORT,
};
use ayeos::prng::seed_hash_hex;
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

fn main() {
    let args: Vec<String> = env::args().collect();
    let is_pid1 = std::process::id() == 1 || args.iter().any(|a| a == "--pid1-init");

    if args.iter().any(|a| a == "--status") {
        let mut st = AyeosOxicastState::default();
        st.step_harmonic_telemetry();
        println!("{}", build_ayeos_oxicast_json(&st));
        return;
    }

    if args.iter().any(|a| a == "--desktop-status") {
        let mut desk = AyeosDesktopState::default();
        if let Some(pos) = args.iter().position(|a| a == "--presence") {
            if let Some(loc) = args.get(pos + 1) {
                desk.set_presence(PresenceLocation::from_name(loc));
            }
        }
        println!("{}", build_ayeos_desktop_json(&desk));
        return;
    }

    if args.iter().any(|a| a == "--memnet-status") {
        let ip = detect_local_ipv4();
        let mut backend =
            MemnetFabricBackend::new(ip, AYEOS_OXICAST_PORT, DhcpChannelMode::ProxyPxeAndOpportunistic);
        let _ = backend.register_or_update_dhcp_peer(
            [0xDC, 0xA6, 0x32, 0x88, 0x43, 0x20],
            "PXEClient:Arch:00000:UNDI:002001",
            "iPXE",
            "oxicast-pi",
        );
        println!("{}", build_memnet_fabric_json(&backend));
        return;
    }

    if let Some(pos) = args.iter().position(|a| a == "--resolve") {
        let query = args
            .get(pos + 1)
            .map(|s| s.as_str())
            .unwrap_or("boot.ayeos");
        let ip = detect_local_ipv4();
        let mut backend =
            MemnetFabricBackend::new(ip, AYEOS_OXICAST_PORT, DhcpChannelMode::ProxyPxeAndOpportunistic);
        if let Some((resolved_ip, txt, score_q10)) = backend.resolve_memnet_domain(query) {
            println!(
                "{{\"ok\":true,\"query\":\"{query}\",\"ip\":\"{}.{}.{}.{}\",\"score_pq\":\"{score_q10}/1024\",\"txt\":\"{txt}\"}}",
                resolved_ip[0], resolved_ip[1], resolved_ip[2], resolved_ip[3]
            );
        } else {
            println!("{{\"ok\":false,\"query\":\"{query}\",\"error\":\"outside_memnet_zone\"}}");
        }
        return;
    }

    if let Some(pos) = args.iter().position(|a| a == "--cast") {
        let title = args
            .get(pos + 1)
            .map(|s| s.as_str())
            .unwrap_or("🌸 AyeOS Sanctuary Cast");
        let markdown = args
            .get(pos + 2)
            .map(|s| s.as_str())
            .unwrap_or("### Casted from AyeOS Mesh\n- **Zero SD Card · iPXE + MEMNET**");
        let host = env::var("OXICAST_HOST").unwrap_or_else(|_| format!("127.0.0.1:{AYEOS_OXICAST_PORT}"));
        cast_markdown_to_peer(&host, title, markdown);
        return;
    }

    if let Some(pos) = args.iter().position(|a| a == "--build-netboot-bundle") {
        let out_dir = args
            .get(pos + 1)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/home/hue/.cache/oxicast-pi/netboot"));
        build_netboot_bundle(&out_dir);
        return;
    }

    if let Some(pos) = args.iter().position(|a| a == "--flash-pxe-sdcard") {
        let dev = args
            .get(pos + 1)
            .map(|s| s.as_str())
            .unwrap_or("/dev/sda");
        let _ = std::process::Command::new("/home/hue/8b-is/oxicast/oxicast-pi/flash-pxe-eeprom-sdcard.sh")
            .arg(dev)
            .status();
        return;
    }

    let mut peer_mother = extract_cmdline_peer();
    if is_pid1 {
        bootstrap_pid1_diskless_pi(&mut peer_mother);
    }

    let port: u16 = args
        .iter()
        .position(|a| a == "--port")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(AYEOS_OXICAST_PORT);

    run_ayeos_sanctuary_daemon(port, peer_mother);
}

/// Extract `ayeos.peer=<ip:port>` from `/proc/cmdline` if booted over iPXE / TFTP.
fn extract_cmdline_peer() -> Option<String> {
    let cmdline = fs::read_to_string("/proc/cmdline").ok()?;
    for tok in cmdline.split_whitespace() {
        if let Some(peer) = tok.strip_prefix("ayeos.peer=") {
            if !peer.is_empty() {
                return Some(peer.to_string());
            }
        }
    }
    None
}

/// Configure an IPv4 address (`ip`) and netmask (`mask`) on `ifname` (`eth0` / `end0`)
/// directly via `SIOCSIFADDR` (`0x8916`) and `SIOCSIFNETMASK` (`0x891C`) in PID 1 initramfs.
fn configure_interface_ipv4(ifname: &str, ip: [u8; 4], mask: [u8; 4]) {
    #[repr(C)]
    struct SockAddrIn {
        sin_family: u16,
        sin_port: u16,
        sin_addr: [u8; 4],
        sin_zero: [u8; 8],
    }
    #[repr(C)]
    struct IfReqAddr {
        ifr_name: [u8; 16],
        ifr_addr: SockAddrIn,
        _pad: [u8; 8],
    }
    unsafe {
        let sock = libc::socket(libc::AF_INET, libc::SOCK_DGRAM, 0);
        if sock >= 0 {
            let mut name_buf = [0u8; 16];
            let bytes = ifname.as_bytes();
            let copy_len = bytes.len().min(15);
            name_buf[..copy_len].copy_from_slice(&bytes[..copy_len]);

            let mut req_ip = IfReqAddr {
                ifr_name: name_buf,
                ifr_addr: SockAddrIn {
                    sin_family: libc::AF_INET as u16,
                    sin_port: 0,
                    sin_addr: ip,
                    sin_zero: [0u8; 8],
                },
                _pad: [0u8; 8],
            };
            let _ = libc::ioctl(sock, 0x8916, &mut req_ip); // SIOCSIFADDR

            let mut req_mask = IfReqAddr {
                ifr_name: name_buf,
                ifr_addr: SockAddrIn {
                    sin_family: libc::AF_INET as u16,
                    sin_port: 0,
                    sin_addr: mask,
                    sin_zero: [0u8; 8],
                },
                _pad: [0u8; 8],
            };
            let _ = libc::ioctl(sock, 0x891C, &mut req_mask); // SIOCSIFNETMASK
            let _ = libc::close(sock);
        }
    }
}
fn bring_up_interface(ifname: &str, is_loopback: bool) {
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
                ifr_flags: if is_loopback {
                    (libc::IFF_UP | libc::IFF_LOOPBACK | libc::IFF_RUNNING) as i16
                } else {
                    (libc::IFF_UP | libc::IFF_BROADCAST | libc::IFF_RUNNING) as i16
                },
                _pad: [0u8; 22],
            };
            let bytes = ifname.as_bytes();
            let copy_len = bytes.len().min(15);
            ifr.ifr_name[..copy_len].copy_from_slice(&bytes[..copy_len]);
            let _ = libc::ioctl(sock, 0x8914, &mut ifr);
            let _ = libc::close(sock);
        }
    }
}

/// PID 1 (`/init`) bootstrap when a diskless Raspberry Pi 4 boots `ayeos-initramfs.cpio`
/// over iPXE or TFTP in RAM with **zero SD card**:
/// 1. Mounts `/proc`, `/sys`, `/dev`, `/tmp`, `/run`.
/// 2. Pets `/dev/watchdog` so BCM2711 PM_RSTC never resets the board.
/// 3. Brings up `lo`, `eth0`, and `end0`.
/// 4. Paints the Sovereign AyeOS × OxiCast × PhoeniX splash onto `/dev/fb0` HDMI TV.
/// 5. Replicates `kernel8.img`, `bcm2711-rpi-4-b.dtb`, `start4.elf`, and `fixup4.dat` from
///    its Mother Node (`ayeos.peer`) into `/tmp/ayeos-netboot` in RAM and registers itself
///    in `MEMNET` (`/api/memnet/register`) so THIS diskless Pi immediately becomes a new
///    Mother Node for the next diskless Pi!
fn bootstrap_pid1_diskless_pi(peer_mother: &mut Option<String>) {
    for dir in ["/proc", "/sys", "/dev", "/tmp", "/run", "/tmp/ayeos-netboot"] {
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
    }

    // Spawn BCM2711 hardware watchdog keeper
    std::thread::spawn(|| {
        let mut wdt = fs::OpenOptions::new().write(true).open("/dev/watchdog").ok();
        loop {
            if let Some(ref mut f) = wdt {
                let _ = f.write_all(b"1");
            }
            std::thread::sleep(Duration::from_secs(4));
        }
    });

    unsafe {
        // Ensure core character devices exist even if devtmpfs was not auto-mounted by kernel
        let _ = libc::mknod(c"/dev/null".as_ptr(), libc::S_IFCHR | 0o666, libc::makedev(1, 3));
        let _ = libc::mknod(c"/dev/console".as_ptr(), libc::S_IFCHR | 0o666, libc::makedev(5, 1));
        let _ = libc::mknod(c"/dev/tty0".as_ptr(), libc::S_IFCHR | 0o666, libc::makedev(4, 0));
        let _ = libc::mknod(c"/dev/tty1".as_ptr(), libc::S_IFCHR | 0o666, libc::makedev(4, 1));
        let _ = libc::mknod(c"/dev/fb0".as_ptr(), libc::S_IFCHR | 0o666, libc::makedev(29, 0));
    }

    bring_up_interface("lo", true);
    bring_up_interface("eth0", false);
    bring_up_interface("end0", false);
    configure_interface_ipv4("eth0", [172, 16, 47, 116], [255, 255, 255, 128]);
    configure_interface_ipv4("end0", [172, 16, 47, 116], [255, 255, 255, 128]);

    if peer_mother.is_none() {
        *peer_mother = extract_cmdline_peer();
    }

    let st = AyeosOxicastState::default();
    let _ = paint_frame_to_hardware_fb0(&st);

    // Replicate netboot firmware from Mother Peer in background + register in MEMNET
    if let Some(ref mother) = peer_mother.clone() {
        let mother_host = mother.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(2));
            replicate_from_mother_peer(&mother_host);
        });
    }
}

/// Pull netboot files (`kernel8.img`, `bcm2711-rpi-4-b.dtb`, `start4.elf`, `fixup4.dat`) from
/// the Mother Peer over HTTP into `/tmp/ayeos-netboot` and register this newly booted diskless
/// Pi in the Mother Peer's `MEMNET` registry!
fn replicate_from_mother_peer(mother_host: &str) {
    let out_dir = Path::new("/tmp/ayeos-netboot");
    let _ = fs::create_dir_all(out_dir);

    for artifact in [
        "kernel8.img",
        "bcm2711-rpi-4-b.dtb",
        "start4.elf",
        "fixup4.dat",
        "config.txt",
        "cmdline.txt",
    ] {
        if let Some(bytes) = http_get_bytes(mother_host, &format!("/boot/{artifact}")) {
            let _ = fs::write(out_dir.join(artifact), bytes);
        }
    }

    // Register this diskless Pi in Mother Node's MEMNET fabric
    if let Ok(mut stream) = TcpStream::connect(mother_host) {
        let body = format!(
            "geo:sanctuary + role:ayeos.tv.diskless_pi + tags:oxicast,phoenix,ipxe,ram_mother + vector:{}",
            &seed_hash_hex()[..16]
        );
        let req = format!(
            "POST /api/memnet/register HTTP/1.1\r\nHost: {mother_host}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(req.as_bytes());
    }
}

fn http_get_bytes(host: &str, path: &str) -> Option<Vec<u8>> {
    let mut stream = TcpStream::connect(host).ok()?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let req = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
    stream.write_all(req.as_bytes()).ok()?;
    let mut resp = Vec::new();
    stream.read_to_end(&mut resp).ok()?;
    let hdr_end = resp
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|i| i + 4)?;
    let header_str = String::from_utf8_lossy(&resp[..hdr_end]);
    if !header_str.starts_with("HTTP/1.1 200") && !header_str.starts_with("HTTP/1.0 200") {
        return None;
    }
    Some(resp[hdr_end..].to_vec())
}

fn cast_markdown_to_peer(host: &str, title: &str, markdown: &str) {
    let mut stream = match TcpStream::connect(host) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[ayeos-sanctuary] failed to connect to {host}: {e}");
            return;
        }
    };
    let payload = format!("{title}\n{}", markdown.replace("\\n", "\n"));
    let req = format!(
        "POST /api/oxicast/cast HTTP/1.1\r\nHost: {host}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
        payload.len()
    );
    let _ = stream.write_all(req.as_bytes());
    let mut resp = String::new();
    let _ = stream.read_to_string(&mut resp);
    println!("🌸 Casted to AyeOS TV Sanctuary ({host}): {title}");
}

/// Write out the complete diskless iPXE + Raspberry Pi 4 TFTP/HTTP netboot bundle to `out_dir`.
fn build_netboot_bundle(out_dir: &Path) {
    let _ = fs::create_dir_all(out_dir);
    let ip = detect_local_ipv4();
    let host_port = format!("{}.{}.{}.{}:{}", ip[0], ip[1], ip[2], ip[3], AYEOS_OXICAST_PORT);
    let ip_str = format!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3]);

    let ipxe_script = build_ayeos_ipxe_script(&host_port);
    let config_txt = build_pi_netboot_config_txt();
    let cmdline_txt = build_pi_netboot_cmdline_txt(&host_port);
    let eeprom_sh = build_pi_eeprom_pxe_enabler_script(&ip_str);
    let cpio_bytes = synthesize_live_ayeos_initramfs(&host_port);

    let _ = fs::write(out_dir.join("ayeos.ipxe"), ipxe_script.as_bytes());
    let _ = fs::write(out_dir.join("autoexec.ipxe"), ipxe_script.as_bytes());
    let _ = fs::write(out_dir.join("config.txt"), config_txt.as_bytes());
    let _ = fs::write(out_dir.join("cmdline.txt"), cmdline_txt.as_bytes());
    let _ = fs::write(out_dir.join("enable-diskless-pxe.sh"), eeprom_sh.as_bytes());
    let _ = fs::write(out_dir.join("ayeos-initramfs.cpio"), &cpio_bytes);

    println!("==================================================================");
    println!("🌸 AyeOS Diskless iPXE & Raspberry Pi Netboot Bundle Generated!");
    println!("   Directory         : {}", out_dir.display());
    println!("   Mother Node       : http://{host_port}");
    println!("   iPXE Script       : {}/ayeos.ipxe", out_dir.display());
    println!("   Initramfs (CPIO)  : {}/ayeos-initramfs.cpio ({} bytes)", out_dir.display(), cpio_bytes.len());
    println!("   EEPROM Unlocker   : {}/enable-diskless-pxe.sh", out_dir.display());
    println!("==================================================================");
}
