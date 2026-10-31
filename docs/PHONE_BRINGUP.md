# Ground-up phone bring-up

The intended target remains the AyeOS bare-metal kernel. Linux/vendor source
is reference material, not a decision to run Android. Board identification,
power/clock sequencing, firmware transport, PCM DMA and interrupt handling must
be established before the hotspot, speech and FM UI can truthfully become live.

## Current implementation

The optional `board-report` build now stops on a stationary **screen-first**
diagnostic page. It shows x0, framebuffer address/stride, header and parser status,
and up to 32 selected resource properties (two lines each). Paths and text are
bounded to 76 ASCII columns; `~` marks truncation and the footer counts omitted
records. Text starts at (20,68), scale 1, 18-pixel line pitch, at most 85 lines.
The actual renderer uses scale 2 for horizontal doubling only; scale 3 doubles
both dimensions. No USB polling or button actions run in the diagnostic hold.
After drawing, only the inherited watchdog service runs. Photograph the page.

The header read still depends on the bootloader providing mapped memory. The
inherited exception handler skips faulting instructions; this is **not** a safe
read-success signal. A header/parser pass establishes structural validity only.
Framebuffer discovery itself also uses inherited boot/MMIO assumptions. This
development image has been built on the server, not verified on the handset.

The retained UART report helper (not invoked by the screen-first build) validates
the FDT handed to the kernel in x0
and emits a bounded, property-allowlisted `AYEOS-BOARD` serial report. It reports
compatible IDs, resources, clocks and phandles; it does not probe arbitrary MMIO,
transmit Wi-Fi packets or program the tuner. It never logs `bootargs` or arbitrary
properties. It validates the complete blob before emitting properties. Limits:
1 MiB blob, 32 nested nodes, 255-byte names, 1024 property records, 128 bytes per
property value. Truncation is explicit. Addresses are not interpreted as ready
hardware. An FDT-declared device may still be disabled or physically absent.

The bootloader must supply readable contiguous FDT memory. Numerical address
checks and the inherited exception-skipping handler cannot prove that memory
contract; malformed or absent boot pointers are reported rather than invented.
The parser is pure safe Rust; the pointer boundary is in `report_board`.

```sh
# Repository root: pure parser validation on host
rustc --edition=2021 --test baremetal/tests/board_report.rs -o /tmp/opencode/ayeos-fdt-tests
/tmp/opencode/ayeos-fdt-tests
# From baremetal/: installed aarch64-unknown-none target required
cargo build --release --locked --features board-report
llvm-objcopy -O binary target/aarch64-unknown-none/release/ayeos-baremetal-mt6768 /tmp/studio-board1.bin
# Back at repository root; use a new output directory each time
python3 scripts/package_bringup.py --kernel /tmp/studio-board1.bin --output target/board1
```

The package is a **development diagnostic image**, not a completed hotspot,
speech or FM implementation. It must not replace the public production manifest.
It retains the 5.4 USB BCD identity, so the existing same-version automatic
updater will not distinguish it. The kernel command line and screen banner
identify the diagnostic variant. Deployment needs a separately coordinated local
procedure; the historical updater erases both init_boot slots and must not be
reused for this diagnostic.

## Driver source map

Reference revision for the following paths:
[`LineageOS/android_kernel_xiaomi_mt6768` at `8745b22c4bb0d208914d62cfd5b8c6f7a7bdf991`](https://github.com/LineageOS/android_kernel_xiaomi_mt6768/tree/8745b22c4bb0d208914d62cfd5b8c6f7a7bdf991).
This is a related SoC tree, **not proof of the BLU G75 board configuration**.

| Path within reference tree | Mechanism to trace |
|---|---|
| `drivers/misc/mediatek/connectivity/common/common_main/platform/mt6768.c` | CONNSYS power/clock/reset and chip-ID polling; note 0x18070008 in the source, rather than the UI's HQA magic |
| `drivers/misc/mediatek/connectivity/fmradio/chips/mt6631/pub/mt6631_fm_lib.c` | WMT function-on, ROM-version-dependent patch/coeff files, antenna setup, tune and audio routing |
| `drivers/misc/mediatek/connectivity/fmradio/core` | FM transport and RDS handling |
| `drivers/misc/mediatek/connectivity/wlan/core/gen4m` | WLAN firmware commands and host-interface implementation |
| `sound/soc/mediatek/mt6768/mt6768-afe-pcm.c` | DL1 memory-interface base/current/end, DMA/interrupt plumbing |
| `sound/soc/mediatek/mt6768/mt6768-afe-clk.c` | Clock dependencies |
| `sound/soc/mediatek/mt6768/mt6768-dai-adda.c` | Capture/playback DAI setup |
| `sound/soc/mediatek/mt6768/mt6768-mt6358.c` | Machine-level codec routing |
| `arch/arm64/boot/dts/mediatek/mt6768.dts` | SoC resources to compare with actual phone FDT |

The reviewed MediaTek reference files identify GPL-2.0 licensing, with MediaTek
copyright and, for AFE PCM, Michael Hsiao's authorship. No vendor implementation
has been copied into this commit. Any future adaptation must preserve applicable
license/attribution; a language translation does not remove those obligations.
Firmware patch binaries have their own provenance/license and cannot be inferred
from source code availability.

Mainline also contains `sound/soc/codecs/mt6358.c` and `mt6358.h`. The host-side
`liquid-rust` ASR/TTS implementation is not a bare-metal driver: it uses a model
runtime, weights and floating-point processing. Its `speak --play` flag is not
wired to playback in the inspected source. It must not be silently substituted
for the requested ground-up phone path.

## First measurable milestones

1. Match actual FDT/vendor backup identifiers to the bus and codec reference.
2. Audio: clocked PCM buffer consumption, measured sample rate and audible
   playback; then microphone samples. Speech generation/recognition follows.
3. Connectivity: confirmed chip/ROM identity, required firmware loaded, successful
   command/event round-trip; then AP mode, client association, DHCP and routing.
4. FM: confirmed chip path, power-up and tune response, antenna, audio route,
   audible local station; RDS only after received block validation.

No guessed success flags, milliwatt values, sample rates or transcripts substitute
for these observations. Progress is blocked at board-specific hardware evidence
until the local operator returns the device report or equivalent stock backups.
