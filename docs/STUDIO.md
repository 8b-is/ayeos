# Constellation Studio · development preview

An evidence-first 720 × 1640 framebuffer interface for the BLU G75: near-black,
mint, gold, generous spacing, rounded cards, and an explicitly generative field.
Motion is off at startup. A volume-key transition toggles it; holding the key
retains the existing experimental reboot path. Timing is counted in render loops,
not seconds. No touch controls are implied.

## What runs

`baremetal/src/main.rs::rust_baremetal_main` uses the existing framebuffer,
watchdog, USB and keypad routines and the new `dashboard.rs` composition.
The screen reports render-loop count, not invented radio/microphone readings.
The previously shipped dashboard is retained behind `#[cfg(any())]` as
source-only audit evidence. Its camera, ASR, RDS and hotspot success displays
are not compiled into this preview. Experimental audio register writes from
that dashboard are not part of the new render loop.

The renderer uses integer coordinates and RGB channels, no heap allocation,
and bounded loops. The quartic ribbons are artwork, not sine measurements or
sensor input. Text uses the existing bitmap font. Hardware framebuffer discovery
and cache/coherency assumptions remain inherited and need device validation.

## Reproduce

From the repository root, with Python 3 and `rustc` installed:

```sh
python3 scripts/preview_studio.py target/studio-preview.png
python3 scripts/audit_os_image.py --online
```

The preview compiles the actual `dashboard.rs` and extracts the actual bitmap
font from `main.rs`. It runs a bounds test across all 256 artwork phases and
the maximum u64 counter, then writes a PNG. It never executes hardware code.

From `baremetal/`, with the Rust `aarch64-unknown-none` target installed:

```sh
cargo build --release --locked
```

This produces an ELF development artifact. It does not replace the shipped
boot image, regenerate its manifest, publish an update or flash the phone.
The UI says `STUDIO PREVIEW`; package/USB version remains 5.4.0 pending an
actual release. Release work must assign a new version consistently, package
and hash the actual image, and validate boot, display, input and USB on-device.

## v5.4 evidence correction

At baseline `51fee9683428afcbcdf1ca034a2214e5368f2003`, the shipped kernel
rebuilds byte-identically. The 20,480-byte boot image is served by g8t.is,
aye.is and 8b.is. This establishes artifact consistency, not peripheral support.

- `step_mt6631_connsys_hqa_and_fm_rds` creates a response header and seal locally;
  it does not exchange HQA commands or decode RDS.
- Camera pixels come from a phase function; speech labels come from arrays.
- Microphone dB numbers and salience are synthetic; the sample-rate labels do
  not establish a clocked PCM output stream.
- `compute_baremetal_image_seal` hashes a fixed label, not kernel memory.
- `wifi_ota_hub.py` verifies an unkeyed public hash. This checks consistency,
  not publisher identity. The audit script demonstrates that changing the
  image and recomputing the public hashes is accepted without a signing key.
- Wi-Fi command-line patching is not a Wi-Fi driver. The bare-metal source has
  no implemented association, IP/HTTPS updater, or command-line credential reader.

The original image, flasher and manifest are preserved for review. The existing
public flasher text still overstates capabilities and requires a coordinated
release correction. Signed updates need a real signature format and pinned
verification key, not a renamed checksum. Neither this preview nor passing
host tests establishes a working FM radio, camera, speech system or hotspot.

Validation: 26 existing host tests passed; AArch64 release build passed;
shared-renderer bounds test passed. New UI has not been tested on the phone.
Attribution: OpenAI GPT-6 Astra (`openai/gpt-6-astra`), 2026-10-31.
