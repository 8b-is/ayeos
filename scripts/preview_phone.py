#!/usr/bin/env python3
"""Render the production phone Studio with synthetic evidence, never device I/O.

Python stdlib + rustc only. The output is a new PNG, visibly marked HOST FIXTURE.
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
    parser.add_argument("--state", choices=("complete", "rejected", "off-unverified", "fault"), default="rejected")
    args = parser.parse_args()
    if args.output.exists():
        parser.error("refusing to overwrite an existing preview")
    root = Path(__file__).resolve().parents[1]
    source = (root / "baremetal/src/main.rs").read_text()
    start = source.index("const fn glyph_8x16_hd(")
    end = source.index("\n}\n", start) + 2
    harness = "#![allow(dead_code)]\n"
    for name in ("dashboard", "device_tree", "board_screen", "tone", "phone_studio"):
        harness += f'#[path = {json.dumps(str(root / "baremetal/src" / (name + ".rs")))}] mod {name};\n'
    harness += source[start:end]
    harness += r'''
struct Image(Vec<u32>);
impl dashboard::Canvas for Image {
    fn pixel(&mut self, x: usize, y: usize, color: u32) {
        assert!(x < dashboard::WIDTH && y < dashboard::HEIGHT);
        self.0[y * dashboard::WIDTH + x] = color;
    }
}
fn main() {
    let mut t = tone::Report::new();
    let state = std::env::args().nth(2).unwrap();
    t.outcome = 1;
    t.off = tone::OFF_MASK;
    t.restored = true;
    t.frequency = tone::HZ;
    t.start = 1;
    t.opened = 2;
    t.closed = 3_250_002;
    t.end = 3_250_003;
    match state.as_str() {
        "rejected" => { t.outcome = 2; t.error = 1; t.off = 0; t.restored = false; },
        "off-unverified" => { t.outcome = 4; t.error = 6; t.off &= !1; t.cleanup_errors = 1; },
        "fault" => { t.outcome = 3; t.error = 4; },
        "complete" => {},
        _ => panic!("unknown preview state"),
    }
    let usb = phone_studio::UsbSnapshot::default();
    let view = phone_studio::View {
        revision: "0123456789abcdef0123456789abcdef01234567",
        dirty: false, passed: 7, failed: 0, faults: u64::from(state == "fault"),
        i2c_status: 1, amp_reads: 256, i2c_restored: true,
        pmic_status: 1, pmic_reads: 33, pmic_idle: true, report_crc: 0x12345678,
        usb: &usb, tone: &t,
    };
    let mut image = Image(vec![0; dashboard::WIDTH * dashboard::HEIGHT]);
    phone_studio::paint(&mut image, glyph_8x16_hd, &view);
    dashboard::text(&mut image, glyph_8x16_hd, 40, 1608, 1,
        "HOST FIXTURE / NOT A HANDSET CAPTURE", dashboard::GOLD);
    let rgb: Vec<u8> = image.0.iter().flat_map(|p| [
        (p >> 16) as u8, (p >> 8) as u8, *p as u8,
    ]).collect();
    std::fs::write(std::env::args().nth(1).unwrap(), rgb).unwrap();
}
'''
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="phone-preview-", dir=args.output.parent) as tmp:
        tmp = Path(tmp)
        src = tmp / "preview.rs"
        src.write_text(harness)
        subprocess.run(["rustc", "--edition=2021", "-O", str(src), "-o", str(tmp / "render")], check=True)
        subprocess.run([str(tmp / "render"), str(tmp / "rgb"), args.state], check=True)
        rgb = (tmp / "rgb").read_bytes()
    width, height = 720, 1640
    assert len(rgb) == width * height * 3
    rows = b"".join(b"\0" + rgb[y * width * 3:(y + 1) * width * 3] for y in range(height))

    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b"")
    with args.output.open("xb") as output:
        output.write(png)
    print(args.output.resolve())


if __name__ == "__main__":
    main()
