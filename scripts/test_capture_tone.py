import os
from pathlib import Path
import struct
import unittest
import zlib
from unittest.mock import patch
import capture_tone as t


def rejected():
    b = bytearray(t.SIZE)
    struct.pack_into("<16I", b, 0, int.from_bytes(b"ATRP", "little"), 1, t.SIZE,
                     1, 2, 1, 0, 0, 0, 0, 0, 1, 2, 3, t.OFF_MASK, 250000)
    b[64:104] = b"0123456789abcdef0123456789abcdef01234567"
    return b


class Fake:
    def __init__(self, data):
        self.data = data
        self.corrupt = None
        self.device = t.DEVICE

    def read(self, kind, request, value=0, index=0, length=64):
        if kind == 0x80:
            return self.device if value == 0x100 else t.b4.CONFIG if value == 0x200 else b"\x01"
        if request == 0xb6:
            return b"ATIF" + struct.pack("<4I", 1, t.SIZE, zlib.crc32(self.data), 48) + b"audio-tone1\0" + bytes(32)
        assert request == 0xb7
        offset = value*48
        block = self.data[offset:offset+48]
        crc = zlib.crc32(block) ^ (1 if value == self.corrupt else 0)
        return b"ATCH" + struct.pack("<IHHI", offset, len(block), 1, crc) + block + bytes(48-len(block))


class Tone(unittest.TestCase):
    def test_preflight_failure_is_not_off(self):
        r = t.decode(rejected())
        self.assertEqual(r["outcome"], "preflight_rejected")
        self.assertFalse(r["verified_off"])

    def test_all_truncated_lengths_rejected(self):
        b = rejected()
        for n in range(t.SIZE):
            with self.assertRaises(ValueError):
                t.decode(b[:n])

    def test_false_off_reserved_and_invalid_bounds_rejected(self):
        for offset, value in [(16, 1), (28, t.OFF_MASK), (36, 257), (40, 1), (152, 1)]:
            b = rejected()
            struct.pack_into("<I", b, offset, value)
            with self.assertRaises(ValueError):
                t.decode(b)

    @unittest.skipUnless(os.environ.get("AYEOS_TONE_TEST_DUMP"), "run scripts/check_tone.sh for Rust wire")
    def test_rust_success_and_forged_readbacks(self):
        b = Path(os.environ["AYEOS_TONE_TEST_DUMP"]).read_bytes()
        r = t.decode(b)
        self.assertEqual(r["outcome"], "complete")
        self.assertTrue(r["verified_off"])
        self.assertGreaterEqual(r["interval_ticks"], 3250000)
        n = struct.unpack_from("<I", b, 36)[0]
        for i in range(n):
            at = 256+i*24
            if b[at+9] == 4:
                bad = bytearray(b)
                # Corrupt the final read while retaining the claimed Off mask.
                address = struct.unpack_from("<H", b, at+12)[0]
                mask = 255 if b[at+8] == 3 else next(m for a, m, _ in t.OFF if a == address)
                actual = struct.unpack_from("<I", b, at+20)[0]
                struct.pack_into("<I", bad, at+20, actual ^ (mask & -mask))
                with self.assertRaises(ValueError):
                    t.decode(bad)

    def test_partial_packet_retained_and_identity_checked_before_vendor_reads(self):
        fake = Fake(rejected())
        saved = {}
        fake.corrupt = 3
        with self.assertRaises(ValueError):
            t.capture(fake, saved.__setitem__)
        self.assertIn("tone-chunk-003.bin", saved)
        self.assertNotIn("tone-report.bin", saved)
        fake.device = t.alpha.DEVICE
        with self.assertRaises(ValueError):
            t.capture(fake, saved.__setitem__)

    def test_complete_capture_checks_cross_report_links(self):
        fake = Fake(rejected())
        saved = {}
        diag = dict(revision="0123456789abcdef0123456789abcdef01234567", dirty=False, crc32="00000002")
        board = dict(crc32="00000001", fdt_crc32="00000003")
        with patch.object(t.alpha, "capture", return_value=(b"", diag, b"", board)) as call:
            t.capture(fake, saved.__setitem__)
            self.assertEqual(call.call_args.kwargs["expected_device"], t.DEVICE)
            self.assertEqual(saved["tone-report.bin"], bytes(rejected()))
            board["fdt_crc32"] = "ffffffff"
            with self.assertRaises(ValueError):
                t.capture(fake, saved.__setitem__)


if __name__ == "__main__":
    unittest.main()
