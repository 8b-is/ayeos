#!/usr/bin/env python3
"""Package an explicitly supplied ARM64 kernel into a development boot image.

Uses the same Android-v4 envelope as the verified 5.4 image. Never writes the
release firmware directory or produces a publisher-authentication signature.
The destination must not already exist.
"""
import argparse
import gzip
import hashlib
import json
from pathlib import Path
import struct


def package(kernel, board="board3"):
    if board not in ("board3", "board4", "board5", "board6", "audio-alpha1", "audio-tone1"):
        raise ValueError("unknown diagnostic identity")
    if not 64 <= len(kernel) <= 4 * 1024 * 1024 or kernel[56:60] != b"ARM\x64":
        raise ValueError("expected a raw ARM64 boot kernel, not an ELF or gzip file")
    compressed = gzip.compress(kernel, compresslevel=9, mtime=0)
    header = bytearray(4096)
    header[:8] = b"ANDROID!"
    struct.pack_into("<I", header, 8, len(compressed))
    struct.pack_into("<I", header, 16, (15 << 25) | (26 << 4) | 10)
    struct.pack_into("<I", header, 20, 1580)
    struct.pack_into("<I", header, 40, 4)
    version = "5.6.0" if board in ("audio-alpha1", "audio-tone1") else "5.4.0"
    cmdline = f"ayeos.baremetal=v{version}-studio-{board} ayeos.board_report=1".encode()
    if board in ("board4", "board5", "board6", "audio-alpha1", "audio-tone1"):
        cmdline += b" ayeos.usb_report=1"
    if board in ("board5", "board6", "audio-alpha1", "audio-tone1"):
        cmdline += b" ayeos.audio_report=1"
    if board in ("audio-alpha1", "audio-tone1"):
        cmdline += b" ayeos.alpha_report=1"
    if board == "audio-tone1":
        cmdline += b" ayeos.tone_report=1"
    cmdline += b"\0"
    header[44:44 + len(cmdline)] = cmdline
    return bytes(header) + compressed + bytes((-len(compressed)) % 4096)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--kernel", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--board", choices=("board3", "board4", "board5", "board6", "audio-alpha1", "audio-tone1"), default="board3")
    args = parser.parse_args()
    with args.kernel.open("rb") as f:
        kernel = f.read(4 * 1024 * 1024 + 1)
    image = package(kernel, args.board)
    args.output.mkdir(parents=True, exist_ok=False)
    (args.output / f"studio-{args.board}.img").write_bytes(image)
    metadata = {
        "development_build": f"{'5.6.0' if args.board in ('audio-alpha1', 'audio-tone1') else '5.4.0'}-studio-{args.board}",
        "hardware_verified": False,
        "publisher_signature": None,
        "image_bytes": len(image),
        "image_sha256": hashlib.sha256(image).hexdigest(),
        "kernel_bytes": len(kernel),
        "kernel_sha256": hashlib.sha256(kernel).hexdigest(),
        "note": "Hashes establish byte consistency only. Verify build provenance separately.",
    }
    (args.output / "receipt.json").write_text(json.dumps(metadata, indent=2) + "\n")
    print(json.dumps(metadata, indent=2))


if __name__ == "__main__":
    main()
