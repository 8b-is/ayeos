#!/usr/bin/env python3
"""Capture Board6's frozen inherited PA/PMIC state (requires adjacent Board4/5 helpers)."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import struct
import zlib
import capture_board4 as b4
import capture_board5 as b5

DEVICE = b4.DEVICE[:12] + b"\x46" + b4.DEVICE[13:]
SIZE = 6400
PMIC_STATUS = ("not_run", "complete", "fdt_mismatch", "fault", "clock_off",
               "inherited_config_unsupported", "bus_busy", "timeout",
               "read_test_mismatch", "swcid_mismatch", "ack_idle_timeout")
REGISTERS = [("DEW_READ_TEST", 0x040e), ("HWCID", 8), ("SWCID", 10),
             ("AFE_UL_DL_CON0", 0x2288)] + [
    (f"AUDENC_ANA_CON{i}", 0x2388 + 2*i) for i in range(13)] + [
    (f"AUDDEC_ANA_CON{i}", 0x2408 + 2*i) for i in range(16)]
SNAPSHOTS = ("infra_cg0", "mux_sel", "wrap_en", "dio_en", "hprio_arb_en",
             "man_en", "wacs2_en", "init_done2", "wacs2_rdata_before",
             "int0_flg_raw", "int1_flg_raw", "wacs2_rdata_after")


def observed(v):
    return None if v == 0xffffffff else v


def decode(data):
    if len(data) != SIZE:
        raise ValueError("unexpected Board6 report size")
    w = struct.unpack("<1600I", data)
    if (data[:4] != b"A6RP" or w[1:3] != (1, SIZE) or
            w[4:8] != (256, 33, 32768, 1) or any(w[8:16]) or any(w[1541:])):
        raise ValueError("unsupported Board6 header/reserved fields")
    summary = b5.decode(data[64:256])
    if summary["fdt_crc32"] != f"{w[3]:08x}":
        raise ValueError("inconsistent FDT CRC")
    amps = []
    for chip, address in enumerate((0x58, 0x59)):
        registers = []
        for reg in range(128):
            at = 64 + (chip*128 + reg)*5
            status, value, irq, count, polls = w[at:at+5]
            if status >= len(b5.STATUSES):
                raise ValueError("invalid amplifier status")
            if status == 1 and (value > 255 or count != 1 or not 1 <= polls <= 32768):
                raise ValueError("invalid completed amplifier read")
            if status == 0 and any(x != 0xffffffff for x in w[at+1:at+5]):
                raise ValueError("unattempted amplifier read has observations")
            registers.append(dict(register=f"0x{reg:02x}", status=b5.STATUSES[status],
                                  value=observed(value), irq=observed(irq),
                                  fifo_count=observed(count), polls=observed(polls)))
        amps.append(dict(address=f"0x{address:02x}", registers=registers))
    h = w[1344:1376]
    if (h[0] >= len(PMIC_STATUS) or h[1] not in (0, 1) or h[2] != 32768 or
            h[3] not in (0, 1) or any(h[16:])):
        raise ValueError("invalid PWRAP header")
    pmic = []
    for i, (name, addr) in enumerate(REGISTERS):
        at = 1376 + 5*i
        status, value, rdata, polls, ack = w[at:at+5]
        if status >= len(PMIC_STATUS):
            raise ValueError("invalid PMIC status")
        if status == 1 and (value > 65535 or not 1 <= polls <= 32768 or
                            not 1 <= ack <= 32768 or (rdata >> 16) & 7 != 6 or
                            not rdata & (1 << 22) or value != rdata & 0xffff):
            raise ValueError("invalid completed PMIC read")
        if status == 0 and any(x != 0xffffffff for x in w[at+1:at+5]):
            raise ValueError("unattempted PMIC read has observations")
        pmic.append(dict(name=name, register=f"0x{addr:04x}", status=PMIC_STATUS[status],
                         value=observed(value), rdata=observed(rdata),
                         polls=observed(polls), ack_idle_polls=observed(ack)))
    if h[0] == 1 and (any(r["status"] != "complete" for r in pmic) or
                      pmic[0]["value"] != 0x5aa5 or pmic[2]["value"] >> 8 != 0x58 or h[3] != 1):
        raise ValueError("inconsistent completed PMIC probe")
    if summary["status"] == "complete" and any(r["status"] != "complete" for a in amps for r in a["registers"]):
        raise ValueError("inconsistent completed amplifier window")
    return dict(protocol="board6-inherited-v1", state="inherited_after_LK_not_POR",
                fdt_crc32=f"{w[3]:08x}", i2c_summary=summary, amplifiers=amps,
                pwrap=dict(status=PMIC_STATUS[h[0]], fdt_profile_matched=bool(h[1]),
                           poll_budget=h[2], last_observed_idle=bool(h[3]),
                           snapshots={n: observed(h[4+i]) for i, n in enumerate(SNAPSHOTS)},
                           registers=pmic), tone_enabled=False, profile_loaded=False,
                bytes=SIZE, crc32=f"{zlib.crc32(data):08x}", sha256=hashlib.sha256(data).hexdigest())


def capture(usb):
    if usb.read(0x80, 6, 0x0100, length=18) != DEVICE:
        raise ValueError("not the exact Board6 device descriptor")
    if usb.read(0x80, 6, 0x0200, length=18) != b4.CONFIG or usb.read(0x80, 8, length=1) != b"\x01":
        raise ValueError("Board6 configuration mismatch")
    info = usb.read(0xc0, 0xb2)
    if len(info) != 64 or info[:4] != b"A6IF":
        raise ValueError("invalid Board6 info")
    version, size, crc, chunk = struct.unpack_from("<4I", info, 4)
    if (version, size, chunk) != (1, SIZE, 48) or info[20:32] != b"board6-aud\0\0" or any(info[32:]):
        raise ValueError("unsupported Board6 info")
    result = bytearray()
    for number in range((SIZE + 47)//48):
        packet = usb.read(0xc0, 0xb3, number, 0, 64)
        if len(packet) != 64 or packet[:4] != b"A6CH":
            raise ValueError("invalid Board6 chunk")
        result.extend(b4.chunk(b"A4CH" + packet[4:], number*48, SIZE))
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
    usb = b5.Usb(args.device, args.deadline_seconds)
    try:
        data, report = capture(usb)
        fdt = b4.capture(usb, expected_device=DEVICE) if args.include_fdt else None
        if fdt is not None and fdt[1]["crc32"] != report["fdt_crc32"]:
            raise ValueError("report/FDT mismatch")
    finally:
        usb.close()
    b4.private_write(args.output / "board6-report.bin", data)
    b4.private_write(args.output / "board6-report.json", (json.dumps(report, indent=2) + "\n").encode())
    if fdt is not None:
        b4.private_write(args.output / "board6-fdt.dtb", fdt[0])
        b4.private_write(args.output / "capture.json", (json.dumps(fdt[1], indent=2) + "\n").encode())
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
