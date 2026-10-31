#!/usr/bin/env python3
"""Linux stdlib-only Board4 EP0 capture. Explicit USB node; private new output directory."""
import argparse
import ctypes
import errno
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import struct
import time
import zlib

DEVICE = bytes.fromhex("12011001ffa80440d118e04e440500000001")
CONFIG = bytes.fromhex("0902120001010080fa0904000000ffa80400")
MAX_BYTES = 1_048_576
CHUNK = 48


class ControlTransfer(ctypes.Structure):
    _fields_ = [("request_type", ctypes.c_uint8), ("request", ctypes.c_uint8),
                ("value", ctypes.c_uint16), ("index", ctypes.c_uint16),
                ("length", ctypes.c_uint16), ("timeout", ctypes.c_uint32),
                ("data", ctypes.c_void_p)]


class Usb:
    ALLOWED = ((0x80, 6), (0x80, 8), (0xc0, 0xb0), (0xc0, 0xb1))
    def __init__(self, path, seconds):
        if not re.fullmatch(r"/dev/bus/usb/[0-9]{3}/[0-9]{3}", path):
            raise ValueError("select an explicit /dev/bus/usb/BBB/DDD node from lsusb")
        self.fd = os.open(path, os.O_RDWR | os.O_CLOEXEC | os.O_NOFOLLOW)
        if not stat.S_ISCHR(os.fstat(self.fd).st_mode):
            os.close(self.fd)
            raise ValueError("USB node is not a character device")
        self.deadline = time.monotonic_ns() + seconds * 1_000_000_000
        self.libc = ctypes.CDLL(None, use_errno=True)
        self.libc.ioctl.argtypes = [ctypes.c_int, ctypes.c_ulong, ctypes.c_void_p]
        self.libc.ioctl.restype = ctypes.c_int
        # Linux asm-generic ioctl ABI (x86/ARM); size is native pointer dependent.
        self.command = (3 << 30) | (ctypes.sizeof(ControlTransfer) << 16) | (ord("U") << 8)

    def close(self):
        os.close(self.fd)

    def read(self, kind, request, value=0, index=0, length=64):
        if (kind, request) not in self.ALLOWED:
            raise ValueError("request outside read-only capture protocol")
        for attempt in range(3):
            remaining = (self.deadline - time.monotonic_ns()) // 1_000_000
            if remaining <= 0:
                raise TimeoutError("capture deadline exceeded")
            buf = ctypes.create_string_buffer(length)
            ctrl = ControlTransfer(kind, request, value, index, length,
                                   min(remaining, 1000), ctypes.addressof(buf))
            n = self.libc.ioctl(self.fd, self.command, ctypes.byref(ctrl))
            if n >= 0:
                return buf.raw[:n]
            error = ctypes.get_errno()
            if error not in (errno.ETIMEDOUT, errno.EINTR) or attempt == 2:
                raise OSError(error, os.strerror(error))
        raise AssertionError("unreachable")


def info(packet):
    if len(packet) != 64 or packet[:4] != b"A4IF":
        raise ValueError("invalid Board4 info header")
    version, size, crc, chunk = struct.unpack_from("<4I", packet, 4)
    if (version != 1 or not 40 <= size <= MAX_BYTES or chunk != CHUNK
            or packet[20:32] != b"board4-ep0\0\0" or any(packet[32:])):
        raise ValueError("unsupported Board4 version, size, identity or reserved bytes")
    return size, crc


def chunk(packet, offset, size):
    if len(packet) != 64 or packet[:4] != b"A4CH":
        raise ValueError("invalid chunk header/length")
    actual_offset, length, version, crc = struct.unpack_from("<IHHI", packet, 4)
    if actual_offset != offset or length != min(CHUNK, size - offset) or version != 1:
        raise ValueError("chunk offset/length/version mismatch")
    data = packet[16:16 + length]
    if zlib.crc32(data) != crc or any(packet[16 + length:]):
        raise ValueError("chunk CRC/padding mismatch")
    return data


def capture(usb, expected_device=DEVICE):
    # Same VID/PID is used by legacy images: exact descriptors AND protocol ID
    # are mandatory. No configuration changes, reset, OUT payload or reboot.
    if usb.read(0x80, 6, 0x0100, length=18) != expected_device:
        raise ValueError("not the exact Board4 device descriptor")
    if usb.read(0x80, 6, 0x0200, length=18) != CONFIG:
        raise ValueError("not the exact Board4 EP0 configuration")
    if usb.read(0x80, 8, length=1) != b"\x01":
        raise ValueError("device has not been configured by the host")
    first = usb.read(0xc0, 0xb0)
    size, crc = info(first)
    data = bytearray()
    for number, offset in enumerate(range(0, size, CHUNK)):
        packet = usb.read(0xc0, 0xb1, number & 0xffff, number >> 16)
        data.extend(chunk(packet, offset, size))
    if usb.read(0xc0, 0xb0) != first:
        raise ValueError("device info changed during capture")
    if len(data) != size or zlib.crc32(data) != crc:
        raise ValueError("complete FDT length/CRC mismatch")
    if data[:4] != b"\xd0\x0d\xfe\xed" or struct.unpack_from(">I", data, 4)[0] != size:
        raise ValueError("FDT header/size mismatch")
    return bytes(data), {"protocol": "board4-ep0-v1", "bytes": size,
                         "chunks": (size + CHUNK - 1) // CHUNK,
                         "crc32": f"{crc:08x}", "sha256": hashlib.sha256(data).hexdigest()}


def private_write(path, data):
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(fd, "wb") as stream:
        stream.write(data)
        stream.flush()
        os.fsync(stream.fileno())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--device", required=True, help="/dev/bus/usb/BBB/DDD from lsusb")
    parser.add_argument("--output", required=True, type=Path, help="new private directory")
    parser.add_argument("--deadline-seconds", type=int, default=300)
    args = parser.parse_args()
    if not 1 <= args.deadline_seconds <= 600:
        parser.error("deadline must be 1..600 integer seconds")
    if os.uname().machine not in ("x86_64", "aarch64", "armv7l", "i686"):
        parser.error("unsupported Linux ioctl architecture")
    args.output.mkdir(mode=0o700, exist_ok=False)
    usb = Usb(args.device, args.deadline_seconds)
    try:
        data, receipt = capture(usb)
    finally:
        usb.close()
    private_write(args.output / "board4-fdt.dtb", data)
    private_write(args.output / "capture.json", (json.dumps(receipt, indent=2) + "\n").encode())
    print(json.dumps(receipt, indent=2))
    print(f"Verified dump: {args.output / 'board4-fdt.dtb'} (keep private)")


if __name__ == "__main__":
    main()
