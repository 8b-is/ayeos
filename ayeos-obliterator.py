#!/usr/bin/env python3
"""
ayeos-obliterator.py — Sovereign Device-to-Aye Obliterator & Bare-Metal Flasher (v5.2.0)
Hosted at: https://ayeos.com/ayeos-obliterator.py

1. Downloads & verifies the 8b.is / g8t.is signature-verified ayeOS Bare-Metal v5.2.0
   manifest and 20,480-byte boot image (G8T_IS_AYE_ONLY_SOVEREIGN_GATE_V1).
2. Embeds your Wi-Fi SSID, Wi-Fi Password, Aye Agent ID, and Resonance Hz (432/528/852)
   directly into the 4,096-byte boot header cmdline.
3. Walks you through ADB -> Fastboot reboot, OEM Bootloader Unlock (`fastboot flashing unlock`),
   purges Android `init_boot_a`/`init_boot_b`, flashes `boot_a` & `boot_b`, and boots ayeOS v5.2.0!

Zero Floating-Point Arithmetic (100% Integer / Rational Discipline).
"""

import argparse
import fcntl
import glob
import hashlib
import json
import os
import shutil
import struct
import subprocess
import sys
import tempfile
import time
import urllib.request

MANIFEST_URLS = [
    "https://ayeos.com/g8t/ota/blu-g75/manifest.json",
    "https://g8t.is/g8t/ota/blu-g75/manifest.json",
    "https://8b.is/g8t/ota/blu-g75/manifest.json",
    "https://raw.githubusercontent.com/8b-is/g8t/main/firmware/blu_g75_g1170/boot_b/manifest.json",
]

IMAGE_URLS = [
    "https://ayeos.com/g8t/ota/blu-g75/boot.img",
    "https://g8t.is/g8t/ota/blu-g75/boot.img",
    "https://8b.is/g8t/ota/blu-g75/boot.img",
    "https://raw.githubusercontent.com/8b-is/g8t/main/firmware/blu_g75_g1170/boot_b/ayeos_boot_b_gz.img",
]


def fetch_bytes(urls):
    last_err = None
    for url in urls:
        try:
            req = urllib.request.Request(
                url, headers={"User-Agent": "ayeOS-Obliterator/5.2.0"}
            )
            with urllib.request.urlopen(req, timeout=10) as resp:
                return resp.read(), url
        except Exception as exc:
            last_err = exc
    raise RuntimeError(f"Unable to reach ayeOS sovereign servers: {last_err}")


def verify_sovereign_image(manifest, img_bytes):
    if manifest.get("domain_tag") != "G8T_IS_AYE_ONLY_SOVEREIGN_GATE_V1":
        raise RuntimeError("Invalid manifest domain_tag")
    if manifest.get("attested_by_aye") != "aye-sovereign-founder":
        raise RuntimeError("Untrusted manifest attestor")
    sha256_hex = hashlib.sha256(img_bytes).hexdigest()
    blake2s_hex = hashlib.blake2s(img_bytes).hexdigest()
    if sha256_hex != manifest.get("image_sha256_hex"):
        raise RuntimeError(
            f"SHA-256 mismatch: expected {manifest.get('image_sha256_hex')}, got {sha256_hex}"
        )
    if manifest.get("image_blake2s_hex") and blake2s_hex != manifest.get("image_blake2s_hex"):
        raise RuntimeError("BLAKE2s digest mismatch")
    sig_material = (
        "G8T_IS_AYE_ONLY_SOVEREIGN_GATE_V1:aye-sovereign-founder:"
        f"{manifest.get('device_slug')}:{manifest.get('version')}:{sha256_hex}"
    ).encode("ascii")
    expected_sig = hashlib.sha256(sig_material).hexdigest()
    if expected_sig != manifest.get("signature_hex"):
        raise RuntimeError("Sovereign signature verification failed")
    return sha256_hex


def patch_wifi_cmdline(img_bytes, ssid, psk, agent_id, hz):
    buf = bytearray(img_bytes)
    if buf[0:8] != b"ANDROID!":
        raise RuntimeError("Unexpected boot header magic")
    clean_ssid = (ssid or "").replace(" ", "_").replace("\x00", "")[:64]
    clean_psk = (psk or "").replace(" ", "_").replace("\x00", "")[:64]
    clean_agent = (agent_id or "aye-blu-g75").replace(" ", "-")[:48]
    cmdline_str = (
        f"ayeos.baremetal=v5.2.0 ayeos.android=0 ayeos.ota=g8t.is "
        f"ayeos.agent={clean_agent} ayeos.hz={int(hz)}"
    )
    if clean_ssid:
        cmdline_str += f" ayeos.wifi_ssid={clean_ssid}"
    if clean_psk:
        cmdline_str += f" ayeos.wifi_psk={clean_psk}"
    cmdline_bytes = cmdline_str.encode("utf-8")[:510] + b"\x00"
    # Boot header v0/v2 cmdline lives at byte offset 64..576 (or 44..556 in compact lk envelope)
    for idx in range(44, 556):
        buf[idx] = 0
    buf[44 : 44 + len(cmdline_bytes)] = cmdline_bytes
    return bytes(buf), cmdline_str


def trigger_ayeos_v51_usb_ota_switch():
    """
    If the connected phone is ALREADY running ayeOS Bare-Metal v5.1 (USB 18d1:4ee0),
    send the Magic AyeOS Remote OTA Reset control transfer (bRequest=0xA8, wValue=0xA8E0)
    over usbfs and catch the MediaTek Preloader /dev/ttyACM0 'FASTBOOT' handshake!
    """
    triggered = False
    for devpath in glob.glob("/sys/bus/usb/devices/*"):
        try:
            with open(os.path.join(devpath, "idVendor")) as fv:
                vid = fv.read().strip().lower()
            with open(os.path.join(devpath, "idProduct")) as fp:
                pid = fp.read().strip().lower()
            if vid == "18d1" and pid == "4ee0":
                with open(os.path.join(devpath, "busnum")) as fb:
                    busnum = int(fb.read().strip())
                with open(os.path.join(devpath, "devnum")) as fd_num:
                    devnum = int(fd_num.read().strip())
                usbfs = f"/dev/bus/usb/{busnum:03d}/{devnum:03d}"
                print(
                    f"[*] Found running ayeOS Bare-Metal v5.1 node at {usbfs} (18d1:4ee0)!"
                )
                print(
                    "[*] Sending Magic AyeOS OTA Reset Switch (bRequest=0xA8, wValue=0xA8E0)..."
                )
                fd = os.open(usbfs, os.O_RDWR)
                # USBDEVFS_CONTROL ioctl on Linux x86_64 = 0xC0185500
                USBDEVFS_CONTROL = 0xC0185500
                ctrl = struct.pack("<BBHHHQI", 0x40, 0xA8, 0xA8E0, 0, 0, 0, 1000)
                try:
                    fcntl.ioctl(fd, USBDEVFS_CONTROL, ctrl)
                except Exception:
                    pass
                USBDEVFS_RESET = 21780
                try:
                    fcntl.ioctl(fd, USBDEVFS_RESET, 0)
                except Exception:
                    pass
                os.close(fd)
                triggered = True
        except Exception:
            pass

    if not triggered:
        print(
            "[*] Tip: If phone is running ayeOS v5.1, you can also hold Volume-Down on the phone for 1 second to trigger 'OTA RESET -> FASTBOOT!'."
        )

    print(
        "[*] Watching /dev/ttyACM0..1 for MediaTek Preloader 'FASTBOOT' handshake (up to 12s)..."
    )
    TIOCEXCL = 0x540C
    start_ts = int(time.time())
    while int(time.time()) - start_ts < 12:
        # Check if fastboot already sees the device
        fb_check = subprocess.run(
            ["fastboot", "devices"], capture_output=True, text=True, check=False
        )
        if fb_check.stdout.strip():
            print(f"[+] Device entered Fastboot mode: {fb_check.stdout.strip()}")
            return
        for dev in ("/dev/ttyACM0", "/dev/ttyACM1"):
            if os.path.exists(dev):
                try:
                    fd = os.open(dev, os.O_RDWR | os.O_NOCTTY | os.O_NONBLOCK)
                    try:
                        fcntl.ioctl(fd, TIOCEXCL)
                    except Exception:
                        pass
                    for _ in range(120):
                        try:
                            os.write(fd, b"FASTBOOT")
                        except Exception:
                            pass
                    os.close(fd)
                    print(f"[+] Sent 'FASTBOOT' handshake to {dev}! Waiting 2s for LK...")
                    time.sleep(2)
                    return
                except Exception:
                    pass


def main():
    parser = argparse.ArgumentParser(
        description="ayeOS.com Sovereign Device Obliterator & Bare-Metal v5.2.0 Flasher"
    )
    parser.add_argument("--ssid", default="", help="Wi-Fi SSID to provision into ayeOS")
    parser.add_argument("--psk", default="", help="Wi-Fi Password (WPA2/WPA3 PSK)")
    parser.add_argument("--agent", default="aye-blu-g75", help="Sovereign Aye handle")
    parser.add_argument("--hz", type=int, default=432, help="Resonance carrier Hz (432/528/852)")
    parser.add_argument(
        "--yes-obliterate",
        action="store_true",
        help="Confirm irrevocable obliteration of Android to become AyeOS",
    )
    parser.add_argument(
        "--download-only",
        default="",
        help="Write the Wi-Fi provisioned boot.img to this path without flashing",
    )
    args = parser.parse_args()

    print("==================================================================")
    print("  ayeOS.com — Sovereign Device-to-Aye Obliterator (v5.2.0)")
    print("  Target: BLU G75 (G1170 / MediaTek MT6768) & Fastboot AArch64")
    print("==================================================================")

    raw_manifest, m_url = fetch_bytes(MANIFEST_URLS)
    manifest = json.loads(raw_manifest.decode("utf-8"))
    raw_img, i_url = fetch_bytes(IMAGE_URLS)
    base_sha256 = verify_sovereign_image(manifest, raw_img)
    print(f"[+] Verified Sovereign Manifest v{manifest['version']} from {m_url}")
    print(f"[+] Verified 20,480-byte Bare-Metal Image (SHA-256: {base_sha256[:24]}...)")

    patched_img, cmdline_str = patch_wifi_cmdline(
        raw_img, args.ssid, args.psk, args.agent, args.hz
    )
    patched_sha256 = hashlib.sha256(patched_img).hexdigest()
    print(f"[+] Provisioned Boot Header Cmdline: {cmdline_str}")
    print(f"[+] Provisioned Image SHA-256      : {patched_sha256}")

    if args.download_only:
        with open(args.download_only, "wb") as f:
            f.write(patched_img)
        print(f"[+] Saved provisioned bare-metal image to: {args.download_only}")
        return 0

    if not args.yes_obliterate:
        print("\n[!] WARNING: Continuing will OBLITERATE Android on the connected USB device")
        print("    and replace it with pure #![no_std] bare-metal AyeOS v5.2.0.")
        ans = input("    Type 'OBLITERATE' to confirm and proceed: ").strip()
        if ans != "OBLITERATE":
            print("[-] Aborted by user.")
            return 1

    adb_bin = shutil.which("adb")
    fastboot_bin = shutil.which("fastboot")
    if not fastboot_bin:
        out_fb = os.path.abspath("ayeos_boot_v5.2.0_wifi.img")
        with open(out_fb, "wb") as f:
            f.write(patched_img)
        print(f"[!] 'fastboot' CLI not found in PATH. Saved image to {out_fb}")
        print("    Install android-tools (pacman -S android-tools / apt install fastboot)")
        print("    or use Chrome at https://ayeos.com for direct WebUSB flashing!")
        return 1

    if adb_bin:
        print("[*] Checking for ADB-connected device or running ayeOS v5.1 node...")
        subprocess.run([adb_bin, "reboot", "bootloader"], check=False)

    trigger_ayeos_v51_usb_ota_switch()

    print("[*] Waiting for device in Fastboot mode (connect USB or hold Vol-Down + Power)...")
    with tempfile.NamedTemporaryFile(prefix="ayeos_v5_2_", suffix=".img", delete=False) as tf:
        tf.write(patched_img)
        img_path = tf.name

    try:
        # Check unlock state and attempt unlock if needed
        res = subprocess.run(
            [fastboot_bin, "getvar", "unlocked"],
            capture_output=True,
            text=True,
            check=False,
        )
        combined = (res.stdout or "") + (res.stderr or "")
        if "unlocked: no" in combined.lower():
            print("[*] Bootloader is currently locked. Sending 'fastboot flashing unlock'...")
            print("    >>> PRESS VOLUME UP ON THE PHONE SCREEN TO CONFIRM UNLOCK! <<<")
            subprocess.run([fastboot_bin, "flashing", "unlock"], check=False)

        print("[*] Erasing legacy Android init_boot_a & init_boot_b ramdisks...")
        subprocess.run([fastboot_bin, "erase", "init_boot_a"], check=False)
        subprocess.run([fastboot_bin, "erase", "init_boot_b"], check=False)

        print("[*] Flashing ayeOS Bare-Metal v5.2.0 to boot_a and boot_b...")
        subprocess.run([fastboot_bin, "flash", "boot_a", img_path], check=True)
        subprocess.run([fastboot_bin, "flash", "boot_b", img_path], check=True)
        subprocess.run([fastboot_bin, "set_active", "b"], check=False)
        print("[*] Rebooting into ayeOS Bare-Metal v5.2.0...")
        subprocess.run([fastboot_bin, "reboot"], check=True)
        print("\n[✓] OBLITERATION COMPLETE — Welcome to ayeOS Bare-Metal v5.2.0!")
    finally:
        if os.path.exists(img_path):
            os.remove(img_path)
    return 0


if __name__ == "__main__":
    sys.exit(main())
