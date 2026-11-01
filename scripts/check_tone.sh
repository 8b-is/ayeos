#!/usr/bin/env bash
set -euo pipefail
output=$(mktemp -d "${TMPDIR:-/tmp/opencode}/ayeos-tone.XXXXXXXX")
export AYEOS_TONE_TEST_DUMP="$output/tone-wire.bin"
rustc --edition=2021 -O --test --cfg 'feature="audio-report"' \
    --cfg 'feature="board6-report"' --cfg 'feature="alpha-report"' \
    --cfg 'feature="tone-report"' baremetal/tests/tone.rs -o "$output/tone-tests"
"$output/tone-tests"
python3 -m unittest discover -s scripts -p 'test_capture_tone.py' -v
rustc --edition=2021 --target aarch64-unknown-none --cfg fatal_cleanup \
    -C opt-level=2 -C panic=abort -C link-arg=-Tbaremetal/linker.ld \
    baremetal/tests/exception_qemu.rs -o "$output/exception-cleanup.elf"
for virtualization in off on; do
    timeout 15 qemu-system-aarch64 -machine "virt,virtualization=$virtualization" \
        -cpu cortex-a53 -m 128M -nographic -monitor none -serial none -no-reboot \
        -semihosting-config enable=on,target=native -kernel "$output/exception-cleanup.elf"
done
printf 'Tone validation artifacts: %s\n' "$output"
