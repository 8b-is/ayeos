import contextlib
import hashlib
import io
import json
import os
from pathlib import Path
import struct
import tempfile
import unittest
from unittest.mock import patch
import zlib

import review_tone as r
from test_capture_alpha import Mock as Alpha
from test_capture_tone import complete, rejected, Fake

REVISION = "0123456789abcdef0123456789abcdef01234567"


def linked(t=None, a=None, b=None):
    fixture = Alpha()
    t = bytearray(complete() if t is None else t)
    a = bytearray(fixture.alpha if a is None else a)
    b = bytes(fixture.data if b is None else b)
    a[64:104] = t[64:104]
    struct.pack_into("<I", a, 12, struct.unpack_from("<I", t, 12)[0])
    struct.pack_into("<I", a, 40, zlib.crc32(b))
    struct.pack_into("<3I", t, 44, zlib.crc32(b), zlib.crc32(a), struct.unpack_from("<I", b, 12)[0])
    return {"tone-report.bin": bytes(t), "alpha-report.bin": bytes(a), "board6-report.bin": b}


def manifest(files):
    return "".join(f"{hashlib.sha256(data).hexdigest()}  {name}\n"
                   for name, data in sorted(files.items()) if name != "SHA256SUMS").encode()


class Review(unittest.TestCase):
    def review(self, files, revision=REVISION, seal=True):
        files = dict(files)
        if seal:
            files["SHA256SUMS"] = manifest(files)
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            for name, data in files.items():
                (root / name).write_bytes(data)
            before = {p.name: (p.read_bytes(), p.stat().st_mtime_ns) for p in root.iterdir()}
            with patch.object(r.tone.Usb, "__init__", side_effect=AssertionError("no device access")):
                result = r.review(root, revision)
            self.assertEqual(before, {p.name: (p.read_bytes(), p.stat().st_mtime_ns) for p in root.iterdir()})
            return result

    def test_raw_success_is_register_only_and_read_only(self):
        result = self.review(linked())
        self.assertEqual(result["status"], "register_checks_passed", result)
        self.assertTrue(result["integrity_valid"])
        self.assertTrue(result["register_checks_passed"])
        self.assertEqual(result["acoustic_status"], "operator_observation_required")
        self.assertNotIn("events", json.dumps(result))

    @unittest.skipUnless(os.environ.get("AYEOS_TONE_TEST_DUMP"), "set path from scripts/check_tone.sh")
    def test_rust_tone_wire_acceptance(self):
        files = linked(t=Path(os.environ["AYEOS_TONE_TEST_DUMP"]).read_bytes())
        revision = files["tone-report.bin"][64:104].decode("ascii")
        self.assertEqual(self.review(files, revision)["status"], "register_checks_passed")

    def test_revision_requires_explicit_full_expected_release(self):
        for revision in ("9277657", "", "z"*40):
            with self.assertRaises(ValueError):
                r.review("unused", revision)
        result = self.review(linked(), "0"*40)
        self.assertTrue(result["integrity_valid"])
        self.assertEqual(result["status"], "register_checks_not_passed")
        self.assertIn("revision", result["acceptance_failures"][0])
        t = complete()
        t[64:104] = b"0"*40
        self.assertFalse(self.review(linked(t=t))["register_checks_passed"])

    def test_dirty_and_preflight_are_not_success(self):
        t = complete()
        struct.pack_into("<I", t, 12, 3)
        for data in (t, rejected()):
            result = self.review(linked(t=data))
            self.assertTrue(result["integrity_valid"], result)
            self.assertEqual(result["status"], "register_checks_not_passed")

    def test_failed_alpha_selftest_and_unexpected_fault_are_not_success(self):
        a = bytearray(Alpha().alpha)
        struct.pack_into("<2I", a, 20, 5, 2)
        result = self.review(linked(a=a))
        self.assertTrue(result["integrity_valid"], result)
        self.assertFalse(result["register_checks_passed"])
        a = bytearray(Alpha().alpha)
        struct.pack_into("<Q", a, 112, 1)
        struct.pack_into("<Q", a, 144, (0x25 << 26) | (1 << 25))
        struct.pack_into("<Q", a, 184, 2)
        result = self.review(linked(a=a))
        self.assertTrue(result["integrity_valid"], result)
        self.assertFalse(result["register_checks_passed"])

    def test_board6_incomplete_reads_profiles_restoration_and_idle(self):
        # Each remains a decodable failure report, not an invalid wire envelope.
        for changes in ({58: 0}, {21: 0}, {20: 9, 64: 9}, {1345: 0}, {1344: 7, 1347: 0}):
            b = bytearray(Alpha().data)
            for word, value in changes.items():
                struct.pack_into("<I", b, word*4, value)
            result = self.review(linked(b=b))
            with self.subTest(changes=changes):
                self.assertTrue(result["integrity_valid"], result)
                self.assertFalse(result["register_checks_passed"])

    def test_every_crosslink_and_identity_mismatch_is_invalid(self):
        for name, offset in (("tone-report.bin", 44), ("tone-report.bin", 48),
                             ("tone-report.bin", 52), ("alpha-report.bin", 40),
                             ("alpha-report.bin", 64), ("alpha-report.bin", 12)):
            files = linked()
            data = bytearray(files[name])
            data[offset] ^= 2 if offset == 12 else 1
            files[name] = bytes(data)
            with self.subTest(name=name, offset=offset):
                self.assertFalse(self.review(files)["integrity_valid"])

    def test_manifest_missing_bad_hash_membership_and_unsafe_rows(self):
        files = linked()
        for bad in (None, b"", b"bad\n", b"0"*64+b"  ../escape\n",
                    manifest(files)+manifest(files), b"0"*64+b"  SHA256SUMS\n",
                    manifest(files).replace(hashlib.sha256(files["tone-report.bin"]).hexdigest().encode(), b"0"*64)):
            bundle = dict(files)
            if bad is not None:
                bundle["SHA256SUMS"] = bad
            with self.subTest(manifest=bad is None):
                self.assertFalse(self.review(bundle, seal=False)["integrity_valid"])
        bundle = dict(files, extra=b"unlisted", SHA256SUMS=manifest(files))
        self.assertFalse(self.review(bundle, seal=False)["integrity_valid"])

    def test_capture_error_blocks_acceptance_without_exposing_text(self):
        files = linked()
        files["capture-error.txt"] = b"private handset path: not for output\n"
        result = self.review(files)
        self.assertTrue(result["capture_error_present"])
        self.assertFalse(result["register_checks_passed"])
        self.assertNotIn("private handset", json.dumps(result))

    def test_partial_truncated_overlong_and_malformed_reports(self):
        for name in r.RAW:
            for replacement in (None, b"", b"x", bytes(r.RAW[name][0]-1), bytes(r.RAW[name][0]+1)):
                files = linked()
                if replacement is None:
                    del files[name]
                else:
                    files[name] = replacement
                with self.subTest(name=name, size=None if replacement is None else len(replacement)):
                    self.assertFalse(self.review(files)["register_checks_passed"])

    def test_saved_json_not_trusted(self):
        files = linked()
        files["tone-report.json"] = json.dumps(r.tone.decode(files["tone-report.bin"])).encode()
        self.assertTrue(self.review(files)["register_checks_passed"])
        for data in (b"{}", b"[]", b"{", b"["*2000, b'{"x": 1, "x": 1}',
                     b'{"x": 1.0}', b'{"x": NaN}'):
            files["tone-report.json"] = data
            self.assertFalse(self.review(files)["integrity_valid"])

    def test_transport_packets_reconstructed_even_with_updated_manifest(self):
        files = linked()
        fake = Fake(files["tone-report.bin"])
        files["tone-info.bin"] = fake.read(0xc0, 0xb6)
        for i in range(134):
            files[f"tone-chunk-{i:03}.bin"] = fake.read(0xc0, 0xb7, i)
        self.assertTrue(self.review(files)["register_checks_passed"])
        for name, offset in (("tone-info.bin", 12), ("tone-info.bin", 32),
                             ("tone-chunk-000.bin", 4), ("tone-chunk-067.bin", 12),
                             ("tone-chunk-133.bin", 63)):
            bad = dict(files)
            data = bytearray(bad[name])
            data[offset] ^= 1
            bad[name] = bytes(data)
            self.assertFalse(self.review(bad)["integrity_valid"])
        del files["tone-chunk-133.bin"]
        self.assertFalse(self.review(files)["integrity_valid"])

    def test_bad_descriptor_and_optional_fdt(self):
        for name, data in (("tone-device.bin", r.alpha.DEVICE), ("tone-config.bin", b""),
                           ("board6-fdt.dtb", b""), ("board6-fdt.dtb", bytes(40))):
            files = linked()
            files[name] = data
            self.assertFalse(self.review(files)["integrity_valid"])
        fdt = b"\xd0\x0d\xfe\xed" + struct.pack(">I", 40) + bytes(32)
        b = bytearray(Alpha().data)
        for offset in (12, 76):
            struct.pack_into("<I", b, offset, zlib.crc32(fdt))
        files = linked(b=b)
        files["board6-fdt.dtb"] = fdt
        self.assertTrue(self.review(files)["register_checks_passed"])

    def test_symlink_fifo_directory_and_resource_bounds(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "link").symlink_to("/dev/zero")
            os.mkfifo(root / "pipe")
            (root / "dir").mkdir()
            result = r.review(root, REVISION)
            self.assertFalse(result["integrity_valid"])
            self.assertEqual(sum("regular" in s for s in result["errors"]), 3)
        with patch.object(r, "MAX_FILES", 2):
            self.assertFalse(self.review(linked())["integrity_valid"])
        with patch.object(r, "MAX_TOTAL_BYTES", 1):
            self.assertFalse(self.review(linked())["integrity_valid"])
        with patch.object(r, "MAX_MANIFEST_BYTES", 1):
            self.assertFalse(self.review(linked())["integrity_valid"])
        self.assertFalse(r.review("/nonexistent-tone-evidence", REVISION)["register_checks_passed"])

    def test_cli_json_summary_and_failure_exit(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            files = linked()
            files["SHA256SUMS"] = manifest(files)
            for name, data in files.items():
                (root / name).write_bytes(data)
            for revision, expected_exit in ((REVISION, 0), ("0"*40, 1)):
                out, err = io.StringIO(), io.StringIO()
                with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
                    code = r.main([str(root), "--expected-revision", revision])
                self.assertEqual(code, expected_exit)
                self.assertEqual(json.loads(out.getvalue())["register_checks_passed"], code == 0)
                self.assertIn("operator_observation_required", err.getvalue())


if __name__ == "__main__":
    unittest.main()
