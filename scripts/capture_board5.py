#!/usr/bin/env python3
"""Read Board5's frozen SmartPA diagnostic; optional private FDT, no device actions."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import struct
import zlib
import capture_board4 as b4

DEVICE = b4.DEVICE[:12] + b"\x45" + b4.DEVICE[13:]
STATUSES = ("not_run", "complete", "fdt_mismatch", "fault", "clock_off",
            "pinmux_mismatch", "bus_busy", "inherited_config_unsupported",
            "timeout", "nack", "arbitration_lost", "controller_error",
            "fifo_length_error", "restore_failed")
OFFSETS = (0x04, 0x08, 0x10, 0x14, 0x18, 0x1c, 0x20, 0x28, 0x2c,
           0x30, 0x34, 0x3c, 0x44, 0x48, 0x90, 0xe8, 0xf8)


class Usb(b4.Usb):
    ALLOWED = b4.Usb.ALLOWED + ((0xc0, 0xb2), (0xc0, 0xb3))


def decode(data):
    if len(data) != 192:
        raise ValueError("unexpected report size")
    w = struct.unpack("<48I", data)
    if data[:4] != b"A5RP" or w[1:3] != (1, 192) or any(w[43:]) or w[7] != 0:
        raise ValueError("unsupported report header/reserved fields")
    if any(w[i] >= len(STATUSES) for i in (4, 32, 37)) or w[5] not in (0, 1) or w[42] not in (0, 1):
        raise ValueError("invalid report status")

    def observed(value):
        return None if value == 0xffffffff else value

    def probe(at, address):
        if w[at] == 1 and (w[at+1] > 255 or w[at+3] != 1):
            raise ValueError("invalid completed chip read")
        return dict(address=f"0x{address:02x}", status=STATUSES[w[at]],
                    raw_chip_id=observed(w[at+1]), irq=observed(w[at+2]),
                    fifo_count=observed(w[at+3]), polls=observed(w[at+4]))

    return dict(protocol="board5-audio-v1", fdt_crc32=f"{w[3]:08x}",
                status=STATUSES[w[4]], fdt_profile_matched=bool(w[5]),
                poll_budget=w[6], configuration_restored=bool(w[42]),
                snapshots={name: observed(w[8+i]) for i, name in enumerate((
                    "top_clk_cfg6", "infra_cg4", "gpio_mode13", "start", "irq", "fifo", "debug"))},
                i2c4_config_before={f"0x{off:02x}": observed(w[15+i]) for i, off in enumerate(OFFSETS)},
                amplifiers=[probe(32, 0x58), probe(37, 0x59)],
                tone_enabled=False, profile_loaded=False,
                bytes=len(data), crc32=f"{zlib.crc32(data):08x}",
                sha256=hashlib.sha256(data).hexdigest())


def capture(usb):
    if usb.read(0x80, 6, 0x0100, length=18) != DEVICE:
        raise ValueError("not the exact Board5 device descriptor")
    if usb.read(0x80, 6, 0x0200, length=18) != b4.CONFIG or usb.read(0x80, 8, length=1) != b"\x01":
        raise ValueError("Board5 configuration mismatch")
    info = usb.read(0xc0, 0xb2)
    if len(info) != 64 or info[:4] != b"A5IF":
        raise ValueError("invalid report info")
    version, size, crc, chunk = struct.unpack_from("<4I", info, 4)
    if (version, size, chunk) != (1, 192, 48) or info[20:32] != b"board5-aud\0\0" or any(info[32:]):
        raise ValueError("unsupported report info")
    result = bytearray()
    for number in range(4):
        packet = usb.read(0xc0, 0xb3, number, 0, 64)
        if len(packet) != 64 or packet[:4] != b"A5CH":
            raise ValueError("invalid report chunk")
        # Same numbered/chunk-CRC envelope as Board4; magic checked above.
        result.extend(b4.chunk(b"A4CH" + packet[4:], number * 48, size))
    if usb.read(0xc0, 0xb2) != info or zlib.crc32(result) != crc:
        raise ValueError("report changed or whole-file CRC failed")
    data = bytes(result)
    return data, decode(data)


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
    usb = Usb(args.device, args.deadline_seconds)
    try:
        data, report = capture(usb)
        fdt = b4.capture(usb, expected_device=DEVICE) if args.include_fdt else None
        if fdt is not None and fdt[1]["crc32"] != report["fdt_crc32"]:
            raise ValueError("report/FDT mismatch")
    finally:
        usb.close()
    b4.private_write(args.output / "board5-report.bin", data)
    b4.private_write(args.output / "board5-report.json", (json.dumps(report, indent=2) + "\n").encode())
    if fdt is not None:
        b4.private_write(args.output / "board5-fdt.dtb", fdt[0])
        b4.private_write(args.output / "capture.json", (json.dumps(fdt[1], indent=2) + "\n").encode())
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
