import struct
import unittest
import zlib
import capture_board6 as b6
from test_capture_board5 import Mock as Board5


class Mock:
    def __init__(self):
        w = [0]*1600
        w[:8] = [int.from_bytes(b"A6RP", "little"), 1, 6400, 0x70ad305a, 256, 33, 32768, 1]
        w[16:64] = struct.unpack("<48I", Board5().data)
        for i in range(256):
            w[64+5*i:69+5*i] = [1, i % 128, 1, 1, 9]
        w[1344:1348] = [1, 1, 32768, 1]
        for i, (_, addr) in enumerate(b6.REGISTERS):
            value = {0x40e: 0x5aa5, 8: 0x5810, 10: 0x5810}.get(addr, addr)
            w[1376+5*i:1381+5*i] = [1, value, (1 << 22) | (6 << 16) | value, 3, 1]
        self.data = struct.pack("<1600I", *w)
        self.corrupt = None
        self.bad_info = None
        self.device = b6.DEVICE
        self.calls = []
        self.info_count = 0
        self.change_info = False

    def read(self, kind, request, value=0, index=0, length=64):
        self.calls.append((kind, request, value))
        if request == 6:
            return self.device if value == 0x100 else b6.b4.CONFIG
        if request == 8:
            return b"\x01"
        if request == 0xb2:
            self.info_count += 1
            info = bytearray(b"A6IF" + struct.pack("<4I", 1, 6400, zlib.crc32(self.data), 48) + b"board6-aud\0\0" + bytes(32))
            if self.bad_info is not None:
                info[self.bad_info] ^= 1
            if self.change_info and self.info_count == 2:
                info[12] ^= 1
            return bytes(info)
        if request == 0xb3:
            data = self.data[value*48:(value+1)*48]
            packet = bytearray(b"A6CH" + struct.pack("<IHHI", value*48, len(data), 1, zlib.crc32(data)) + data + bytes(48-len(data)))
            if self.corrupt is not None and value == 133:
                packet[self.corrupt] ^= 1
            return bytes(packet)
        raise AssertionError("unexpected request")


class CaptureTests(unittest.TestCase):
    def test_complete_transfer_and_short_final_chunk(self):
        usb = Mock()
        data, r = b6.capture(usb)
        self.assertEqual(data, usb.data)
        self.assertEqual(len(r["amplifiers"][1]["registers"]), 128)
        self.assertEqual(r["amplifiers"][1]["registers"][-1]["register"], "0x7f")
        self.assertEqual(r["pwrap"]["registers"][-1]["register"], "0x2426")
        self.assertEqual(sum(req == 0xb3 for _, req, _ in usb.calls), 134)
        self.assertTrue(all((kind, req) in b6.b5.Usb.ALLOWED for kind, req, _ in usb.calls))

    def test_info_and_short_chunk_integrity(self):
        for offset in (0, 4, 8, 10, 12, 16, 31, 32, 63):
            usb = Mock()
            usb.corrupt = offset
            with self.assertRaises(ValueError):
                b6.capture(usb)
        for offset in (0, 4, 8, 12, 16, 20, 32, 63):
            usb = Mock()
            usb.bad_info = offset
            with self.assertRaises(ValueError):
                b6.capture(usb)
        usb = Mock()
        usb.change_info = True
        with self.assertRaises(ValueError):
            b6.capture(usb)

    def test_prior_identity_rejected_before_vendor_reads(self):
        for device in (b6.b4.DEVICE, b6.b5.DEVICE):
            usb = Mock()
            usb.device = device
            with self.assertRaises(ValueError):
                b6.capture(usb)
            self.assertEqual(usb.calls, [(0x80, 6, 0x100)])

    def test_invalid_schema_and_completed_observations(self):
        for word, value in ((1, 2), (2, 6399), (7, 0), (8, 1), (1541, 1),
                            (64, 99), (65, 256), (67, 0), (68, 0),
                            (1344, 11), (1360, 1), (1377, 65536),
                            (1378, (2 << 16) | 0x5aa5), (1380, 0)):
            data = bytearray(Mock().data)
            struct.pack_into("<I", data, word*4, value)
            with self.assertRaises(ValueError, msg=f"word {word}"):
                b6.decode(data)
        for size in (0, 192, 6399, 6401):
            with self.assertRaises(ValueError):
                b6.decode(bytes(size))

    def test_partial_capture_retains_unknown_as_null(self):
        w = list(struct.unpack("<1600I", Mock().data))
        w[20] = 9
        w[64:69] = [9, 0xffffffff, 2, 0xffffffff, 5]
        for i in range(1, 256):
            w[64+5*i:69+5*i] = [0] + [0xffffffff]*4
        r = b6.decode(struct.pack("<1600I", *w))
        self.assertEqual(r["i2c_summary"]["status"], "nack")
        self.assertIsNone(r["amplifiers"][0]["registers"][0]["value"])
        self.assertEqual(r["amplifiers"][1]["registers"][-1]["status"], "not_run")
        self.assertEqual(r["pwrap"]["status"], "complete")


if __name__ == "__main__":
    unittest.main()
