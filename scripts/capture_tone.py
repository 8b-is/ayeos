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


def complete_sequence():
    """Successful tone1-v1 journal, from baremetal/src/tone.rs (not acoustic proof).

    Failure journals may be prefixes with independent cleanup; only complete
    results must contain this entire sequence. Values are register encodings.
    """
    rows = []

    def add(kind, phase, address=0, mask=0, expected=0):
        rows.append((kind, phase, address, mask, expected))

    add(5, 0, expected=10)
    for reg in (0x040e, 0x000a, 0x1ac4, 0x1ad2):
        add(1, 0, reg)
    for addr in (0x5800, 0x5900):
        add(3, 0, addr, 255, 0x23)
    for reg in (0x2288, 0x228a, 0x2408, 0x2414, 0x2416,
                0x2388, 0x238a, 0x238c, 0x238e):
        add(1, 0, reg)
    add(2, 1, 0x22c0, 0xc0, 0x40)
    add(2, 1, 0x2416, 0xc, 0)
    for addr in (0x5800, 0x5900):
        for reg, expected in ((2, 0), (0, 0x23), (2, 0)):
            add(4, 1, addr | reg, 255, expected)
    for reg, mask, value in ((0x07ac, 0x2000, 0x2000), (0x2422, 0x10, 0),
                             (0x2394, 2, 0), (0x2394, 1, 1),
                             (0x220c, 0x64, 0), (0x220c, 2, 0)):
        add(2, 1, reg, mask, value)
    add(5, 1, expected=250)
    add(2, 1, 0x2292, 0x46, 0)
    add(2, 1, 0x2292, 0x80, 0)
    add(5, 1, expected=250)
    add(2, 1, 0x2288, 1, 1)
    for reg, value in ((0x229a, 6), (0x2296, 0xcba1), (0x229a, 3), (0x229a, 0xb)):
        add(2, 1, reg, 0xffff, value)
    for reg, mask, value in ((0x22c0, 0xff3f, 0), (0x22c2, 0xffff, 1),
                             (0x2290, 1, 1), (0x22c0, 0x80, 0x80), (0x228a, 1, 1)):
        add(2, 1, reg, mask, value)
    for reg, value in ((0x240c, 0x4000), (0x223c, 1), (0x223e, 0x2c),
                       (0x223a, 1), (0x2242, 3), (0x2240, 0)):
        add(2, 1, reg, 0xffff, value)
    add(5, 1, expected=250)
    add(2, 1, 0x2424, 0x1055, 0x1055)
    add(2, 1, 0x2426, 0xffff, 1)
    add(5, 1, expected=100)
    for reg, mask, value in (
            (0x241e, 7, 0), (0x2488, 0x7ff, 0x10b), (0x2416, 0x10, 0x10),
            (0x248a, 0xffff, 0x0912), (0x2420, 0xffff, 0x55),
            (0x241e, 0xff80, 0x4900), (0x2420, 0xffff, 0x55),
            (0x2416, 0x100, 0x100), (0x241a, 0xffff, 0), (0x241c, 0xffff, 0),
            (0x2416, 2, 2), (0x2416, 1, 1), (0x248a, 0xf9f, 0x0408),
            (0x2422, 1, 1), (0x2408, 6, 6), (0x241a, 0xffff, 1), (0x2416, 0xc, 4)):
        add(2, 1, reg, mask, value)
    music = ((1, 2), (2, 0x3f), (3, 0x36), (4, 0x3b), (5, 0x5a),
             (0x60, 0xf0), (0x61, 0x14), (0x62, 3), (0x63, 1), (0x64, 0x59),
             (0x65, 0x7f), (0x66, 0x50), (0x67, 0x19),
             (0x79, 0), (0x7a, 0), (0x7b, 0), (0x7c, 0), (0x7d, 0))
    for addr in (0x5800, 0x5900):
        for reg, value in music:
            add(4, 1, addr | reg, 255, 0x2b if addr == 0x5900 and reg == 4 else value)
    add(5, 1, expected=30000)
    add(2, 2, 0x22c0, 0x40, 0)
    for div in (12, 8, 6, 4):
        add(2, 2, 0x22c2, 0xffff, div)
    add(2, 3, 0x22c0, 0xc0, 0x40)
    add(2, 3, 0x2416, 0xc, 0)
    for addr in (0x5802, 0x5902):
        add(4, 3, addr, 255, 0)
    for reg, mask, value in (
            (0x229a, 0xffff, 0), (0x2296, 0xffff, 0xcba0), (0x228a, 1, 0),
            (0x2408, 6, 0), (0x2422, 1, 0), (0x248a, 0xffff, 0xf9f),
            (0x2416, 1, 0), (0x2416, 2, 0), (0x241a, 0xff00, 0),
            (0x241a, 0xff00, 0x200), (0x2420, 0x100, 0x100), (0x241e, 7, 4),
            (0x2488, 0xffff, 0), (0x2426, 1, 0), (0x2424, 0x1055, 0),
            (0x2240, 1, 1), (0x2288, 1, 0), (0x2290, 1, 0), (0x2292, 0xc7, 0xc6),
            (0x220c, 0x66, 0x66), (0x2394, 1, 0), (0x2422, 0x10, 0x10),
            (0x07ac, 0x2000, 0)):
        if reg in (0x2292, 0x220c):
            add(5, 3, expected=250)
        add(2, 3, reg, mask, value)
    for addr in (0x5802, 0x5902):
        add(3, 4, addr, 255, 0)
    for reg, _, _ in OFF:
        add(1, 4, reg)
    return tuple(rows)


COMPLETE_SEQUENCE = complete_sequence()


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
    signatures = []
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
        if kind <= 4:
            width = 0xffff if kind <= 2 else 0xff
            if expected > width or (error in (0, 10) and actual > width):
                raise ValueError("out-of-range register value")
            if kind == 1 and (mask or expected):
                raise ValueError("PMIC read has update fields")
            if kind == 2 and not mask:
                raise ValueError("PMIC update has empty mask")
            if kind in (3, 4) and mask != 255:
                raise ValueError("amplifier event must use full-byte mask")
            if kind == 3 and expected != (0x23 if addr & 255 == 0 else 0):
                raise ValueError("invalid amplifier read expectation")
            if kind in (1, 3) and (phase not in (0, 4) or error == 10):
                raise ValueError("invalid read phase or readback error")
            if kind in (2, 4) and phase not in (1, 2, 3):
                raise ValueError("invalid write phase")
            pa_off = kind in (3, 4) and addr in (0x5802, 0x5902) and expected == 0
            zcd_con0 = kind == 2 and addr == 0x2488 and mask == 0x7ff and expected == 0x10b
            matched = (actual in (0, 0x3e) if pa_off else
                       actual & 0x6ff == 0x00b if zcd_con0 else
                       actual & mask == expected & mask)
            if error == 10 and matched:
                raise ValueError("readback error has matching observation")
        elif (addr or mask or phase not in (0, 1, 3) or error == 10
              or expected not in (10, 100, 250, 30000)
              or (not error and actual < expected * 13)):
            raise ValueError("invalid delay evidence")
        signatures.append((kind, phase, addr, mask, expected))
        if phase == 4 and kind in (1, 3):
            key = (kind, addr)
            if key in observed:
                raise ValueError("duplicate final Off observation")
            observed[key] = (error, actual)
        if kind in (2, 4) and not error and not matched:
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
        if not error and (actual in (0, 0x3e) if kind == 3 else actual & mask == value):
            off |= 1 << i
    if off != w[7]:
        raise ValueError("Off mask disagrees with final readback evidence")
    if w[4] == 1 and (w[5] or w[6] or off != OFF_MASK or not w[8]
                      or hz != 13000000 or not 0 < start <= opened < closed <= end
                      or closed-opened < 3250000 or opened-start >= 26000000
                      or any(e["error"] != "none" for e in events)):
        raise ValueError("inconsistent successful tone result")
    if w[4] == 1:
        if tuple(signatures) != COMPLETE_SEQUENCE:
            raise ValueError("complete result lacks the successful tone1 operation sequence")
        prior_tick = start
        cleanup = [e for e in events if e["phase"] == 3]
        for e in events:
            tick = e["tick"]
            if not prior_tick <= tick <= end:
                raise ValueError("complete event timestamps out of order or bounds")
            if ((e["phase"] < 2 and tick > opened)
                    or (e["phase"] >= 2 and tick < opened)
                    or (e["phase"] == 4 and tick < closed)
                    or (e["kind"] == 5 and tick + e["observed"] > end)):
                raise ValueError("complete event timestamp contradicts phase or delay")
            prior_tick = tick
        if (cleanup[0]["tick"] - opened < 3250000
                or any(e["tick"] > closed for e in cleanup[:4])
                or any(e["tick"] < closed for e in cleanup[4:])):
            raise ValueError("complete stop timestamps contradict the tone window")
        preflight = {int(e["address"], 16): e["observed"]
                     for e in events if e["phase"] == 0 and e["kind"] != 5}
        predicates = [(0x040e, 0xffff, 0x5aa5), (0x000a, 0xff00, 0x5800),
                      (0x1ac4, 1, 1), (0x1ad2, 0x8000, 0x8000),
                      (0x5800, 255, 0x23), (0x5900, 255, 0x23)] + [
            (reg, mask, 0) for reg, mask in ((0x2288, 1), (0x228a, 1),
            (0x2408, 0xffff), (0x2414, 0xffff), (0x2416, 15),
            (0x2388, 0xffff), (0x238a, 0xffff), (0x238c, 0xffff), (0x238e, 0xffff))]
        if any(preflight[reg] & mask != value for reg, mask, value in predicates):
            raise ValueError("complete result contradicts preflight gates")
    if w[4] == 2 and (not w[5] or w[6] or off or opened or closed or end
                      or any(e["phase"] != 0 for e in events)):
        raise ValueError("preflight reject includes cleanup, audio writes or Off claim")
    if w[4] != 2:
        if not w[8] and not w[6]:
            raise ValueError("failed post-arm restoration lacks cleanup error")
        if tuple(s for s in signatures if s[1] == 4) != COMPLETE_SEQUENCE[-17:]:
            raise ValueError("post-arm result lacks final independent Off observations")
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
