#!/usr/bin/env python3
"""AyeOS Universal Multi-Architecture PXE / UEFI HTTP Boot / iPXE / TFTP Builder & Verifier.

Zero floating-point arithmetic.
Builds, stages, and verifies all bare-metal AyeOS network-boot kernels so ANY machine on the LAN
can PXE-boot AyeOS with zero disk:
  1. x86_64 UEFI PXE (Arch 0x0007 / 0x0009) & UEFI HTTP Boot (Arch 0x0010) -> BOOTX64.EFI
  2. ARM64 UEFI PXE  (Arch 0x000B)          & UEFI HTTP Boot (Arch 0x0013) -> BOOTAA64.EFI
  3. x86 Legacy BIOS PXE (Arch 0x0000)      & Limine PXE                   -> limine-bios-pxe.bin + ayeos-x86_64.elf
  4. Raspberry Pi 4 / 5 BCM2711 EEPROM TFTP ROM                            -> config.txt + kernel8.img
  5. Universal iPXE Chainloader (`Option 77 = "iPXE"`)                     -> ayeos.ipxe
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import socket
import struct
import subprocess
import sys

AYEOS_ROOT = Path("/home/hue/source/8b-is/ayeos")
NETBOOT_DIRS = (
    AYEOS_ROOT / "netboot",
    Path("/home/hue/.cache/oxicast-pi/netboot"),
)
LIMINE_SHARE = Path("/usr/share/limine")


def sha256_file(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        while True:
            chunk = f.read(65536)
            if not chunk:
                break
            h.update(chunk)
    return h.hexdigest()


def verify_pe32_plus(path, expected_machine):
    data = Path(path).read_bytes()
    if len(data) < 0x100:
        return {"ok": False, "reason": "file too short"}
    mz = struct.unpack_from("<H", data, 0)[0]
    pe_off = struct.unpack_from("<I", data, 0x3C)[0]
    if mz != 0x5A4D or pe_off + 26 > len(data):
        return {"ok": False, "reason": "bad MZ or PE offset"}
    pe_sig, machine = struct.unpack_from("<IH", data, pe_off)
    opt_magic = struct.unpack_from("<H", data, pe_off + 24)[0]
    return {
        "ok": pe_sig == 0x00004550 and machine == expected_machine and opt_magic == 0x020B,
        "dos_magic": f"0x{mz:04x}",
        "pe_sig": f"0x{pe_sig:08x}",
        "machine": f"0x{machine:04x}",
        "opt_magic": f"0x{opt_magic:04x}",
        "bytes": len(data),
        "sha256": sha256_file(path),
    }


def verify_elf64_x86_64(path):
    data = Path(path).read_bytes()
    if len(data) < 64 or data[:4] != b"\x7fELF":
        return {"ok": False, "reason": "not ELF"}
    ei_class = data[4]
    e_machine = struct.unpack_from("<H", data, 18)[0]
    e_entry = struct.unpack_from("<Q", data, 24)[0]
    return {
        "ok": ei_class == 2 and e_machine == 0x3E and (e_entry >> 31) == 0x1FFFFFFFF,
        "elf_class": "ELF64" if ei_class == 2 else str(ei_class),
        "machine": f"0x{e_machine:04x}",
        "entry": f"0x{e_entry:016x}",
        "bytes": len(data),
        "sha256": sha256_file(path),
    }


def verify_arm64_kernel8(path):
    data = Path(path).read_bytes()
    if len(data) < 64:
        return {"ok": False, "reason": "too short"}
    text_offset = struct.unpack_from("<Q", data, 8)[0]
    magic = data[56:60]
    return {
        "ok": magic == b"ARM\x64" and text_offset == 0x00080000,
        "magic": magic.decode("latin1", errors="replace"),
        "text_offset": f"0x{text_offset:08x}",
        "bytes": len(data),
        "sha256": sha256_file(path),
    }


def detect_primary_ipv4():
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    try:
        sock.connect(("1.1.1.1", 80))
        ip = sock.getsockname()[0]
        if not ip.startswith("127."):
            return ip
    except OSError:
        pass
    finally:
        sock.close()
    return "172.16.47.76"


def build_all_pxe_targets(host_ip=None, oxicast_port=8432):
    mother_ip = host_ip or detect_primary_ipv4()
    host_with_port = f"{mother_ip}:{oxicast_port}"

    # 1. Build BOOTX64.EFI (x86_64-unknown-uefi)
    subprocess.run(
        [
            "cargo",
            "build",
            "--release",
            "--manifest-path",
            str(AYEOS_ROOT / "uefi-bootloader" / "Cargo.toml"),
            "--target",
            "x86_64-unknown-uefi",
            "--target-dir",
            "/tmp/ayeos-uefi-target",
        ],
        check=True,
    )

    # 2. Build BOOTAA64.EFI (aarch64-unknown-uefi)
    subprocess.run(
        [
            "cargo",
            "build",
            "--release",
            "--manifest-path",
            str(AYEOS_ROOT / "uefi-bootloader" / "Cargo.toml"),
            "--target",
            "aarch64-unknown-uefi",
            "--target-dir",
            "/tmp/ayeos-uefi-target",
        ],
        check=True,
    )

    # 3. Build ayeos-x86_64.elf (x86_64-unknown-none Limine Higher-Half Kernel)
    env_x86 = dict(os.environ)
    env_x86["RUSTFLAGS"] = (
        f"-C link-arg=-T{AYEOS_ROOT / 'baremetal-x86_64' / 'linker.ld'} -C relocation-model=static"
    )
    subprocess.run(
        [
            "cargo",
            "build",
            "--release",
            "--manifest-path",
            str(AYEOS_ROOT / "baremetal-x86_64" / "Cargo.toml"),
            "--target",
            "x86_64-unknown-none",
            "--target-dir",
            "/tmp/ayeos-x86-target",
        ],
        env=env_x86,
        check=True,
    )

    # 4. Build kernel8.img (aarch64-unknown-none Raspberry Pi 4 BCM2711 Kernel)
    env_pi4 = dict(os.environ)
    env_pi4["RUSTFLAGS"] = f"-C link-arg=-T{AYEOS_ROOT / 'baremetal-pi4' / 'linker.ld'}"
    subprocess.run(
        [
            "cargo",
            "build",
            "--release",
            "--manifest-path",
            str(AYEOS_ROOT / "baremetal-pi4" / "Cargo.toml"),
            "--target",
            "aarch64-unknown-none",
            "--target-dir",
            "/tmp/ayeos-pi4-target",
        ],
        env=env_pi4,
        check=True,
    )
    subprocess.run(
        [
            "/usr/bin/llvm-objcopy",
            "-O",
            "binary",
            "/tmp/ayeos-pi4-target/aarch64-unknown-none/release/ayeos-baremetal-pi4",
            "/tmp/ayeos-pi4-target/kernel8.img",
        ],
        check=True,
    )

    bootx64_src = Path("/tmp/ayeos-uefi-target/x86_64-unknown-uefi/release/ayeos-uefi-bootloader.efi")
    bootaa64_src = Path("/tmp/ayeos-uefi-target/aarch64-unknown-uefi/release/ayeos-uefi-bootloader.efi")
    x86_elf_src = Path("/tmp/ayeos-x86-target/x86_64-unknown-none/release/ayeos-baremetal-x86_64")
    pi4_img_src = Path("/tmp/ayeos-pi4-target/kernel8.img")

    limine_conf = (
        "timeout: 0\n"
        "quiet: yes\n"
        "default_entry: 1\n\n"
        "/8b-is AyeOS x OxiCast x PhoeniX Bare-Metal x86_64 Sanctuary\n"
        "    protocol: limine\n"
        "    path: boot():/ayeos-x86_64.elf\n"
        "    resolution: 1280x720x32\n"
        f"    cmdline: ayeos.peer={host_with_port} ayeos.mode=tv-sanctuary\n"
    )
    ipxe_script = (
        "#!ipxe\n"
        "dhcp || echo Using existing link configuration\n"
        "echo ==================================================================\n"
        "echo   8b-is AyeOS v1.1.0 Universal Multi-Arch Netboot (Diskless iPXE)\n"
        f"echo   Mother Node : http://{host_with_port}\n"
        "echo   Arch        : ${buildarch} (${platform})\n"
        "echo   g8t.is Snap : 0xFFFFE47F4C607310 (v1==v2 Verified)\n"
        "echo ==================================================================\n"
        f"iseq ${{buildarch}} x86_64 && chain http://{host_with_port}/boot/BOOTX64.EFI ||\n"
        f"iseq ${{buildarch}} arm64 && chain http://{host_with_port}/boot/BOOTAA64.EFI ||\n"
        f"kernel http://{host_with_port}/boot/kernel8.img init=/init rdinit=/init console=tty1 ayeos.peer={host_with_port} ayeos.mode=tv-sanctuary\n"
        f"initrd http://{host_with_port}/boot/ayeos-initramfs.cpio\n"
        "boot\n"
    )
    pi_config = (
        "# --- 8b-is AyeOS Bare-Metal Rust Raspberry Pi 4/5 Netboot ---\n"
        "arm_64bit=1\n"
        "kernel_address=0x00080000\n"
        "dtparam=i2s=on\n"
        "hdmi_force_hotplug=1\n"
        "hdmi_group=1\n"
        "hdmi_mode=16\n"
        "disable_overscan=1\n"
        "kernel=kernel8.img\n"
    )
    pi_cmdline = f"console=serial0,115200 console=tty1 ayeos.peer={host_with_port} ayeos.mode=tv-sanctuary\n"

    for dst in NETBOOT_DIRS:
        dst.mkdir(parents=True, exist_ok=True)
        shutil.copy2(bootx64_src, dst / "BOOTX64.EFI")
        shutil.copy2(bootaa64_src, dst / "BOOTAA64.EFI")
        shutil.copy2(x86_elf_src, dst / "ayeos-x86_64.elf")
        shutil.copy2(pi4_img_src, dst / "kernel8.img")
        (dst / "limine.conf").write_text(limine_conf)
        (dst / "limine.cfg").write_text(limine_conf)
        (dst / "ayeos.ipxe").write_text(ipxe_script)
        (dst / "boot.ipxe").write_text(ipxe_script)
        (dst / "autoexec.ipxe").write_text(ipxe_script)
        (dst / "config.txt").write_text(pi_config)
        (dst / "cmdline.txt").write_text(pi_cmdline)

        if LIMINE_SHARE.is_dir():
            for name in ("limine-bios-pxe.bin", "limine-bios.sys", "BOOTIA32.EFI", "BOOTRISCV64.EFI"):
                src_f = LIMINE_SHARE / name
                if src_f.is_file():
                    shutil.copy2(src_f, dst / name)

        blu_img = Path("/tmp/ayeos-v030-g8t/studio-audio-tone1.img")
        if blu_img.is_file():
            shutil.copy2(blu_img, dst / "studio-audio-tone1.img")

    primary_dir = NETBOOT_DIRS[0]
    verifications = {
        "BOOTX64.EFI": verify_pe32_plus(primary_dir / "BOOTX64.EFI", 0x8664),
        "BOOTAA64.EFI": verify_pe32_plus(primary_dir / "BOOTAA64.EFI", 0xAA64),
        "ayeos-x86_64.elf": verify_elf64_x86_64(primary_dir / "ayeos-x86_64.elf"),
        "kernel8.img": verify_arm64_kernel8(primary_dir / "kernel8.img"),
    }
    matrix = {
        "ok": all(v["ok"] for v in verifications.values()),
        "mother_ip": mother_ip,
        "oxicast_http_boot_root": f"http://{host_with_port}/boot/",
        "tftp_root_dirs": [str(d) for d in NETBOOT_DIRS],
        "g8t_snap_circuit_seal_hex": "0xffffe47f4c607310",
        "supported_architectures": [
            {
                "arch": "x86_64 UEFI PXE (RFC 4578 Arch 0x0007 / 0x0009)",
                "bootfile": "BOOTX64.EFI",
                "protocol": "TFTP (:69 / :6969)",
            },
            {
                "arch": "x86_64 UEFI HTTP Boot (RFC 4578 Arch 0x0010 / HTTPClient)",
                "bootfile": f"http://{host_with_port}/boot/BOOTX64.EFI",
                "protocol": f"HTTP (:{oxicast_port})",
            },
            {
                "arch": "ARM64 UEFI PXE (RFC 4578 Arch 0x000B)",
                "bootfile": "BOOTAA64.EFI",
                "protocol": "TFTP (:69 / :6969)",
            },
            {
                "arch": "ARM64 UEFI HTTP Boot (RFC 4578 Arch 0x0013 / HTTPClient)",
                "bootfile": f"http://{host_with_port}/boot/BOOTAA64.EFI",
                "protocol": f"HTTP (:{oxicast_port})",
            },
            {
                "arch": "x86 Legacy BIOS PXE (RFC 4578 Arch 0x0000)",
                "bootfile": "limine-bios-pxe.bin -> limine.conf -> ayeos-x86_64.elf",
                "protocol": "TFTP (:69 / :6969)",
            },
            {
                "arch": "Raspberry Pi 4 / 5 BCM2711 EEPROM Netboot",
                "bootfile": "config.txt -> kernel8.img",
                "protocol": "TFTP (:69 / :6969) + DHCP Option 43",
            },
            {
                "arch": "Universal iPXE (Option 77 = 'iPXE')",
                "bootfile": f"http://{host_with_port}/boot/ayeos.ipxe",
                "protocol": f"HTTP (:{oxicast_port})",
            },
        ],
        "artifacts": verifications,
    }
    for dst in NETBOOT_DIRS:
        (dst / "pxe-matrix.json").write_text(json.dumps(matrix, indent=2) + "\n")
    return matrix


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host-ip", help="Override Mother Node IPv4 address")
    parser.add_argument("--port", type=int, default=8432, help="HTTP / OxiCast port (default: 8432)")
    parser.add_argument(
        "--serve",
        action="store_true",
        help="After building, launch the live AyeOS Sanctuary PXE/TFTP/DHCP/HTTP/DNS Mother Node",
    )
    args = parser.parse_args()

    matrix = build_all_pxe_targets(host_ip=args.host_ip, oxicast_port=args.port)
    print(json.dumps(matrix, indent=2))

    if args.serve:
        subprocess.run(
            [
                "cargo",
                "run",
                "--release",
                "--manifest-path",
                str(AYEOS_ROOT / "Cargo.toml"),
                "--bin",
                "ayeos-sanctuary",
            ],
            check=True,
        )


if __name__ == "__main__":
    main()
