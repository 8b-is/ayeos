import struct
import unittest
import zlib
import os
from pathlib import Path
import capture_alpha as a
from test_capture_board6 import Mock as Board6


class Mock(Board6):
    def __init__(self):
        super().__init__()
        self.device = a.DEVICE
        self.alpha = bytearray(1024)
        struct.pack_into("<8I", self.alpha, 0, int.from_bytes(b"ADRP", "little"), 1, 1024, 1, 3, 7, 0, 0)
        struct.pack_into("<I", self.alpha, 40, zlib.crc32(self.data))
        self.alpha[64:104] = b"0123456789"*4
        struct.pack_into("<8Q", self.alpha, 128, 4, 1, (0x3c << 26) | (1 << 25) | 0xa8, 0x40080000, 0, 5, 0x40100000, 1)
        struct.pack_into("<2Q", self.alpha, 192, a.REFERENCE_HASH, 1024)
        self.alpha_reads = 0
        self.bad = None

    def read(self, kind, request, value=0, index=0, length=64):
        if request not in (0xb4, 0xb5):
            return super().read(kind, request, value, index, length)
        self.calls.append((kind, request, value))
        packet = bytearray(64)
        if request == 0xb4:
            self.alpha_reads += 1
            packet[:4] = b"ADIF"
            struct.pack_into("<4I", packet, 4, 1, 1024, zlib.crc32(self.alpha), 48)
            packet[20:32] = b"audio-alpha1"
        else:
            payload = self.alpha[value*48:(value+1)*48]
            packet[:4] = b"ADCH"
            struct.pack_into("<IHHI", packet, 4, value*48, len(payload), 1, zlib.crc32(payload))
            packet[16:16+len(payload)] = payload
        if self.bad == "changed" and request == 0xb4 and self.alpha_reads == 2:
            packet[12] ^= 1
        if self.bad == "chunk" and request == 0xb5:
            packet[12] ^= 1
        if self.bad == "padding" and request == 0xb5 and value == 21:
            packet[-1] = 1
        if self.bad == "size" and request == 0xb4:
            packet[8:12] = b"\xff"*4
        return bytes(packet)


class AlphaTests(unittest.TestCase):
    def test_capture_and_reference_evidence(self):
        usb = Mock()
        data, report, _, board = a.capture(usb)
        self.assertEqual(len(data), 1024)
        self.assertEqual(report["tests"], dict(crc32="pass", marine_fixture="pass", exception="pass"))
        self.assertFalse(report["fault"]["far_valid"])
        self.assertEqual(report["board6_crc32"], board["crc32"])
        self.assertFalse(report["capabilities"]["live_pcm"])
        self.assertEqual(sum(c[1] == 0xb5 for c in usb.calls), 22)
        self.assertTrue(all(c[:2] in a.Usb.ALLOWED for c in usb.calls))

    def test_reject_descriptor_and_corrupted_transport(self):
        for bad in ("changed", "chunk", "padding", "size", "device"):
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                usb = Mock()
                usb.bad = bad
                if bad == "device":
                    usb.device = a.b6.DEVICE
                a.capture(usb)

    def test_all_truncated_prefixes_rejected(self):
        data = Mock().alpha
        for n in range(len(data)):
            with self.assertRaises(ValueError):
                a.decode(data[:n])

    def test_malformed_schema_and_cross_report_rejected(self):
        changes = [(4, 2), (8, 1), (12, 0), (16, 7), (20, 0), (24, 1),
                   (28, 17), (40, 1), (44, 1), (64, 0), (104, 1), (112, 1),
                   (120, 1), (128, 5), (136, 3), (144, 0), (152, 1),
                   (176, 1), (184, 3), (192, 0), (200, 1), (208, 1),
                   (256, 1), (768, 1), (1023, 1)]
        for offset, value in changes:
            with self.subTest(offset=offset), self.assertRaises((ValueError, UnicodeError)):
                usb = Mock()
                usb.alpha[offset] = value
                a.capture(usb)

    def test_failed_fixture_is_evidence_not_success(self):
        usb = Mock()
        struct.pack_into("<2I", usb.alpha, 20, 5, 2)
        struct.pack_into("<Q", usb.alpha, 192, 99)
        report = a.capture(usb)[1]
        self.assertEqual(report["tests"]["marine_fixture"], "fail")

    def test_partial_packets_preserved_on_failure(self):
        usb = Mock()
        usb.bad = "chunk"
        saved = {}
        with self.assertRaises(ValueError):
            a.capture(usb, lambda n, b: saved.__setitem__(n, b))
        self.assertIn("alpha-info.bin", saved)
        self.assertIn("alpha-chunk-00.bin", saved)

    @unittest.skipUnless(os.environ.get("AYEOS_ALPHA_TEST_DUMP"), "run scripts/check_alpha.sh for Rust wire fixture")
    def test_rust_wire_fixture(self):
        report = a.decode(Path(os.environ["AYEOS_ALPHA_TEST_DUMP"]).read_bytes())
        self.assertEqual(report["dropped_events"], 4)
        self.assertEqual([e["sequence"] for e in report["events"]], list(range(5, 21)))
        self.assertEqual(report["marine"]["fnv64"], "800d3c2f169c4722")
        self.assertEqual(report["revision"], "0123456789"*4)


if __name__ == "__main__":
    unittest.main()
