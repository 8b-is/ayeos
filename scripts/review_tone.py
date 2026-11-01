#!/usr/bin/env python3
"""Offline, read-only tone1 evidence review; JSON stdout, concise summary stderr."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import struct
import sys
import zlib

# Even if the helpers are colocated with evidence, imports must not add caches.
sys.dont_write_bytecode = True

import capture_tone as tone
import capture_alpha as alpha
import capture_board6 as board6

MAX_FILES = 256
MAX_FILE_BYTES = 2 * 1024 * 1024
MAX_TOTAL_BYTES = 16 * 1024 * 1024
MAX_MANIFEST_BYTES = 64 * 1024
SAFE_NAME = re.compile(r"[A-Za-z0-9][A-Za-z0-9_.-]{0,127}")
RAW = {"tone-report.bin": (tone.SIZE, tone.decode),
       "alpha-report.bin": (alpha.SIZE, alpha.decode),
       "board6-report.bin": (board6.SIZE, board6.decode)}


def read_bundle(directory):
    """Bound work/memory and never follow a bundle entry symlink or open a device.

    O_NONBLOCK prevents a substituted FIFO from blocking before fstat. The
    directory fd anchors all opens; trusted ancestors and a quiescent evidence
    directory are still required (this is not a filesystem snapshot).
    """
    files, problems = {}, []
    fd = os.open(directory, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        names = []
        with os.scandir(fd) as entries:
            for entry in entries:
                names.append(entry.name)
                if len(names) > MAX_FILES:
                    raise ValueError("bundle exceeds file-count bound")
        if any(not SAFE_NAME.fullmatch(name) for name in names):
            raise ValueError("bundle contains an unsafe filename")
        total = 0
        for name in sorted(names):
            limit = (RAW[name][0] if name in RAW else
                     MAX_MANIFEST_BYTES if name == "SHA256SUMS" else MAX_FILE_BYTES)
            try:
                # Check without opening nonregular files, then recheck the fd.
                if not stat.S_ISREG(os.stat(name, dir_fd=fd, follow_symlinks=False).st_mode):
                    raise ValueError("not a regular nonsymlink file")
                child = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC | os.O_NONBLOCK,
                                dir_fd=fd)
                with os.fdopen(child, "rb") as stream:
                    before = os.fstat(stream.fileno())
                    read_limit = min(limit, MAX_TOTAL_BYTES - total)
                    if not stat.S_ISREG(before.st_mode) or before.st_size > read_limit:
                        raise ValueError("nonregular, oversized or beyond remaining byte budget")
                    data = stream.read(read_limit + 1)
                    total += len(data)  # Charge even reads subsequently found to be inconsistent.
                    after = os.fstat(stream.fileno())
                if total > MAX_TOTAL_BYTES:
                    raise ValueError("bundle exceeds total-byte bound")
                if len(data) > limit or (before.st_size, before.st_mtime_ns, before.st_ctime_ns) != (
                        after.st_size, after.st_mtime_ns, after.st_ctime_ns) or len(data) != after.st_size:
                    raise ValueError("oversized or changed during read")
                files[name] = data
            except (OSError, ValueError) as error:
                problems.append(f"{name}: {error}")
                if total > MAX_TOTAL_BYTES:
                    break
        return files, set(names), problems
    finally:
        os.close(fd)


def check_manifest(files, names):
    data = files.get("SHA256SUMS")
    if data is None:
        return ["SHA256SUMS: missing or unreadable"]
    try:
        entries = {}
        for line in data.decode("ascii").splitlines():
            match = re.fullmatch(r"([0-9a-fA-F]{64})  ([A-Za-z0-9][A-Za-z0-9_.-]{0,127})", line)
            if not match:
                raise ValueError("malformed row (only safe local basenames allowed)")
            digest, name = match.groups()
            if name in entries or name == "SHA256SUMS":
                raise ValueError("duplicate or self-referential entry")
            entries[name] = digest.lower()
        if set(entries) != names - {"SHA256SUMS"}:
            raise ValueError("membership differs from bundle files")
        problems = []
        for name, expected in entries.items():
            if name not in files or hashlib.sha256(files[name]).hexdigest() != expected:
                problems.append(f"SHA256SUMS: hash mismatch or unreadable {name}")
        return problems
    except (UnicodeError, ValueError) as error:
        return [f"SHA256SUMS: {error}"]


def check_transport(files, prefix, magic, identity, size, digits):
    """Raw-only review is allowed; any saved envelope requires its whole group."""
    info_name = f"{prefix}-info.bin"
    chunk_names = {name for name in files if name.startswith(f"{prefix}-chunk-")}
    if info_name not in files and not chunk_names:
        return
    expected_names = {f"{prefix}-chunk-{i:0{digits}}.bin" for i in range((size + 47) // 48)}
    if chunk_names != expected_names:
        raise ValueError(f"{prefix}: incomplete or extra transport packets")
    info = files.get(info_name, b"")
    if (len(info) != 64 or info[:4] != magic + b"IF"
            or struct.unpack_from("<2I", info, 4) != (1, size)
            or struct.unpack_from("<I", info, 16)[0] != 48
            or info[20:32] != identity or any(info[32:])):
        raise ValueError(f"{prefix}: invalid saved info")
    rebuilt = bytearray()
    for i in range((size + 47) // 48):
        packet = files[f"{prefix}-chunk-{i:0{digits}}.bin"]
        if packet[:4] != magic + b"CH":
            raise ValueError(f"{prefix}: invalid saved chunk magic")
        rebuilt.extend(tone.b4.chunk(b"A4CH" + packet[4:], i * 48, size))
    if (bytes(rebuilt) != files.get(f"{prefix}-report.bin")
            or zlib.crc32(rebuilt) != struct.unpack_from("<I", info, 12)[0]):
        raise ValueError(f"{prefix}: transport/raw report mismatch")


def strict_json(data):
    def pairs(items):
        result = {}
        for key, value in items:
            if key in result:
                raise ValueError("duplicate JSON key")
            result[key] = value
        return result

    def noninteger(_):
        raise ValueError("noninteger JSON number")

    return json.loads(data, object_pairs_hook=pairs,
                      parse_float=noninteger, parse_constant=noninteger)


def review(directory, expected_revision):
    if not re.fullmatch(r"[0-9a-fA-F]{40}", expected_revision):
        raise ValueError("expected revision must be a full 40-hex release revision")
    result = dict(schema="tone-review-v1", expected_revision=expected_revision.lower(),
                  status="evidence_invalid", register_checks_passed=False,
                  integrity_valid=False, acoustic_status="operator_observation_required",
                  errors=[], acceptance_failures=[], reports={}, capture_error_present=False)
    errors, failures = result["errors"], result["acceptance_failures"]
    try:
        files, names, problems = read_bundle(directory)
    except (OSError, ValueError) as error:
        errors.append(f"bundle: {error}")
        return result
    errors.extend(problems)
    errors.extend(check_manifest(files, names))
    result["capture_error_present"] = "capture-error.txt" in names
    if result["capture_error_present"]:
        errors.append("capture-error.txt is present; capture did not finish successfully")
        # Deliberately do not echo potentially private device paths/error text.
        if "capture-error.txt" in files:
            result["capture_error"] = dict(bytes=len(files["capture-error.txt"]),
                sha256=hashlib.sha256(files["capture-error.txt"]).hexdigest())
    decoded = {}
    for name, (size, decoder) in RAW.items():
        try:
            data = files.get(name, b"")
            if len(data) != size:
                raise ValueError(f"expected {size} bytes")
            report = decoder(data)
            decoded[name] = report
            result["reports"][name] = dict(bytes=size, sha256=report["sha256"], crc32=report["crc32"])
        except (ValueError, UnicodeError, struct.error) as error:
            errors.append(f"{name}: {error}")
    for prefix, magic, identity, size, digits in (
            ("tone", b"AT", b"audio-tone1\0", tone.SIZE, 3),
            ("alpha", b"AD", b"audio-alpha1", alpha.SIZE, 2),
            ("board6", b"A6", b"board6-aud\0\0", board6.SIZE, 3)):
        try:
            check_transport(files, prefix, magic, identity, size, digits)
        except (ValueError, struct.error) as error:
            errors.append(f"transport: {error}")
    for name, expected in (("tone-device.bin", tone.DEVICE), ("tone-config.bin", tone.b4.CONFIG)):
        if name in files and files[name] != expected:
            errors.append(f"{name}: unexpected USB descriptor")
    if len(decoded) != 3:
        return result
    t, a, b = (decoded[name] for name in RAW)
    if (t["revision"] != a["revision"] or t["dirty"] != a["dirty"]
            or t["alpha_crc32"] != a["crc32"] or t["board6_crc32"] != b["crc32"]
            or a["board6_crc32"] != b["crc32"] or t["fdt_crc32"] != b["fdt_crc32"]):
        errors.append("tone/alpha/Board6 crosslinks disagree")
    if "board6-fdt.dtb" in files:
        fdt = files["board6-fdt.dtb"]
        if (not 40 <= len(fdt) <= tone.b4.MAX_BYTES or fdt[:4] != b"\xd0\x0d\xfe\xed"
                or struct.unpack_from(">I", fdt, 4)[0] != len(fdt)
                or f"{zlib.crc32(fdt):08x}" != b["fdt_crc32"]):
            errors.append("optional FDT header/size/CRC mismatch")
    # Saved JSON is convenience output, not an authority. Re-decode and compare.
    for name, report in decoded.items():
        json_name = name[:-4] + ".json"
        if json_name in files:
            try:
                if strict_json(files[json_name]) != report:
                    raise ValueError("does not match decoded raw report")
            except (ValueError, UnicodeError, RecursionError) as error:
                errors.append(f"{json_name}: {error}")
    if t["revision"].lower() != expected_revision.lower():
        failures.append("release revision differs from explicitly expected revision")
    if t["dirty"] or a["dirty"]:
        failures.append("build is dirty")
    if (t["outcome"] != "complete" or not t["verified_off"] or not t["controllers_restored"]):
        failures.append("tone is not complete with verified Off and restored controllers")
    if any(value != "pass" for value in a["tests"].values()) or a["unexpected_faults"]:
        failures.append("alpha requires tests 7/0 and zero unexpected faults")
    i2c, pwrap = b["i2c_summary"], b["pwrap"]
    if (i2c["status"] != "complete" or not i2c["fdt_profile_matched"]
            or not i2c["configuration_restored"] or i2c["poll_budget"] != 32768
            or any(chip["status"] != "complete" for chip in i2c["amplifiers"])
            or any(reg["status"] != "complete" for chip in b["amplifiers"] for reg in chip["registers"])
            or pwrap["status"] != "complete" or not pwrap["fdt_profile_matched"]
            or not pwrap["last_observed_idle"]
            or any(reg["status"] != "complete" for reg in pwrap["registers"])):
        failures.append("pre-tone Board6 reads/profiles/restoration/idle are not all complete")
    result.update(integrity_valid=not errors, revision=t["revision"], dirty=t["dirty"],
                  tone_outcome=t["outcome"], verified_off=t["verified_off"],
                  controllers_restored=t["controllers_restored"])
    if not errors:
        result["status"] = "register_checks_not_passed" if failures else "register_checks_passed"
        result["register_checks_passed"] = not failures
    return result


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path, help="existing private capture directory (never modified)")
    parser.add_argument("--expected-revision", required=True, help="full revision of the intended release image")
    args = parser.parse_args(argv)
    if not re.fullmatch(r"[0-9a-fA-F]{40}", args.expected_revision):
        parser.error("--expected-revision must contain exactly 40 hexadecimal digits")
    result = review(args.directory, args.expected_revision)
    print(json.dumps(result, indent=2))
    print(f"{result['status']}; acoustic: {result['acoustic_status']}; "
          f"{len(result['errors'])} evidence errors, {len(result['acceptance_failures'])} acceptance failures",
          file=sys.stderr)
    return 0 if result["register_checks_passed"] else 1


if __name__ == "__main__":
    sys.exit(main())
