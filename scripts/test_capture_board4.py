import ctypes
import gzip
import os
from pathlib import Path
import struct
import tempfile
import unittest
from unittest.mock import patch
import zlib

import capture_board4 as capture
import package_bringup


class SimulatedUsb:
    def __init__(self, size=207201):
        self.blob = bytearray((n % 251 for n in range(size)))
        self.blob[:8] = struct.pack(">II", 0xd00dfeed, size)
        self.calls = []
        self.mutate = lambda req, value, packet: packet

    def read(self, kind, req, value=0, index=0, length=64):
        self.calls.append((kind, req, value, index, length))
        if (kind, req) == (0x80, 6):
            packet = capture.DEVICE if value == 0x100 else capture.CONFIG
        elif (kind, req) == (0x80, 8):
            packet = b"\1"
        elif req == 0xb0:
            packet = struct.pack("<4s4I12s32x", b"A4IF", 1, len(self.blob),
                                 zlib.crc32(self.blob), 48, b"board4-ep0\0\0")
        elif req == 0xb1:
            offset = (value | (index << 16)) * 48
            data = self.blob[offset:offset + 48]
            packet = struct.pack("<4sIHHI48s", b"A4CH", offset, len(data), 1, zlib.crc32(data), data)
        else:
            raise AssertionError("unexpected command")
        return self.mutate(req, value, packet)


class CaptureTests(unittest.TestCase):
    def test_complete_capture_and_read_only_commands(self):
        usb = SimulatedUsb()
        data, receipt = capture.capture(usb)
        self.assertEqual(data, usb.blob)
        self.assertEqual(receipt["chunks"], 4317)
        self.assertEqual(receipt["crc32"], f"{zlib.crc32(data):08x}")
        self.assertTrue(all(k & 0x80 and r in (6, 8, 0xb0, 0xb1) for k, r, *_ in usb.calls))

    def test_descriptor_mismatch_stops_before_vendor_requests(self):
        usb = SimulatedUsb()
        usb.mutate = lambda r, v, p: bytes(len(p))
        with self.assertRaises(ValueError):
            capture.capture(usb)
        self.assertEqual(len(usb.calls), 1)

    def test_corrupt_offset_length_version_crc_padding_and_short_chunk(self):
        for where in (0, 4, 8, 10, 12, 16, 63, None):
            usb = SimulatedUsb(49)

            def corrupt(req, value, packet):
                if req == 0xb1 and value == 1:
                    if where is None:
                        return packet[:-1]
                    p = bytearray(packet)
                    p[where] ^= 1
                    return bytes(p)
                return packet

            usb.mutate = corrupt
            with self.subTest(where=where), self.assertRaises(ValueError):
                capture.capture(usb)

    def test_info_bounds_reserved_bytes_and_whole_crc(self):
        for where in (0, 4, 8, 12, 16, 20, 40):
            usb = SimulatedUsb(49)

            def corrupt(req, value, packet):
                if req == 0xb0:
                    p = bytearray(packet)
                    p[where] ^= 1
                    return bytes(p)
                return packet

            usb.mutate = corrupt
            with self.subTest(where=where), self.assertRaises(ValueError):
                capture.capture(usb)

    def test_info_change_after_capture_fails(self):
        usb = SimulatedUsb(49)
        count = 0

        def change(req, value, packet):
            nonlocal count
            if req == 0xb0:
                count += 1
                if count == 2:
                    return packet[:-1] + b"\1"
            return packet

        usb.mutate = change
        with self.assertRaisesRegex(ValueError, "changed"):
            capture.capture(usb)

    def test_private_exclusive_file(self):
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "dump"
            capture.private_write(p, b"verified")
            self.assertEqual(p.stat().st_mode & 0o777, 0o600)
            with self.assertRaises(FileExistsError):
                capture.private_write(p, b"overwrite")
            self.assertEqual(p.read_bytes(), b"verified")
            link = Path(d) / "link"
            link.symlink_to(p)
            with self.assertRaises(FileExistsError):
                capture.private_write(link, b"overwrite")

    def test_ioctl_timeout_budget_and_command_allowlist(self):
        usb = capture.Usb.__new__(capture.Usb)
        usb.fd = 99
        usb.command = 0
        usb.deadline = 5_000_000_000

        class Lib:
            calls = 0

            def ioctl(self, fd, command, arg):
                self.calls += 1
                transfer = ctypes.cast(arg, ctypes.POINTER(capture.ControlTransfer)).contents
                self.assert_timeout = transfer.timeout
                ctypes.set_errno(110)  # ETIMEDOUT
                return -1

        usb.libc = Lib()
        with patch.object(capture.time, "monotonic_ns", return_value=0):
            with self.assertRaises(OSError):
                usb.read(0xc0, 0xb0)
        self.assertEqual(usb.libc.calls, 3)
        self.assertEqual(usb.libc.assert_timeout, 1000)
        with self.assertRaises(ValueError):
            usb.read(0x40, 0xa8)
        with patch.object(capture.time, "monotonic_ns", return_value=usb.deadline):
            with self.assertRaises(TimeoutError):
                usb.read(0xc0, 0xb0)
        self.assertEqual(usb.libc.calls, 3)

    def test_packaging_identity_roundtrip_padding_and_rejection(self):
        raw = bytearray(5000)
        raw[56:60] = b"ARM\x64"
        for board in ("board3", "board4", "board5", "board6"):
            image = package_bringup.package(raw, board)
            self.assertEqual(image, package_bringup.package(raw, board))
            self.assertEqual(image[:8], b"ANDROID!")
            self.assertEqual(len(image) % 4096, 0)
            self.assertEqual(struct.unpack_from("<I", image, 20)[0], 1580)
            self.assertEqual(struct.unpack_from("<I", image, 40)[0], 4)
            self.assertIn(f"studio-{board}".encode(), image[:4096])
            size = struct.unpack_from("<I", image, 8)[0]
            self.assertEqual(gzip.decompress(image[4096:4096 + size]), raw)
            self.assertFalse(any(image[4096 + size:]))
        for bad in (b"", b"\0" * 64, b"\0" * (4 * 1024 * 1024 + 1)):
            with self.assertRaises(ValueError):
                package_bringup.package(bad, "board4")
        with self.assertRaises(ValueError):
            package_bringup.package(raw, "wrong")


if __name__ == "__main__":
    unittest.main()
