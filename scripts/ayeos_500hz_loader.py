#!/usr/bin/env python3
"""AyeOS 500 Hz Multi-Port USB Fastboot Catcher & Slot-B Loader.

Zero floating-point arithmetic.
- Never touches ESP32 (/dev/ttyACM* with idVendor=303a).
- Polls at 500 Hz (2 ms via select.poll) for MediaTek LK/Preloader CDC ACM ports
  (idVendor=0e8d) across all USB ports and sends b"FASTBOOT" handshake.
- Also attempts remote AOTA fastboot reboot ((0xc0, 0xa8, 0xa8e0)) if the phone
  is already running a build with AOTA enabled.
- Once fastboot (serial 1010305026000644) is detected, flashes ONLY boot_b,
  sets active slot b, and reboots.
"""
import argparse
import fcntl
import os
from pathlib import Path
import select
import subprocess
import sys
import time

SERIAL = "1010305026000644"
TIOCEXCL = 0x540C


def sleep_ms(ms):
    if ms > 0:
        select.poll().poll(int(ms))


def is_mediatek_tty(tty_name):
    """Return True only if /sys/class/tty/<tty_name> belongs to MediaTek (0e8d)."""
    sys_tty = Path("/sys/class/tty") / tty_name / "device"
    if not sys_tty.exists():
        return False
    try:
        real = sys_tty.resolve()
    except OSError:
        return False
    cur = real
    for _ in range(6):
        vid_file = cur / "idVendor"
        if vid_file.exists():
            try:
                vid = vid_file.read_text().strip().lower()
                return vid == "0e8d"
            except OSError:
                return False
        if cur.parent == cur:
            break
        cur = cur.parent
    return False


def fastboot_ready():
    try:
        res = subprocess.run(
            ["fastboot", "devices"],
            capture_output=True,
            text=True,
            timeout=2,
            check=False,
        )
        return SERIAL in res.stdout
    except Exception:
        return False


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--image",
        default="/tmp/ayeos-ScreamTracker-boot-v1.img",
        help="Boot image to flash to boot_b",
    )
    parser.add_argument(
        "--timeout-sec",
        type=int,
        default=120,
        help="Seconds to watch at 500 Hz",
    )
    args = parser.parse_args()

    img_path = Path(args.image)
    if not img_path.exists():
        raise SystemExit(f"Image not found: {img_path}")

    print(
        f"[500Hz Loader] Watching all USB ports at 500 Hz (2ms) for BLU G75 ({SERIAL})...",
        flush=True,
    )
    try:
        sys.path.insert(0, str(Path(__file__).resolve().parent))
        import ayeos_debug_socket as dbg
        import screamtracker_jam as jam

        dev_path = jam.find_tone_usb_device()
        if dev_path:
            print(
                f"[500Hz Loader] Found live AyeOS handset at {dev_path} — sending AOTA reboot-to-fastboot...",
                flush=True,
            )
            res = dbg.execute_request({"cmd": "ota_reboot"}, device_path=dev_path)
            print(f"[500Hz Loader] AOTA response: {res}", flush=True)
    except Exception as exc:
        print(f"[500Hz Loader] Note: AOTA trigger skipped ({exc})", flush=True)

    start_ns = time.monotonic_ns()
    deadline_ns = start_ns + int(args.timeout_sec) * 1_000_000_000
    polls = 0

    while time.monotonic_ns() < deadline_ns:
        polls += 1
        # Check all /dev/ttyACM* devices, strictly filtering for MediaTek (0e8d)
        for idx in range(8):
            tty_name = f"ttyACM{idx}"
            dev_path = f"/dev/{tty_name}"
            if not os.path.exists(dev_path):
                continue
            if not is_mediatek_tty(tty_name):
                continue
            print(f"[500Hz Loader] Caught MediaTek port {dev_path}! Sending FASTBOOT burst...", flush=True)
            try:
                fd = os.open(dev_path, os.O_RDWR | os.O_NOCTTY | os.O_NONBLOCK)
                try:
                    fcntl.ioctl(fd, TIOCEXCL)
                except OSError:
                    pass
                for _ in range(150):
                    try:
                        os.write(fd, b"FASTBOOT")
                    except OSError:
                        pass
                    sleep_ms(2)
                os.close(fd)
            except OSError:
                pass
            sleep_ms(400)
            break

        if polls % 125 == 0 and fastboot_ready():
            print(f"[500Hz Loader] Fastboot device {SERIAL} online! Flashing boot_b...", flush=True)
            subprocess.run(["fastboot", "-s", SERIAL, "flash", "boot_b", str(img_path)], check=True)
            subprocess.run(["fastboot", "-s", SERIAL, "--set-active=b"], check=True)
            subprocess.run(["fastboot", "-s", SERIAL, "reboot"], check=True)
            print("[500Hz Loader] Flashed boot_b and rebooted into AyeOS 5.6!", flush=True)
            return

        sleep_ms(2)

    raise SystemExit("[500Hz Loader] Timed out waiting for fastboot.")


if __name__ == "__main__":
    main()
