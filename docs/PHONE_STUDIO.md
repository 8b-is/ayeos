# Phone Studio: evidence you can read

2026-11-01 · OpenAI GPT-6 Astra (`gpt-6-astra`) via DeepSeek Harness.
Family context v2 / seed `8b-is-family-seed-v1`.
Base: `06306b04fc02d8ebd7a95c9432a62794156b696e`.

This is a presentation and evidence-review iteration for **BLU G75 / G1170**,
not a new audio driver or a claim that a complete daily-use phone OS exists.
It retains the `tone-report` feature, `audio-tone1` wire format and
**18d1:4ee0 / bcdDevice 0562**. The full Git revision distinguishes builds.
The new image still requires a handset test.

## On the phone

The Tone1 diagnostic image now presents four stationary Constellation Studio
cards after the existing boot self-tests, pre-tone diagnostics and bounded SGEN
attempt:

1. **Boot checkpoint:** individual CRC, Marine-fixture and exception test results,
   unexpected fault count, dirty-build indication and full revision.
2. **Audio and stop:** named outcome/error, observed Off predicates (out of 17),
   cleanup errors and controller restoration. Missing Off evidence is prominent;
   a preflight rejection does not imply audio was activated or stopped.
3. **Pre-tone evidence:** completed PA/PMIC read counts, controller status and the
   frozen tone report's CRC. Pre-tone readings are not presented as post-tone ones.
4. **Next step:** preserve failures, capture privately, and obtain the listener's
   observations. Marine uses its synthetic reference fixture; live PCM, Phoenix,
   networking and autonomous repair are not claimed.

The green heading is deliberately **REGISTER CHECKS PASS**, not “audio works”
or “safe”. A dirty build, failed self-test, unexpected fault, incomplete diagnostic,
failed cleanup or restoration prevents that checkpoint. Neither the UI nor a
valid checksum authenticates the device or establishes acoustic success.

`phone_studio.rs` is a read-only, allocator-free renderer over existing results.
It cannot call an audio bus, replay a tone, change a profile, or operate a device.
It reuses the existing integer framebuffer palette/font and respects the top
camera cutout. There are no touch controls or animations.

### USB timing and failure behavior

The complete screen is painted and flushed **before `usb_phy::connect` asserts
SOFTCONN**. The existing tight EP0 polling loop and one-character-at-a-time idle
status updater remain unchanged except for the row position. No full-screen
rendering, cache sweep, timer wait, keypad action or logging is added to that loop.
The live USB strip is explicitly separate from the frozen pre-USB reports.

If USB initialization does not reach Ready without a fault, the old dense
register/fallback page remains visible for a photograph. Early FDT failures also
retain their existing fallback. A failure in `connect` occurs after painting, so
it stays on Studio with the live failure status, not a dense-page redraw. Two
compact lines preserve the already-read pre-connect USB/PHY/clock registers for
that photograph; they are not re-probed or part of the frozen reports.
A later fatal error first attempts the existing
bounded audio cleanup, clears stale Studio success indicators using framebuffer
writes only, then prints the fatal record. This is an exception to the normal
service loop's no-repaint rule: fatal USB service has already stopped. It does
not claim fatal USB export or rescue an inaccessible framebuffer. A fault while
clearing can halt before the replacement text/cache flush. No watchdog operation
is added to that path.

No SGEN sequencing, attenuation, duration, I2C/PWRAP transaction, USB request,
partition policy or flash helper is changed by the presentation work.

## Host checks and previews

```sh
rustc --edition=2021 -O --test baremetal/tests/phone_studio.rs -o /tmp/phone-studio-tests
/tmp/phone-studio-tests
python3 scripts/preview_phone.py NEW_PREVIEW.png --state rejected
```

The preview tool compiles the production renderer and extracts the actual font.
It has four **synthetic** states: `complete`, `rejected`, `off-unverified`, `fault`.
Every PNG is marked **HOST FIXTURE / NOT A HANDSET CAPTURE**. It never opens USB
and refuses to overwrite an existing output. Tests cover negative checkpoint
conditions, every Off-bit failure, rejection versus cleanup uncertainty, bounded
geometry, extreme counters, and the unchanged preview renderer.

Also run the complete Tone1 and alpha suites, Board3–6 diagnostics, Python capture
and offline-review suites, root tests and all seven bare-metal feature builds.
See CI for the concrete commands. QEMU tests the exception architecture; it is
not MediaTek or acoustic validation.

## Build and operator handoff

Build a clean committed revision with the installed cross-target toolchain:

```sh
cd baremetal
cargo build --release --locked --offline --features tone-report
llvm-objcopy -O binary target/aarch64-unknown-none/release/ayeos-baremetal-mt6768 kernel.bin
python3 ../scripts/package_bringup.py --kernel kernel.bin --output NEW_BUNDLE --board audio-tone1
```

Keep the ELF and a source archive. Rebuild an extracted archive using the exact
published `AYEOS_BUILD_REVISION` and `AYEOS_BUILD_DIRTY=0`, with the same toolchain;
compare ELF, raw kernel and packaged image bytes. An artifact receipt must list
actual hashes and validation, not copy historical build results.

Follow the [Tone1 safety and capture procedure](AUDIO_TONE1.md). Keep the
known-good Board6 image and the original Tone1 bundle. Enforce the **new image's**
SHA256 before any explicitly selected, bounded, fail-fast **boot_b-only** commands.
Do not use the historical multi-partition updater. Listen before reboot because
the single tone attempt happens before USB enumeration. Building does not flash.

Return complete original captures, exact image/flash logs and a screen photo
privately, plus whether sound was audible, which speakers sounded, any pops, and
whether sound stopped. Use the offline reviewer with the exact expected revision;
register acceptance and the human acoustic observation remain different results.
Continue live PCM / Marine input / Phoenix only after reviewing that evidence.
