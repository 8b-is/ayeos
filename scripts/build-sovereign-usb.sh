#!/bin/sh
# ==============================================================================
# Sovereign 8b-is x MacCracken x AyeOS Universal Bootable USB Builder
# Zero-Float Discipline | Multi-Boot (CachyOS 7.3.1 + Bare-Metal AGNOS + AyeOS)
# ==============================================================================
set -e

USB_DEV="${1:-/dev/sda}"
echo "[sovereign-usb] Target device: $USB_DEV"
lsblk -o NAME,LABEL,FSTYPE,SIZE,MOUNTPOINT "$USB_DEV"
