#!/usr/bin/env bash
set -euo pipefail

# Zero-C Boot & Kernel Guard for 8b-is/ayeos
# Enforces: zero .c/.h sources and zero C runtime (cc/libc) in AyeOS boot/kernel/capsule crates.
# Note: vendor firmware and the Linux host test harness (`ayeos-capsule-worker`) are outside boot images.

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BOOT_DIRS=(
  "uefi-bootloader"
  "baremetal-x86_64"
  "baremetal-pi4"
  "baremetal"
  "crates/ayeos-manifest"
  "capsules"
)

echo "[Zero-C Guard] Checking boot, kernel, manifest, and no_std capsule directories in $ROOT..."

for d in "${BOOT_DIRS[@]}"; do
  [ -d "$ROOT/$d" ] || continue
  c_files=$(find "$ROOT/$d" -not -path "*/target/*" -type f \( -name "*.c" -o -name "*.h" \))
  if [ -n "$c_files" ]; then
    echo "FAIL [Zero-C]: Found C source/header files in $d:" >&2
    echo "$c_files" >&2
    exit 1
  fi
done

for toml in \
  "$ROOT/uefi-bootloader/Cargo.toml" \
  "$ROOT/baremetal-x86_64/Cargo.toml" \
  "$ROOT/baremetal-pi4/Cargo.toml" \
  "$ROOT/baremetal/Cargo.toml" \
  "$ROOT/crates/ayeos-manifest/Cargo.toml" \
  "$ROOT/capsules/fn_harmonic_fold/Cargo.toml"; do
  [ -f "$toml" ] || continue
  if grep -E '^[[:space:]]*(cc|libc)[[:space:]]*=' "$toml" >/dev/null 2>&1; then
    echo "FAIL [Zero-C]: Forbidden C/libc crate dependency in $toml" >&2
    exit 1
  fi
done

# Verify built zero-dep capsules have zero DT_NEEDED entries (no libc.so.6 / C runtime)
CARGO_TARGET="${CARGO_TARGET_DIR:-$HOME/.cargo/target}"
for so in \
  "$CARGO_TARGET/release/libayeos_fn_harmonic_fold.so" \
  "$ROOT/capsules/target/release/libayeos_fn_harmonic_fold.so"; do
  if [ -f "$so" ]; then
    needed=$(readelf -d "$so" | grep '(NEEDED)' || true)
    if [ -n "$needed" ]; then
      echo "FAIL [Zero-C]: Capsule $so links external dynamic libraries:" >&2
      echo "$needed" >&2
      exit 1
    fi
    echo "  OK: $so has zero DT_NEEDED entries"
  fi
done

echo "[Zero-C Guard] PASS: zero C sources, zero cc/libc boot deps, zero DT_NEEDED in capsules."
