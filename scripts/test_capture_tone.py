import os
from pathlib import Path
import struct
import subprocess
import tempfile
import re
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


def complete():
    """Synthetic wire for negative tests; independent Rust wire is checked too."""
    b = rejected()
    struct.pack_into("<6I", b, 16, 1, 0, 0, t.OFF_MASK, 1, len(t.COMPLETE_SEQUENCE))
    tick, opened, closed, stops = 100, 0, 0, 0
    values = {0x040e: 0x5aa5, 0x000a: 0x5810, 0x1ac4: 1, 0x1ad2: 0x8000}
    for i, (kind, phase, addr, mask, expected) in enumerate(t.COMPLETE_SEQUENCE):
        if phase == 2:
            opened = tick
        if phase == 3 and not stops:
            tick = opened + 3250001
        if kind == 1:
            actual = values.get(addr, 0) if phase == 0 else dict((a, v) for a, m, v in t.OFF)[addr]
        elif kind == 3:
            actual = 0x23 if phase == 0 else 0
        elif kind == 5:
            actual = expected * 13
        else:
            actual = expected
        struct.pack_into("<QBBHHHII", b, 256 + 24*i,
                         tick, kind, phase, 0, addr, mask, expected, actual)
        tick += actual + 1 if kind == 5 else 1
        if phase == 3:
            stops += 1
            if stops == 4:
                closed = tick
    struct.pack_into("<5Q", b, 112, 13000000, 100, opened, closed, tick)
    return b


def event_at(b, phase, kind=None):
    return next(256 + i*24 for i in range(struct.unpack_from("<I", b, 36)[0])
                if b[256+i*24+9] == phase and (kind is None or b[256+i*24+8] == kind))


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
    def test_synthetic_success_and_release_identity_is_separate_policy(self):
        b = complete()
        self.assertEqual(t.decode(b)["outcome"], "complete")
        b3e = bytearray(b)
        n = struct.unpack_from("<I", b3e, 36)[0]
        for i in range(n):
            at = 256 + i * 24
            kind, addr, expected = b3e[at + 8], struct.unpack_from("<H", b3e, at + 12)[0], struct.unpack_from("<I", b3e, at + 16)[0]
            if kind in (3, 4) and addr in (0x5802, 0x5902) and expected == 0:
                struct.pack_into("<I", b3e, at + 20, 0x3e)
            if kind == 2 and addr == 0x2488 and expected == 0x10b:
                struct.pack_into("<I", b3e, at + 20, 0x000b)
                for bad_zcd in (0x000a, 0x020b):
                    bad = bytearray(b3e)
                    struct.pack_into("<I", bad, at + 20, bad_zcd)
                    with self.assertRaisesRegex(ValueError, "mismatching readback"):
                        t.decode(bad)
        self.assertEqual(t.decode(b3e)["outcome"], "complete")
        self.assertTrue(t.decode(b3e)["verified_off"])
        b[64:104] = b"0" * 40
        self.assertEqual(t.decode(b)["revision"], "0" * 40)
        struct.pack_into("<I", b, 12, 3)
        self.assertTrue(t.decode(b)["dirty"])

    def test_all_four_outcomes_and_failed_timer_evidence(self):
        b = complete()
        struct.pack_into("<I", b, 16, 3)
        struct.pack_into("<I", b, 20, 2)
        # A failed timer may be unavailable or go backwards, including cleanup.
        struct.pack_into("<5Q", b, 112, 13000000, 100, 200, 0, 0)
        struct.pack_into("<Q", b, event_at(b, 3), 0)
        self.assertEqual(t.decode(b)["outcome"], "aborted")
        struct.pack_into("<I", b, 32, 0)
        struct.pack_into("<I", b, 24, 1)
        self.assertFalse(t.decode(b)["controllers_restored"])
        struct.pack_into("<I", b, 16, 4)
        struct.pack_into("<I", b, 28, t.OFF_MASK & ~1)
        at = event_at(b, 4)
        struct.pack_into("<H", b, at + 10, 4)
        struct.pack_into("<I", b, at + 20, 0xffffffff)
        self.assertEqual(t.decode(b)["outcome"], "off_unverified")
        for restored in (0, 1):
            b = rejected()
            struct.pack_into("<I", b, 32, restored)
            self.assertEqual(t.decode(b)["outcome"], "preflight_rejected")

    def test_missing_complete_sequence_rejected(self):
        original = complete()
        n = struct.unpack_from("<I", original, 36)[0]
        for i in range(n):
            b = bytearray(original)
            at = 256 + i*24
            b[at:-24] = b[at+24:]
            b[-24:] = bytes(24)
            struct.pack_into("<I", b, 36, n-1)
            with self.subTest(removed=i), self.assertRaises(ValueError):
                t.decode(b)
        b = bytearray(original)
        b[256:] = original[256+(n-17)*24:256+n*24] + bytes(t.SIZE-256-17*24)
        struct.pack_into("<I", b, 36, 17)
        with self.assertRaisesRegex(ValueError, "sequence"):
            t.decode(b)

    def test_timestamp_and_gate_contradictions_rejected(self):
        b = complete()
        end = struct.unpack_from("<Q", b, 144)[0]
        changes = [(event_at(b, 4), "Q", end+1), (event_at(b, 1), "Q", 0),
                   (event_at(b, 2), "Q", 1), (event_at(b, 3), "Q", 200),
                   (event_at(b, 0, 1)+20, "I", 0), (136, "Q", end)]
        for at, fmt, value in changes:
            bad = bytearray(b)
            struct.pack_into("<"+fmt, bad, at, value)
            with self.subTest(at=at), self.assertRaises(ValueError):
                t.decode(bad)

    def test_register_width_masks_delay_and_error_invariants(self):
        b = complete()
        pa, pm = event_at(b, 4, 3), event_at(b, 4, 1)
        write, delay = event_at(b, 1, 2), event_at(b, 1, 5)
        changes = [(pa+20, "I", 0x100), (pm+20, "I", 0x10000),
                   (pa+16, "I", 256), (pm+14, "H", 1), (pa+14, "H", 0),
                   (write+14, "H", 0), (write+10, "H", 10),
                   (pm+10, "H", 10), (delay+12, "H", 2),
                   (delay+20, "I", 0), (delay+16, "I", 0),
                   (pa+10, "H", 4)]
        for at, fmt, value in changes:
            bad = bytearray(b)
            struct.pack_into("<"+fmt, bad, at, value)
            with self.subTest(at=at), self.assertRaises(ValueError):
                t.decode(bad)

    def test_outcome_cleanup_bookkeeping(self):
        b = complete()
        struct.pack_into("<I", b, 16, 3)
        struct.pack_into("<I", b, 20, 1)
        struct.pack_into("<I", b, 32, 0)
        with self.assertRaisesRegex(ValueError, "restoration"):
            t.decode(b)
        for at in (24, 144):
            b = rejected()
            struct.pack_into("<I", b, at, 1)
            with self.assertRaises(ValueError):
                t.decode(b)

    def test_overlong_and_revision_syntax_rejected(self):
        with self.assertRaises(ValueError):
            t.decode(complete() + b"\0")
        for value in (b"x", b"\xff"):
            b = complete()
            b[64:65] = value
            with self.assertRaises(ValueError):
                t.decode(b)

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

    @unittest.skipUnless(os.environ.get("AYEOS_TONE_TEST_DUMP"), "run scripts/check_tone.sh for Rust failures")
    def test_real_producer_failure_journals_remain_decodable(self):
        # Reuse the actual Rust bus model, not a second Python failure simulator.
        # Compile a temporary copy plus an export test; never edit the producer.
        root = Path(__file__).resolve().parents[1]
        source_dir = root / "baremetal/tests"
        source = (source_dir / "tone.rs").read_text()
        source = re.sub(r'#\[path = "([^"]+)"\]',
                        lambda m: '#[path = "' + str((source_dir / m[1]).resolve()) + '"]', source)
        for name in ("tone_board_cases", "tone_bus_cases"):
            source = source.replace(f"mod {name};", f'#[path = "{source_dir / (name + ".rs")}"] mod {name};')
        source += r'''
#[test]
fn export_failure_reports() {
    let mut bytes = Vec::new();
    let mut save = |m: &mut Model, gates| {
        let report = tone::run(m, gates);
        assert_ne!(report.outcome, 1);
        bytes.extend_from_slice(&report.encode(
            "0123456789abcdef0123456789abcdef01234567", false, 1, 2, 3));
    };
    let mut baseline = Model::new();
    tone::run(&mut baseline, true);
    for after in [false, true] {
        for fail in 1..=baseline.ops {
            let mut m = Model::new();
            m.after = after; m.fail = fail;
            save(&mut m, true);
        }
    }
    for broken in [1, 0x58, 0x59] {
        let mut m = Model::new(); m.broken = broken; save(&mut m, true);
    }
    let mut m = Model::new(); m.freeze = true; save(&mut m, true);
    let mut m = Model::new(); m.restore_bad = true; save(&mut m, true);
    let mut m = Model::new(); m.corrupt = Some(0x22c2); save(&mut m, true);
    let mut m = Model::new(); m.step = tone::HZ; save(&mut m, true);
    let mut m = Model::new(); m.restore_bad = true; save(&mut m, false);
    let mut m = Model::new(); m.hz = 0; save(&mut m, true);
    let mut m = Model::new(); m.step = 0; save(&mut m, true);
    std::fs::write(std::env::var("AYEOS_FAILURE_DUMP").unwrap(), bytes).unwrap();
}
'''
        with tempfile.TemporaryDirectory(prefix="ayeos-tone-failures-") as temp:
            temp = Path(temp)
            (temp / "cases.rs").write_text(source)
            subprocess.run(["rustc", "--edition=2021", "-O", "--test",
                            *[arg for feature in ("audio-report", "board6-report", "alpha-report", "tone-report")
                              for arg in ("--cfg", f'feature="{feature}"')],
                            str(temp / "cases.rs"), "-o", str(temp / "cases")],
                           check=True, capture_output=True, timeout=120)
            subprocess.run([str(temp / "cases"), "--exact", "export_failure_reports"],
                           env=dict(os.environ, AYEOS_FAILURE_DUMP=str(temp / "failures.bin")),
                           check=True, capture_output=True, timeout=60)
            data = (temp / "failures.bin").read_bytes()
        self.assertEqual(len(data) % t.SIZE, 0)
        self.assertGreater(len(data) // t.SIZE, 500)
        outcomes = set()
        for offset in range(0, len(data), t.SIZE):
            with self.subTest(case=offset // t.SIZE):
                outcomes.add(t.decode(data[offset:offset+t.SIZE])["outcome"])
        self.assertEqual(outcomes, {"preflight_rejected", "aborted", "off_unverified"})
        print(f"Decoded {len(data) // t.SIZE} real Rust failure journals")

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

    def test_transport_headers_snapshot_and_all_packet_regions(self):
        class Alter(Fake):
            info_reads = 0

            def read(self, kind, request, value=0, index=0, length=64):
                data = bytearray(super().read(kind, request, value, index, length))
                if request == 0xb6:
                    self.info_reads += 1
                if request == target and (target == 0xb6 or value == number):
                    if offset == "short":
                        return bytes(data[:-1])
                    if offset == "changed":
                        if self.info_reads == 2:
                            data[12] ^= 1
                    else:
                        data[offset] ^= 1
                return bytes(data)

        cases = [(0xb6, 0, at) for at in (0, 4, 8, 12, 16, 20, 32, 63, "short", "changed")]
        cases += [(0xb7, n, at) for n in (0, 67, 133)
                  for at in (0, 4, 8, 10, 12, 16, 63, "short")]
        for target, number, offset in cases:
            with self.subTest(target=target, number=number, offset=offset), self.assertRaises(ValueError):
                t.capture(Alter(rejected()), lambda name, data: None)

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
