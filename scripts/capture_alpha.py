#!/usr/bin/env python3
"""Owner-requested alpha1 snapshot + Board6 capture; Linux stdlib only."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import struct
import zlib
import capture_board4 as b4
import capture_board5 as b5
import capture_board6 as b6

DEVICE = b4.DEVICE[:12] + b"\x61" + b4.DEVICE[13:]
SIZE = 1024
REFERENCE_HASH = 0x800d3c2f169c4722
STAGES = {1: "entry", 2: "framebuffer", 3: "crc_selftest", 4: "marine_fixture",
          5: "exception_selftest", 6: "board6_probe", 7: "snapshot_frozen"}


class Usb(b5.Usb):
    ALLOWED = b5.Usb.ALLOWED + ((0xc0, 0xb4), (0xc0, 0xb5))


def decode(data):
    if len(data) != SIZE:
        raise ValueError("unexpected alpha snapshot size")
    w = struct.unpack_from("<11I", data)
    if (data[:4] != b"ADRP" or w[1:3] != (1, SIZE) or w[3] not in (1, 3)
            or w[4] != 3 or w[5] & w[6] or w[5] | w[6] != 7 or not 0 <= w[7] <= 16):
        raise ValueError("unsupported alpha header, tests or capabilities")
    for start, end in ((44, 64), (104, 112), (120, 128), (208, 256), (768, SIZE)):
        if any(data[start:end]):
            raise ValueError("nonzero alpha reserved bytes")
    revision = data[64:104].decode("ascii")
    if not re.fullmatch("[0-9a-fA-F]{40}", revision):
        raise ValueError("invalid build revision")
    count = struct.unpack_from("<Q", data, 112)[0]
    vector, level, esr, pc, far, spsr, sp, action = struct.unpack_from("<8Q", data, 128)
    if action not in (0, 1, 2):
        raise ValueError("fatal data cannot belong to a pre-USB frozen snapshot")
    if action == 0:
        if count or any(data[128:192]):
            raise ValueError("absent fault has observations")
    elif (vector not in (0, 4) or level not in (1, 2) or not esr & (1 << 25)
          or pc & 3 or sp & 15 or (action == 1 and (count or esr >> 26 != 0x3c or esr & 0xffff != 0xa8))
          or (action == 2 and (not count or esr >> 26 != 0x25))):
        raise ValueError("inconsistent recovered exception")
    if w[5] & 4 and action == 0:
        raise ValueError("exception self-test passed without a record")
    marine_hash, samples = struct.unpack_from("<2Q", data, 192)
    if samples > 1024 or (w[5] & 2 and (samples != 1024 or marine_hash != REFERENCE_HASH)):
        raise ValueError("inconsistent Marine fixture result")
    dropped = struct.unpack_from("<Q", data, 32)[0]
    n = w[7]
    if (dropped and n != 16) or dropped + n >= 1 << 64:
        raise ValueError("invalid journal bounds")
    events = []
    for i in range(n):
        seq, stage, result, detail = struct.unpack_from("<4Q", data, 256 + i*32)
        if seq != dropped + i + 1 or stage not in STAGES:
            raise ValueError("invalid journal sequence/stage")
        events.append(dict(sequence=seq, stage=STAGES[stage], result=result, detail=detail))
    if any(data[256 + n*32:768]):
        raise ValueError("nonzero unused journal entries")
    tests = {name: "pass" if w[5] & mask else "fail"
             for name, mask in (("crc32", 1), ("marine_fixture", 2), ("exception", 4))}
    return dict(protocol="audio-alpha1-v1", revision=revision, dirty=bool(w[3] & 2),
                lifetime="frozen_pre_USB_single_boot_RAM", capabilities=dict(
                    diagnostics=True, marine_fixture=True, live_pcm=False, phoenix=False),
                tests=tests, board6_crc32=f"{w[10]:08x}", unexpected_faults=count,
                fault=dict(action=("absent", "expected_breakpoint", "guarded_data_abort")[action],
                           vector=vector, level=level, esr=f"{esr:016x}", pc=f"{pc:016x}",
                           far=f"{far:016x}", far_valid=action == 2 and not esr & (1 << 10),
                           spsr=f"{spsr:016x}", sp=f"{sp:016x}"),
                marine=dict(input="synthetic_fixture", samples=samples, fnv64=f"{marine_hash:016x}"),
                events=events, dropped_events=dropped, bytes=SIZE,
                crc32=f"{zlib.crc32(data):08x}", sha256=hashlib.sha256(data).hexdigest())


def capture(usb, save=None, expected_device=DEVICE):
    if (usb.read(0x80, 6, 0x0100, length=18) != expected_device or
            usb.read(0x80, 6, 0x0200, length=18) != b4.CONFIG or
            usb.read(0x80, 8, length=1) != b"\x01"):
        raise ValueError("not the exact configured alpha1 device")
    info = usb.read(0xc0, 0xb4)
    if save:
        save("alpha-info.bin", info)
    if (len(info) != 64 or info[:4] != b"ADIF" or
            struct.unpack_from("<2I", info, 4) != (1, SIZE) or
            struct.unpack_from("<I", info, 16)[0] != 48 or
            info[20:32] != b"audio-alpha1" or any(info[32:])):
        raise ValueError("invalid alpha info")
    result = bytearray()
    for number in range((SIZE+47)//48):
        packet = usb.read(0xc0, 0xb5, number, 0, 64)
        if save:
            save(f"alpha-chunk-{number:02}.bin", packet)
        if len(packet) != 64 or packet[:4] != b"ADCH":
            raise ValueError("invalid alpha chunk")
        result.extend(b4.chunk(b"A4CH" + packet[4:], number*48, SIZE))
    data = bytes(result)
    if save:
        save("alpha-report.bin", data)
    if usb.read(0xc0, 0xb4) != info or zlib.crc32(data) != struct.unpack_from("<I", info, 12)[0]:
        raise ValueError("alpha report changed or whole-file CRC failed")
    report = decode(data)
    board_data, board = b6.capture(usb, expected_device=expected_device)
    if save:
        save("board6-report.bin", board_data)
    if board["crc32"] != report["board6_crc32"]:
        raise ValueError("alpha/Board6 snapshot mismatch")
    return data, report, board_data, board


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--device", required=True)
    parser.add_argument("--output", type=Path, required=True, help="new private directory")
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
    usb = Usb(args.device, args.deadline_seconds)
    try:
        _, report, _, board = capture(usb, save)
        for name, obj in (("alpha-report.json", report), ("board6-report.json", board)):
            save(name, (json.dumps(obj, indent=2) + "\n").encode())
        if args.include_fdt:
            data, info = b4.capture(usb, expected_device=DEVICE)
            save("board6-fdt.dtb", data)
            save("capture.json", (json.dumps(info, indent=2) + "\n").encode())
            if info["crc32"] != board["fdt_crc32"]:
                raise ValueError("report/FDT mismatch")
    except Exception as error:
        save("capture-error.txt", (str(error) + "\n").encode())
        raise
    finally:
        usb.close()
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
