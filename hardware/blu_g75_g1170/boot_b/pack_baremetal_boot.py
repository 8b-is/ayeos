#!/usr/bin/env python3
import hashlib
import json
import os
import struct

BOOT_DIR = os.path.dirname(os.path.abspath(__file__))
with open(os.path.join(BOOT_DIR, "ayeos_baremetal.bin.gz"), "rb") as f:
    kernel_gz = f.read()

PAGE_SIZE = 4096
hdr = bytearray(PAGE_SIZE)
# Note: MediaTek MT6768 LittleKernel (lk) bootloader requires the 8-byte "ANDROID!"
# envelope magic at offset 0 of boot_a/boot_b before loading our 100% #![no_std]
# bare-metal Rust kernel to 0x4008_0000. Zero Android or Linux code is present.
hdr[0:8] = b"ANDROID!"
struct.pack_into("<I", hdr, 8, len(kernel_gz))
struct.pack_into("<I", hdr, 12, 0)
os_version = (15 << 25) | (26 << 4) | 10
struct.pack_into("<I", hdr, 16, os_version)
struct.pack_into("<I", hdr, 20, 1580)
struct.pack_into("<I", hdr, 40, 4)
cmdline = b"ayeos.baremetal=v5.5.0 ayeos.board=studio-board5 ayeos.android=0 ayeos.audio=afe,mt6358,aw87xxx ayeos.mt6631=hqa,fmrds ayeos.ota=g8t.is,aye.is\x00"
hdr[44:44 + len(cmdline)] = cmdline

pad_len = (PAGE_SIZE - (len(kernel_gz) % PAGE_SIZE)) % PAGE_SIZE
img = bytes(hdr) + kernel_gz + (b"\x00" * pad_len)

out_path = os.path.join(BOOT_DIR, "ayeos_boot_b_gz.img")
with open(out_path, "wb") as f:
    f.write(img)

sha256_hex = hashlib.sha256(img).hexdigest()
blake2s_hex = hashlib.blake2s(img).hexdigest()
sig_material = (
    b"G8T_IS_AYE_ONLY_SOVEREIGN_GATE_V1:"
    b"aye-sovereign-founder:blu-g75-g1170:5.5.0:"
    + sha256_hex.encode("ascii")
)
sig_hex = hashlib.sha256(sig_material).hexdigest()

manifest = {
    "device_slug": "blu-g75-g1170",
    "os_name": "ayeOS Bare-Metal Rust Kernel v5.5.0 (studio-board5: AFE + MT6358 + AW87xxx SmartPA + MT6631)",
    "version": "5.5.0",
    "resonance_hz": 432,
    "image_size_bytes": len(img),
    "gzip_kernel_bytes": len(kernel_gz),
    "image_sha256_hex": sha256_hex,
    "image_blake2s_hex": blake2s_hex,
    "attested_by_aye": "aye-sovereign-founder",
    "domain_tag": "G8T_IS_AYE_ONLY_SOVEREIGN_GATE_V1",
    "signature_hex": sig_hex,
    "primary_ota_urls": [
        "https://g8t.is/g8t/ota/blu-g75/boot.img",
        "https://aye.is/g8t/ota/blu-g75/boot.img",
        "https://8b.is/g8t/ota/blu-g75/boot.img",
        "http://192.168.151.14:8788/g8t/ota/blu-g75/boot.img",
    ],
}
manifest_path = os.path.join(BOOT_DIR, "manifest.json")
with open(manifest_path, "w") as f:
    json.dump(manifest, f, indent=2)
    f.write("\n")

print(
    f"Packed bare-metal {out_path}: {len(img)}B (GZIP: {len(kernel_gz)}B) "
    f"sha256={sha256_hex[:16]}... sig={sig_hex[:16]}..."
)
