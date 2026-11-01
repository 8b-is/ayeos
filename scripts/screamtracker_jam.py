#!/usr/bin/env python3
"""AyeOS 5.6 ScreamTracker (S3M) + Piano Keyboard + ESP32 Mothership Jam.

Zero floating-point arithmetic. Linux stdlib only.
Communicates with the AyeOS Phone (18d1:4ee0 bcdDevice=0562) via USB EP0
vendor request (0xc0, 0xb8) and optionally conducts the phone from live
rational (~ p/q) wave telemetry emitted by a connected ESP32 (e.g. VAK:MEDUSA
on /dev/ttyACM0).
"""
import argparse
import fcntl
import json
import os
from pathlib import Path
import select
import struct
import sys
import termios
import time
import tty

sys.path.insert(0, str(Path(__file__).resolve().parent))

import capture_board4 as b4
import capture_tone as ct

OFF_MASK = ct.OFF_MASK

GAIN_CODE_BY_DB = {
    0: 0,
    4: 1,
    8: 2,
    -4: 3,
    -10: 4,
}

SPK_CODE_BY_NAME = {
    "both": 3,
    "left": 1,
    "right": 2,
}

KEY_MAP = {
    "z": (16, "E-4", 375),
    "s": (15, "F-4", 400),
    "x": (14, "F#4", 428),
    "d": (13, "G-4", 461),
    "c": (12, "B-4", 500),
    "v": (11, "C-5", 545),
    "g": (10, "D-5", 600),
    "b": (9, "E-5", 666),
    "n": (8, "F#5", 750),
    "j": (7, "A-5", 857),
    "m": (6, "B-5", 1000),
    ",": (5, "D-6", 1200),
    "l": (4, "F#6", 1500),
    ".": (3, "B-6", 2000),
}


def sleep_ms(ms):
    if ms > 0:
        select.poll().poll(int(ms))


class JamUsb(ct.Usb):
    ALLOWED = ct.Usb.ALLOWED + ((0xc0, 0xb8),)

    def __init__(self, path, seconds):
        self.path = path
        super().__init__(path, seconds)

    def reset_reopen(self):
        try:
            fcntl.ioctl(self.fd, 21780, 0)
        except OSError:
            pass
        try:
            os.close(self.fd)
        except OSError:
            pass
        sleep_ms(550)
        self.path = find_tone_usb_device() or self.path
        self.deadline = time.monotonic_ns() + 60 * 1_000_000_000
        self.fd = os.open(self.path, os.O_RDWR | os.O_CLOEXEC | os.O_NOFOLLOW)

    def read(self, kind, request, value=0, index=0, length=64):
        self.deadline = time.monotonic_ns() + 60 * 1_000_000_000
        last_err = None
        for _ in range(3):
            try:
                return super().read(kind, request, value, index, length)
            except OSError as exc:
                last_err = exc
                self.reset_reopen()
        raise last_err


def find_tone_usb_device():
    sys_usb = Path("/sys/bus/usb/devices")
    if not sys_usb.exists():
        return None
    matches = []
    for entry in sorted(sys_usb.iterdir()):
        vid_p = entry / "idVendor"
        pid_p = entry / "idProduct"
        bcd_p = entry / "bcdDevice"
        bus_p = entry / "busnum"
        dev_p = entry / "devnum"
        desc_p = entry / "descriptors"
        if not (vid_p.exists() and pid_p.exists() and bcd_p.exists()
                and bus_p.exists() and dev_p.exists()):
            continue
        try:
            vid = vid_p.read_text().strip().lower()
            pid = pid_p.read_text().strip().lower()
            bcd = bcd_p.read_text().strip().lower()
            busnum = int(bus_p.read_text().strip(), 10)
            devnum = int(dev_p.read_text().strip(), 10)
            if desc_p.exists():
                raw = desc_p.read_bytes()
                if len(raw) >= 36 and (raw[:18] != ct.DEVICE or raw[18:36] != b4.CONFIG):
                    continue
        except OSError:
            continue
        if vid == "18d1" and pid == "4ee0" and bcd == "0562":
            cand = f"/dev/bus/usb/{busnum:03d}/{devnum:03d}"
            if os.path.exists(cand):
                matches.append(((busnum, devnum), cand))
    if not matches:
        return None
    matches.sort()
    return matches[-1][1]


def decode_atrk(packet):
    if len(packet) != 64 or packet[:4] != b"ATRK":
        raise ValueError("invalid ATRK response packet")
    version, plays, last_cmd, active_row, pending, outcome, off = struct.unpack_from(
        "<IIHBBII", packet, 4
    )
    if version != 1 or packet[24:36] != b"s3m-tracker\0":
        raise ValueError("unexpected ATRK version or label")
    return {
        "protocol": "s3m-tracker-v1",
        "plays": plays,
        "last_cmd": f"0x{last_cmd:04x}",
        "active_row": active_row,
        "pending": bool(pending),
        "last_outcome": outcome,
        "last_off_mask": f"0x{off:05x}",
        "verified_off": off == OFF_MASK,
    }


def encode_note_index(gain_db=4, spk="both", duration_ms=140):
    gain_code = GAIN_CODE_BY_DB.get(gain_db, 1)
    spk_code = SPK_CODE_BY_NAME.get(spk, 3)
    dur_10ms = max(2, min(150, int(duration_ms) // 10))
    return (dur_10ms << 8) | ((gain_code & 7) << 2) | (spk_code & 3)


def trigger_and_wait(usb, cmd_value, cmd_index=0, timeout_ms=4000):
    before = decode_atrk(usb.read(0xc0, 0xb8, 0, 0, 64))
    target_plays = before["plays"] + 1
    usb.read(0xc0, 0xb8, cmd_value, cmd_index, 64)
    # Wait for the single bare-metal CPU to finish tone::play_steps + framebuffer flush
    # before sending EP0 status polls so the USB 2.0 Hub TT never aborts a split transfer.
    initial_wait_ms = 2350 if cmd_value == 0x0080 else 420
    sleep_ms(initial_wait_ms)
    start_ns = time.monotonic_ns()
    deadline_ns = start_ns + int(timeout_ms) * 1_000_000
    while time.monotonic_ns() < deadline_ns:
        try:
            status = decode_atrk(usb.read(0xc0, 0xb8, 0, 0, 64))
        except OSError:
            sleep_ms(120)
            continue
        if status["plays"] >= target_plays and not status["pending"]:
            return status
        sleep_ms(120)
    return decode_atrk(usb.read(0xc0, 0xb8, 0, 0, 64))


def parse_rational(token):
    if "/" not in token:
        return int(token, 10), 1
    p_str, q_str = token.split("/", 1)
    p = int(p_str, 10)
    q = int(q_str, 10)
    if q <= 0:
        raise ValueError("denominator must be positive")
    return p, q


def map_esp32_wave_to_note(line):
    """Map an ESP32 VAK:MEDUSA rational wave line (~ f_p/f_q ph_p/ph_q harm trit sal_p/sal_q)
    to an AyeOS SGEN note using strictly integer/rational math.
    """
    parts = line.strip().split()
    if len(parts) < 6 or parts[0] != "~":
        return None
    try:
        fp, fq = parse_rational(parts[1])
        _php, _phq = parse_rational(parts[2])
        harmonic = int(parts[3], 10)
        trit = int(parts[4], 10)
        sal_p, sal_q = parse_rational(parts[5])
    except ValueError:
        return None

    # Map rational frequency + harmonic into SGEN divider [3..12] (500Hz..2000Hz)
    # Using exact cross-multiplied integer buckets:
    scaled = (abs(fp) * 16) // fq + max(0, harmonic)
    div_table = (12, 10, 8, 6, 5, 4, 3, 6)
    div = div_table[scaled % len(div_table)]
    hz = 6000 // div

    # Map balanced ternary trit {-1, 0, +1} to Left / Both / Right AW87394 PA:
    if trit < 0:
        spk = "left"
    elif trit > 0:
        spk = "right"
    else:
        spk = "both"

    # Map rational salience sal_p / sal_q to gain_db {0, +4, +8}:
    if sal_p * 10 >= sal_q * 8:
        gain_db = 8
    elif sal_p * 10 >= sal_q * 5:
        gain_db = 4
    else:
        gain_db = 0

    return {
        "raw": line.strip(),
        "div": div,
        "hz": hz,
        "spk": spk,
        "gain_db": gain_db,
        "trit": trit,
        "salience": f"{sal_p}/{sal_q}",
    }


def open_esp32_serial(path):
    fd = os.open(path, os.O_RDONLY | os.O_NOCTTY | os.O_NONBLOCK)
    attrs = termios.tcgetattr(fd)
    attrs[0] = 0
    attrs[1] = 0
    attrs[2] = termios.CS8 | termios.CREAD | termios.CLOCAL | termios.B115200
    attrs[3] = 0
    attrs[4] = termios.B115200
    attrs[5] = termios.B115200
    termios.tcsetattr(fd, termios.TCSANOW, attrs)
    return fd


def run_esp32_mothership(usb, esp32_tty, steps=6, duration_ms=140):
    fd = open_esp32_serial(esp32_tty)
    poller = select.poll()
    poller.register(fd, select.POLLIN)
    buf = b""
    played = []
    start_ns = time.monotonic_ns()
    deadline_ns = start_ns + 15_000_000_000
    try:
        while len(played) < steps and time.monotonic_ns() < deadline_ns:
            events = poller.poll(200)
            if not events:
                continue
            try:
                chunk = os.read(fd, 512)
            except BlockingIOError:
                continue
            if not chunk:
                continue
            buf += chunk
            while b"\n" in buf:
                raw_line, buf = buf.split(b"\n", 1)
                text = raw_line.decode("utf-8", errors="replace").strip()
                mapped = map_esp32_wave_to_note(text)
                if mapped is None:
                    continue
                idx = encode_note_index(
                    gain_db=mapped["gain_db"],
                    spk=mapped["spk"],
                    duration_ms=duration_ms,
                )
                status = trigger_and_wait(usb, mapped["div"], idx, timeout_ms=3000)
                mapped["phone_status"] = status
                played.append(mapped)
                print(
                    f"[MOTHERSHIP {len(played)}/{steps}] ESP32({mapped['raw']}) -> "
                    f"Phone {mapped['hz']}Hz (div={mapped['div']}) "
                    f"gain={mapped['gain_db']:+d}dB spk={mapped['spk']} "
                    f"verified_off={status['verified_off']}",
                    flush=True,
                )
                if len(played) >= steps:
                    break
    finally:
        os.close(fd)
    return played


def run_interactive(usb, esp32_tty):
    print("=== AyeOS 5.6 ScreamTracker & Music Keyboard Live Jam ===")
    print("Keys: [z s x d c v g b n j m , l .] -> Play Piano Note (+8dB)")
    print("      [1..8]                        -> Play ScreamTracker Row 00..07")
    print("      [SPACE]                       -> Play Full 8-Row S3M Song")
    print("      [e]                           -> Conduct 4 notes from ESP32 VAK:MEDUSA")
    print("      [q]                           -> Quit")
    fd = sys.stdin.fileno()
    old = termios.tcgetattr(fd)
    try:
        tty.setraw(fd)
        while True:
            ch = sys.stdin.read(1)
            if ch in ("q", "\x03", "\x04"):
                break
            try:
                if ch == " ":
                    sys.stdout.write("\r\nPlaying 8-row ScreamTracker S3M song...\r\n")
                    status = trigger_and_wait(usb, 0x0080, 0, timeout_ms=5000)
                    sys.stdout.write(f"Result: {json.dumps(status)}\r\n")
                elif ch in "12345678":
                    row = int(ch, 10) - 1
                    sys.stdout.write(f"\r\nPlaying Tracker Row {row:02d}...\r\n")
                    status = trigger_and_wait(usb, 0x0081 + row, 0, timeout_ms=3000)
                    sys.stdout.write(f"Result: {json.dumps(status)}\r\n")
                elif ch == "e":
                    sys.stdout.write(f"\r\nListening to ESP32 on {esp32_tty}...\r\n")
                    run_esp32_mothership(usb, esp32_tty, steps=4, duration_ms=140)
                elif ch in KEY_MAP:
                    div, name, hz = KEY_MAP[ch]
                    idx = encode_note_index(gain_db=8, spk="both", duration_ms=150)
                    sys.stdout.write(f"\r\nNote {name} ({hz} Hz, div={div}, +8dB)...\r\n")
                    status = trigger_and_wait(usb, div, idx, timeout_ms=3000)
                    sys.stdout.write(f"Result: {json.dumps(status)}\r\n")
            except OSError as exc:
                sys.stdout.write(f"\r\n[USB bus re-syncing after note ({exc.strerror})...]\r\n")
                usb.reset_reopen()
    finally:
        termios.tcsetattr(fd, termios.TCSADRAIN, old)
        print()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--device", help="USB device path (/dev/bus/usb/BBB/DDD); auto-detected if omitted")
    parser.add_argument("--status", action="store_true", help="Read ATRK tracker status")
    parser.add_argument("--play-song", action="store_true", help="Play the 8-row ScreamTracker S3M pattern")
    parser.add_argument("--row", type=int, choices=range(8), help="Play a single ScreamTracker row (0..7)")
    parser.add_argument("--note", type=int, choices=range(1, 32), help="Play custom SGEN divider (1..31, 6000/div Hz)")
    parser.add_argument("--gain-db", type=int, choices=(-10, -4, 0, 4, 8), default=8, help="Lineout PGA gain in dB")
    parser.add_argument("--spk", choices=("both", "left", "right"), default="both", help="Speaker PA selection")
    parser.add_argument("--duration-ms", type=int, default=160, help="Note duration in ms (20..1500)")
    parser.add_argument("--esp32-mothership", action="store_true", help="Conduct phone from live ESP32 ~ p/q waves")
    parser.add_argument("--esp32-tty", default="/dev/ttyACM0", help="ESP32 serial device (default: /dev/ttyACM0)")
    parser.add_argument("--steps", type=int, default=6, help="Number of ESP32 wave steps to play")
    parser.add_argument("--interactive", action="store_true", help="Interactive keyboard mode")
    args = parser.parse_args()

    device_path = args.device or find_tone_usb_device()
    if not device_path:
        raise SystemExit("No AyeOS audio-tone1 USB device (18d1:4ee0 bcdDevice=0562) found.")

    usb = JamUsb(device_path, 60)
    try:
        dev_desc = usb.read(0x80, 6, 0x0100, length=18)
        cfg_desc = usb.read(0x80, 6, 0x0200, length=18)
        if dev_desc != ct.DEVICE or cfg_desc != b4.CONFIG:
            raise ValueError("USB device descriptor does not match audio-tone1 (0562)")
        if args.play_song:
            res = trigger_and_wait(usb, 0x0080, 0, timeout_ms=5000)
            print(json.dumps(res, indent=2))
        elif args.row is not None:
            res = trigger_and_wait(usb, 0x0081 + args.row, 0, timeout_ms=3000)
            print(json.dumps(res, indent=2))
        elif args.note is not None:
            idx = encode_note_index(args.gain_db, args.spk, args.duration_ms)
            res = trigger_and_wait(usb, args.note, idx, timeout_ms=3000)
            print(json.dumps(res, indent=2))
        elif args.esp32_mothership:
            played = run_esp32_mothership(usb, args.esp32_tty, steps=args.steps, duration_ms=args.duration_ms)
            print(json.dumps({"esp32_tty": args.esp32_tty, "steps_played": len(played), "notes": played}, indent=2))
        elif args.interactive:
            run_interactive(usb, args.esp32_tty)
        else:
            res = decode_atrk(usb.read(0xc0, 0xb8, 0, 0, 64))
            print(json.dumps(res, indent=2))
    finally:
        usb.close()


if __name__ == "__main__":
    main()
