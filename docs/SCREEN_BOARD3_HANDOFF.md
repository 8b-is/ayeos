# Board3: paged, lossless selected-resource capture

Author: OpenAI GPT-6 Astra (`openai/gpt-6-astra`), 2026-10-31.
Family context v2 / seed `8b-is-family-seed-v1`.
Base: `dfcc982b2d20ffe7a12005163be8339a22712906`.

The Board2 photographs verified the diagnostic and identified MT6358 audio,
MT6768 AFE, two AW87xxx amplifier declarations and CONNSYS. Its 16-byte value
prefixes left most of the 160-byte CONNSYS reg and 48-byte interrupt array unseen.
Board3 retains each selected property's complete bytes across numbered pages.

## Format and bounds

- 68 body lines/page, within the existing 85-line screen geometry. Header/footer
  identify page and total. Record IDs and byte offsets remain stable across pages.
- Exact property names and full paths are chunked instead of truncated. Values
  are raw big-endian hex in 24-byte chunks, with original length and IEEE CRC32.
  CRC32 detects transcription errors; it is not authentication.
- Connectivity comes first, followed by audio, amplifiers, ancestor buses and
  direct dependencies. Ancestors preserve cell-count/range context. Allowlisting
  excludes bootargs and arbitrary properties, including credentials.
- 1 MiB FDT, depth 32, 128 selected nodes, 512 phandle providers, 4096 output lines.
  Capacity failures and report capping are visible, not silent success.
- Direct clock/reset/power/GPIO/sound-DAI arrays use provider-specific argument
  counts; pinctrl and supply references are also followed. Unknown argument sizes
  stop decoding that property. Duplicate handles are unresolved. Referenced
  pinctrl subtrees expose allowlisted pin configuration. This is not a recursive
  closure of every dependency and does not resolve interrupts-extended specifiers.

## Operator capture

Use the reviewed local deployment procedure and verify the new image checksum.
Keep volume keys released during boot/calibration. Photograph page 1 first.
Hold a volume key briefly, then release to advance one page; the last wraps to 1.
Capture header, all body rows, and footer on each page. A hold never auto-repeats
or requests a reboot. Paging uses the inherited keypad address and still needs
handset validation. If it does not respond, report the baseline and first page;
do not infer a working keypad from a nonzero register value alone.

The key sampler uses 32 stable calibration samples (256-attempt budget), and a
three-sample press/release debounce. The inherited instruction-count delay is
not a measured time interval. Zero/unstable calibration disables page advance.
The existing skipped-fault handler is not proof of memory readability. FDT and
framebuffer mapping remain inherited boot-ABI assumptions.

The diagnostic runs no USB updater, radio initialization, server connection or
remote-command code. While holding a page it only services the watchdog and
reads keypad state. Future networking/loader design is in PHONE_CONTROL_ROADMAP.md.

Build from `baremetal/` with `cargo build --release --locked --features board-report`.
Host tests: `rustc --edition=2021 --test baremetal/tests/board_pages.rs -o /tmp/opencode/ayeos-board3-tests`
and run that executable. The package script now emits `studio-board3.img`.

## Server validation and artifact

13 tests in the paged-report harness pass; the eight Board2 regression tests
also pass. Coverage includes complete 160-byte registers and 48-byte interrupts,
cross-page reconstruction/CRCs, mixed clock argument counts, missing and duplicate
providers, capacity limits, pinctrl child records, private-property exclusion,
and press/release debounce without auto-repeat. The first host compile exposed
an ambiguous test-vector type; it was corrected before these passing runs.
Locked AArch64 release build and packaging roundtrip/determinism/header/padding/
identity/invalid-input checks pass. Legacy unused/unreachable warnings remain.

Image: `/data/os-verification-20261031/screen-board3/studio-board3.img`
(32768 bytes), SHA-256:
`ff773bd8e335b9e078dd8179493605f215b04e7626f466299d3a4b272050c507`.
Raw kernel: 57392 bytes, SHA-256:
`9c32ca76de70338d7224844be63c51f3238945ce1e8ffbd8a61a10dc53852e38`.
Publication target: `https://g8t.is/downloads/ayeos/studio-board3/`.
No handset validation or Wi-Fi communication has occurred in this build step.
