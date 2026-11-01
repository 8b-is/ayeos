#!/usr/bin/env bash
# Host/reference, cross-language wire and EL1/EL2 architectural checks.
# Run from repository root. QEMU validates architecture, not the phone board.
set -euo pipefail
output=$(mktemp -d "${TMPDIR:-/tmp/opencode}/ayeos-alpha.XXXXXXXX")
export AYEOS_ALPHA_TEST_DUMP="$output/alpha-wire.bin"
python3 - <<'PY'
from pathlib import Path
import hashlib
expected = {
    "marine_int.rs": "7523a876901e834218810986c26835f67c4e4776c82793b7c10f650a137b8801",
    "rational.rs": "c94790fc5eedd75902428031db050b81a48ee396fee418e545d6f856b28d8a2f",
    "internal_math.rs": "6be8f8f249e988f2480dd898af705cf54228453043ed3bbaa6410d00b0b805a3",
}
for name, digest in expected.items():
    path = Path("baremetal/vendor/marine-core/reference") / name
    assert hashlib.sha256(path.read_bytes()).hexdigest() == digest, name
print("Pinned Marine reference fingerprints verified")
PY
rustc --edition=2021 --crate-name marine_core --crate-type rlib \
    baremetal/vendor/marine-core/src/lib.rs -o "$output/libmarine_core.rlib"
rustc --edition=2021 --test baremetal/vendor/marine-core/src/lib.rs -o "$output/marine-tests"
"$output/marine-tests"
rustc --edition=2021 --cfg 'feature="alpha-report"' --cfg 'feature="audio-report"' \
    --cfg 'feature="board6-report"' --extern "marine_core=$output/libmarine_core.rlib" \
    --test baremetal/tests/alpha.rs -o "$output/alpha-tests"
"$output/alpha-tests"
python3 -m unittest discover -s scripts -p 'test_capture_alpha.py' -v
for mode in fatal_brk fatal_load; do
    rustc --edition=2021 --target aarch64-unknown-none --cfg "$mode" \
        -C opt-level=2 -C panic=abort -C link-arg=-Tbaremetal/linker.ld \
        baremetal/tests/exception_qemu.rs -o "$output/exception-$mode.elf"
    for virtualization in off on; do
        timeout 15 qemu-system-aarch64 -machine "virt,virtualization=$virtualization" \
            -cpu cortex-a53 -m 128M -nographic -monitor none -serial none -no-reboot \
            -semihosting-config enable=on,target=native -kernel "$output/exception-$mode.elf"
    done
done
printf 'Alpha validation artifacts: %s\n' "$output"
