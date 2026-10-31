#!/usr/bin/env python3
"""Render the actual phone composition on the host, without running device code.

Requires rustc and Python stdlib. Writes a PNG and runs pixel-boundary tests.
The font is extracted verbatim from main.rs; dashboard.rs is compiled directly.
"""
import argparse
import json
from pathlib import Path
import struct
import subprocess
import tempfile
import zlib


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    source = (root / "baremetal/src/main.rs").read_text()
    start = source.index("const fn glyph_8x16_hd(")
    end = source.index("\n}\n", start) + 2
    font = source[start:end]
    module = root / "baremetal/src/dashboard.rs"
    # JSON quoting is also valid for this absolute Rust string literal.
    harness = '#[path = ' + json.dumps(str(module)) + '] mod dashboard;\n' + font
    harness += r'''
use dashboard::Canvas;
struct Image(Vec<u32>);
impl Canvas for Image {
    fn pixel(&mut self, x: usize, y: usize, color: u32) {
        assert!(x < dashboard::WIDTH && y < dashboard::HEIGHT);
        self.0[y * dashboard::WIDTH + x] = color;
    }
}
fn main() {
    let mut image = Image(vec![0; dashboard::WIDTH * dashboard::HEIGHT]);
    dashboard::paint(&mut image, glyph_8x16_hd, false);
    let rgb: Vec<u8> = image.0.iter().flat_map(|p| [
        (p >> 16) as u8, (p >> 8) as u8, *p as u8
    ]).collect();
    std::fs::write(std::env::args().nth(1).unwrap(), rgb).unwrap();
}
'''
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="studio-", dir=args.output.parent) as tmp:
        tmp = Path(tmp)
        src = tmp / "preview.rs"
        src.write_text(harness)
        subprocess.run(["rustc", "--edition=2021", "--test", str(src), "-o", str(tmp / "tests")], check=True)
        subprocess.run([str(tmp / "tests")], check=True)
        subprocess.run(["rustc", "--edition=2021", str(src), "-o", str(tmp / "render")], check=True)
        subprocess.run([str(tmp / "render"), str(tmp / "rgb")], check=True)
        rgb = (tmp / "rgb").read_bytes()
    width, height = 720, 1640
    assert len(rgb) == width * height * 3
    rows = b"".join(b"\0" + rgb[y * width * 3:(y + 1) * width * 3] for y in range(height))

    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b"")
    args.output.write_bytes(png)
    print(args.output.resolve())


if __name__ == "__main__":
    main()
