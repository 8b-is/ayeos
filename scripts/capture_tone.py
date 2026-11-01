#!/usr/bin/env python3
"""Owner-requested tone1 + alpha + pre-tone Board6 evidence. Linux stdlib only."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import struct
import zlib
import capture_alpha as alpha
import capture_board4 as b4

DEVICE = alpha.DEVICE[:12] + b"\x62" + alpha.DEVICE[13:]
SIZE = 6400
OFF = [(0x22c0, 0xc0, 0x40), (0x229a, 0xffff, 0), (0x2296, 0xffff, 0xcba0),
       (0x228a, 1, 0), (0x2288, 1, 0), (0x2416, 15, 0), (0x2408, 6, 0),
       (0x2422, 0x11, 0x10), (0x2420, 0x100, 0x100), (0x2488, 0xffff, 0),
       (0x2426, 1, 0), (0x2424, 0x1055, 0), (0x2240, 1, 1),
       (0x2292, 0xc7, 0xc6), (0x220c, 0x66, 0x66)]
OFF_MASK = (1 << (len(OFF) + 2)) - 1
ERRORS = ["none", "gate", "timer", "deadline", "fault", "busy", "timeout",
          "nack", "controller", "fifo", "readback", "restore", "capacity"]


class Usb(alpha.Usb):
    ALLOWED = alpha.Usb.ALLOWED + ((0xc0, 0xb6), (0xc0, 0xb7))


def decode(data):
    if len(data) != SIZE:
        raise ValueError("unexpected tone report size")
    w = struct.unpack_from("<16I", data)
    if (data[:4] != b"ATRP" or w[1:3] != (1, SIZE) or w[3] not in (1, 3)
            or w[4] not in range(1, 5) or w[5] >= len(ERRORS) or w[6] > 64
            or w[7] & ~OFF_MASK or w[8] > 1 or w[9] > 256 or w[10]
            or w[14:16] != (OFF_MASK, 250000)):
        raise ValueError("invalid tone header, status or journal capacity")
    if any(data[104:112]) or any(data[152:256]) or any(data[256+w[9]*24:]):
        raise ValueError("nonzero tone reserved/unused bytes")
    revision = data[64:104].decode("ascii")
    if not re.fullmatch("[0-9a-fA-F]{40}", revision):
        raise ValueError("invalid tone revision")
    hz, start, opened, closed, end = struct.unpack_from("<5Q", data, 112)
    events = []
    observed = {}
    prior_phase = 0
    for i in range(w[9]):
        tick, kind, phase, error, addr, mask, expected, actual = struct.unpack_from(
            "<QBBHHHII", data, 256+i*24)
        if (kind not in range(1, 6) or not prior_phase <= phase <= 4
                or error >= len(ERRORS) or (error not in (0, 10) and actual != 0xffffffff)):
            raise ValueError("invalid tone event")
        if kind <= 2 and addr & 1:
            raise ValueError("odd PMIC address")
        if kind in (3, 4) and (addr >> 8 not in (0x58, 0x59) or addr & 255 > 127):
            raise ValueError("unexpected amplifier address")
        if phase == 4 and kind in (1, 3):
            key = (kind, addr)
            if key in observed:
                raise ValueError("duplicate final Off observation")
            observed[key] = (error, actual)
        if kind in (2, 4) and not error and actual & mask != expected & mask:
            raise ValueError("successful update has mismatching readback")
        prior_phase = phase
        events.append(dict(tick=tick, kind=kind, phase=phase, error=ERRORS[error],
                           address=f"{addr:04x}", mask=mask, expected=expected,
                           observed=None if actual == 0xffffffff else actual))
    off = 0
    checks = [(3, 0x5802, 255, 0), (3, 0x5902, 255, 0)] + [
        (1, reg, mask, value) for reg, mask, value in OFF]
    for i, (kind, addr, mask, value) in enumerate(checks):
        error, actual = observed.get((kind, addr), (1, 0))
        if not error and actual & mask == value:
            off |= 1 << i
    if off != w[7]:
        raise ValueError("Off mask disagrees with final readback evidence")
    if w[4] == 1 and (w[5] or w[6] or off != OFF_MASK or not w[8]
                      or hz != 13000000 or not 0 < start <= opened < closed <= end
                      or closed-opened < 3250000 or opened-start >= 26000000
                      or any(e["error"] != "none" for e in events)):
        raise ValueError("inconsistent successful tone result")
    if w[4] == 2 and (not w[5] or off or opened or closed or any(e["kind"] in (2, 4) for e in events)):
        raise ValueError("preflight reject includes audio writes or Off claim")
    if w[4] == 3 and (off != OFF_MASK or not (w[5] or w[6])):
        raise ValueError("inconsistent aborted result")
    if w[4] == 4 and off == OFF_MASK:
        raise ValueError("Off-unverified result claims complete Off")
    return dict(protocol="audio-tone1-v1", revision=revision, dirty=bool(w[3] & 2),
                outcome=("unused", "complete", "preflight_rejected", "aborted", "off_unverified")[w[4]],
                error=ERRORS[w[5]], cleanup_errors=w[6], verified_off=off == OFF_MASK,
                off_mask=off, off_required=OFF_MASK, controllers_restored=bool(w[8]),
                board6_crc32=f"{w[11]:08x}", alpha_crc32=f"{w[12]:08x}", fdt_crc32=f"{w[13]:08x}",
                counter_hz=hz, start_tick=start, unmute_attempt_tick=opened,
                first_stop_attempts_end_tick=closed, end_tick=end, requested_tone_us=w[15],
                interval_ticks=closed-opened if opened and closed >= opened else None,
                acoustic_result="operator_observation_required", live_pcm=False, phoenix=False,
                events=events, bytes=SIZE, crc32=f"{zlib.crc32(data):08x}",
                sha256=hashlib.sha256(data).hexdigest())


def capture(usb, save):
    device = usb.read(0x80, 6, 0x0100, length=18)
    config = usb.read(0x80, 6, 0x0200, length=18)
    save("tone-device.bin", device)
    save("tone-config.bin", config)
    if device != DEVICE or config != b4.CONFIG or usb.read(0x80, 8, length=1) != b"\x01":
        raise ValueError("not the exact configured tone1 device")
    info = usb.read(0xc0, 0xb6)
    save("tone-info.bin", info)
    if (len(info) != 64 or info[:4] != b"ATIF" or struct.unpack_from("<2I", info, 4) != (1, SIZE)
            or struct.unpack_from("<I", info, 16)[0] != 48
            or info[20:32] != b"audio-tone1\0" or any(info[32:])):
        raise ValueError("invalid tone info")
    result = bytearray()
    for number in range((SIZE+47)//48):
        packet = usb.read(0xc0, 0xb7, number, 0, 64)
        save(f"tone-chunk-{number:03}.bin", packet)
        if len(packet) != 64 or packet[:4] != b"ATCH":
            raise ValueError("invalid tone chunk")
        result.extend(b4.chunk(b"A4CH" + packet[4:], number*48, SIZE))
    data = bytes(result)
    save("tone-report.bin", data)
    if usb.read(0xc0, 0xb6) != info or zlib.crc32(data) != struct.unpack_from("<I", info, 12)[0]:
        raise ValueError("changed tone snapshot or CRC error")
    report = decode(data)
    save("tone-report.json", (json.dumps(report, indent=2)+"\n").encode())
    _, diagnostic, _, board = alpha.capture(usb, save, expected_device=DEVICE)
    for name, obj in (("alpha-report.json", diagnostic), ("board6-report.json", board)):
        save(name, (json.dumps(obj, indent=2)+"\n").encode())
    if (report["revision"] != diagnostic["revision"] or report["dirty"] != diagnostic["dirty"]
            or report["alpha_crc32"] != diagnostic["crc32"]
            or report["board6_crc32"] != board["crc32"] or report["fdt_crc32"] != board["fdt_crc32"]):
        raise ValueError("tone/alpha/Board6 evidence mismatch")
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--device", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--include-fdt", action="store_true")
    parser.add_argument("--deadline-seconds", type=int, default=300)
    args = parser.parse_args()
    if not 1 <= args.deadline_seconds <= 600:
        parser.error("deadline must be 1..600 integer seconds")
    if os.uname().machine not in ("x86_64", "aarch64", "armv7l", "i686"):
        parser.error("unsupported Linux ioctl architecture")
    args.output.mkdir(mode=0o700, exist_ok=False)
    def save(name, data):
        b4.private_write(args.output / name, data)
    usb = None
    try:
        usb = Usb(args.device, args.deadline_seconds)
        report = capture(usb, save)
        if args.include_fdt:
            data, info = b4.capture(usb, expected_device=DEVICE)
            save("board6-fdt.dtb", data)
            save("capture.json", (json.dumps(info, indent=2)+"\n").encode())
            if info["crc32"] != report["fdt_crc32"]:
                raise ValueError("tone/FDT mismatch")
    except Exception as error:
        save("capture-error.txt", (str(error)+"\n").encode())
        raise
    finally:
        if usb is not None:
            usb.close()
        rows = [f"{hashlib.sha256(p.read_bytes()).hexdigest()}  {p.name}\n"
                for p in sorted(args.output.iterdir()) if p.is_file()]
        save("SHA256SUMS", "".join(rows).encode())
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
