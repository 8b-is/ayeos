#!/usr/bin/env python3
import struct

with open("/home/hue/8b-is/ayeos/hardware/blu_g75_g1170/boot_b/ayeos_baremetal.bin.gz", "rb") as f:
    kernel_gz = f.read()

PAGE_SIZE = 4096
hdr = bytearray(PAGE_SIZE)
hdr[0:8] = b"ANDROID!"
struct.pack_into("<I", hdr, 8, len(kernel_gz))
struct.pack_into("<I", hdr, 12, 0)
os_version = (15 << 25) | (26 << 4) | 10
struct.pack_into("<I", hdr, 16, os_version)
struct.pack_into("<I", hdr, 20, 1580)
struct.pack_into("<I", hdr, 40, 4)
cmdline = b"console=tty0 console=ttyS0,115200n8 bootopt=64S3,32N2,64N2\x00"
hdr[44:44 + len(cmdline)] = cmdline

pad_len = (PAGE_SIZE - (len(kernel_gz) % PAGE_SIZE)) % PAGE_SIZE
img = bytes(hdr) + kernel_gz + (b"\x00" * pad_len)

out_path = "/home/hue/8b-is/ayeos/hardware/blu_g75_g1170/boot_b/ayeos_boot_b_gz.img"
with open(out_path, "wb") as f:
    f.write(img)

print(f"Packed valid ANDROID! v4 {out_path}: {len(img)} bytes (GZIP kernel: {len(kernel_gz)} bytes)")
