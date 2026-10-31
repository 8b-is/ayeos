import struct
import unittest
import zlib
import capture_board5 as b5


class Mock:
    def __init__(self):
        w = [0xffffffff] * 48
        w[:8] = [int.from_bytes(b"A5RP", "little"), 1, 192, 0x70ad305a, 1, 1, 32768, 0]
        w[32:42] = [1, 0x39, 1, 1, 10, 1, 0x59, 1, 1, 11]
        w[42:] = [1, 0, 0, 0, 0, 0]
        self.data = struct.pack("<48I", *w)
        self.info = b"A5IF" + struct.pack("<4I", 1, 192, zlib.crc32(self.data), 48) + b"board5-aud\0\0" + bytes(32)
        self.calls = []
        self.corrupt = None

    def read(self, kind, request, value=0, index=0, length=64):
        self.calls.append((kind, request))
        if (kind, request) == (0x80, 6):
            return b5.DEVICE if value == 0x100 else b5.b4.CONFIG
        if (kind, request) == (0x80, 8):
            return b"\x01"
        if request == 0xb2:
            return self.info
        if request == 0xb3:
            data = self.data[value * 48:(value + 1) * 48]
            packet = bytearray(b"A5CH" + struct.pack("<IHHI", value * 48, 48, 1, zlib.crc32(data)) + data)
            if self.corrupt is not None:
                packet[self.corrupt] ^= 1
            return bytes(packet)
        raise AssertionError("unexpected request")


class CaptureTests(unittest.TestCase):
    def test_complete_read_only_report(self):
        usb = Mock()
        data, report = b5.capture(usb)
        self.assertEqual(data, usb.data)
        self.assertEqual(report["amplifiers"][0]["raw_chip_id"], 0x39)
        self.assertEqual(report["status"], "complete")
        self.assertFalse(report["tone_enabled"])
        self.assertTrue(all(c in b5.Usb.ALLOWED for c in usb.calls))
        self.assertNotIn((0xc0, 0xb2), b5.b4.Usb.ALLOWED)

    def test_corrupt_chunk_fields_rejected(self):
        for offset in (0, 4, 8, 10, 12, 16, 63):
            usb = Mock()
            usb.corrupt = offset
            with self.assertRaises(ValueError):
                b5.capture(usb)

    def test_info_crc_and_size_rejected(self):
        for offset in (0, 4, 8, 12, 16, 20, 32, 63):
            usb = Mock()
            info = bytearray(usb.info)
            info[offset] ^= 1
            usb.info = bytes(info)
            with self.assertRaises(ValueError):
                b5.capture(usb)

    def test_invalid_report_and_missing_observations(self):
        for offset in (0, 4, 8, 16, 20, 28, 128, 148, 168, 172):
            raw = bytearray(Mock().data)
            raw[offset:offset + 4] = b"\xff" * 4
            with self.assertRaises(ValueError):
                b5.decode(raw)
        self.assertIsNone(b5.decode(Mock().data)["snapshots"]["top_clk_cfg6"])

    def test_board4_identity_stops_before_vendor_reads(self):
        usb = Mock()
        original = usb.read
        def read(kind, request, value=0, index=0, length=64):
            if request == 6 and value == 0x100:
                return b5.b4.DEVICE
            return original(kind, request, value, index, length)
        usb.read = read
        with self.assertRaises(ValueError):
            b5.capture(usb)
        self.assertEqual(usb.calls, [])


if __name__ == "__main__":
    unittest.main()
