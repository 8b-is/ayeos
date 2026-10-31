# Board4: full-speed USB EP0 readback

OpenAI GPT-6 Astra (`openai/gpt-6-astra`), 2026-10-31.
Family context v2 / seed `8b-is-family-seed-v1`.
Base: `39a35aa8c5eacc67b63bcff9a44afaa165d5f0cf` (Board3).

Board4 is a bare-metal USB bring-up experiment. It serves the live boot FDT on
explicit laptop requests, using endpoint zero only. It is **not CDC-ACM** and
does not create a tty. Host/mock tests and an AArch64 build cannot establish
enumeration on the handset. No radio or remote service is contacted.

## Operator handoff

Download directory: `https://g8t.is/downloads/ayeos/studio-board4/`.
It contains `studio-board4.img`, `SHA256SUMS`, `receipt.json`, this handoff,
`capture_board4.py`, and `studio-board4-source.tar.gz`.
Verify hashes before using the reviewed local deployment procedure. Preserve
the known boot/recovery path; the historical updater that erases both init_boot
slots is not the deployment mechanism for this diagnostic.

After boot, connect the laptop USB-C cable and run `lsusb`. Expected experimental
identity is `18d1:4ee0`, bcdDevice `0544`, vendor class/subclass/protocol
`ff/a8/04`. This VID/PID is inherited and is not an assigned AyeOS identity.
The script verifies the complete device/configuration descriptors and protocol
identity, because the VID/PID alone also matches older development images.

Example **on the Linux laptop**, replacing BBB/DDD with its actual bus/device:

```sh
sudo python3 capture_board4.py --device /dev/bus/usb/BBB/DDD \
  --output "$HOME/board4-capture-01"
sudo chown "$(id -u):$(id -g)" "$HOME/board4-capture-01" \
  "$HOME/board4-capture-01/board4-fdt.dtb" "$HOME/board4-capture-01/capture.json"
```

No Python packages or CDC driver are needed; Linux usbfs control transfers use
stdlib ctypes. The host must already have selected configuration 1. The script
does not detach kernel drivers, reset devices or send configuration/OTA/reboot
commands. Each request has a maximum 1000 ms timeout, at most three attempts
(only EINTR/timeouts are retried), and the complete session has a 300 second
default deadline (integer override 1..600). Failed validation stops capture.

Output is a new 0700 directory with exclusive 0600 files; existing files are
never overwritten. The complete dump is written only after all checks pass.
Raw FDT can include bootargs and identifiers: transfer it privately to the
collaborator, not to the public download directory. It can then be decoded with
`dtc -I dtb -O dts board4-fdt.dtb` where dtc is installed.

If no device appears, return **one photograph of the whole Board4 screen** and
the laptop's USB enumeration errors. Include FDT match flags, INIT, register
snapshots, USB state and the omitted-record count. Board3's 14 pages are already
received; they do not contain the USB-specific nodes needed for this test.

## Hardware boundary

- Runtime x0, valid bounded v17 FDT, maximum 1 MiB/depth 32. No hardcoded boot
  pointer. Existing framebuffer/boot mappings and the instruction-skipping
  exception handler remain assumptions; checks do not prove memory readability.
- Require MT6768 root compatibility, one enabled `mediatek,mt6768-usb20`
  controller, supported identity address translation, controller `0x11200000`,
  shared PHY region `0x11cc0000`, and a linked USB2 port `0x11cc0800` under
  `mediatek,generic-tphy-v1` with one PHY argument of type 3.
- Require declared infracfg `0x10001000` and topckgen `0x10000000`. Enable only
  ICUSB gate bit8 and USB_TOP gate bit23/update bit22; preserve clock parents.
  The inherited PMIC rails and PLLs are **not enabled or verified** here.
- Recover the USB2 PHY, force device role, disable high-speed negotiation and
  soft-connect at full speed. `Attached` means the SOFTCONN register readback
  passed, not host enumeration. Address/configuration show host progress.
- MUSB indexed EP0: INDEX +0x0e (8 bits), CSR0 +0x12 (16 bits), COUNT0 +0x18
  (8 bits), FIFO0 +0x20 (8 bits). SET_ADDRESS/SET_CONFIGURATION are deferred
  until DATAEND/status completion; reset and SETUPEND cancel pending work.
- All USB MMIO operations are suppressed after a newly counted skipped fault.
  A rejected FDT profile causes zero USB/PHY/clock accesses. Watchdog and
  framebuffer setup are separate inherited boot operations.
- Settling before attachment uses four million NOPs per settling call. This is
  an instruction-count delay, **not calibrated microseconds**. The static screen
  and snapshots are drawn before soft connection. The service loop has no
  deliberate delay or graphics pacer; one line changes only on USB state/fault
  changes. No measured polling latency or enumeration success is claimed.
- The one-page fallback selects USB/PHY/vusb properties, at most 24 records.
  Paths/fields may be shortened with `~`; omitted records are counted. Register
  snapshots are pre-connection, not continuously refreshed telemetry. Full FDT
  bytes are available only if USB capture succeeds.

## Wire format v1

All multibyte protocol fields below are little-endian; FDT bytes stay unchanged
(FDT internals are big-endian). EP0 max packet 64 bytes; one packet per reply.
All vendor requests use bmRequestType `0xc0`, wLength 64, configuration 1.

| Request | Inputs | 64-byte response |
|---|---|---|
| `0xb0` | wValue=0, wIndex=0 | `A4IF`, u32 version=1, u32 total bytes, u32 whole CRC32, u32 chunk=48, 12-byte `board4-ep0\0\0`, 32 zero bytes |
| `0xb1` | chunk number = wValue + (wIndex << 16) | `A4CH`, u32 byte offset, u16 data length, u16 version=1, u32 data CRC32, up to 48 data bytes, zero padding |

IEEE CRC32 detects accidental corruption, not sender authenticity. The laptop
checks every expected offset, last-chunk length, version, CRC and zero padding;
then checks total length, whole CRC, FDT magic/size, and an unchanged final info
response. It stores a SHA-256 receipt. Requests are idempotent. Out-of-range
chunks, OUT commands, unknown requests and the old `0xa8` command stall.

## Build, test and source

Toolchain used: Rust 1.99.1; target `aarch64-unknown-none`, no Cargo dependencies.
Use the rustup cargo/rustc on PATH (the system Rust lacks this target here).

```sh
rustup target add aarch64-unknown-none
cd baremetal
cargo build --release --locked --features usb-report
llvm-objcopy -O binary target/aarch64-unknown-none/release/ayeos-baremetal-mt6768 \
  /tmp/studio-board4.bin
cd ..
python3 scripts/package_bringup.py --board board4 --kernel /tmp/studio-board4.bin \
  --output /tmp/studio-board4-new
rustc --edition=2021 --test baremetal/tests/usb_board4.rs -o /tmp/board4-tests
/tmp/board4-tests
python3 -m unittest discover -s scripts -p test_capture_board4.py -v
```

The archive includes the complete baremetal crate, linker/configuration, host
tests, capture/packaging scripts, this document and license notices. Its revision
and validation results belong in the adjacent artifact receipt. Legacy unused
and unreachable-code warnings are expected in this feature-selected build.

Server validation: 16 Rust tests (protocol, mock MMIO, FDT and shared parser),
eight Python capture/packaging tests and 13 Board3 regression tests passed.
The locked AArch64 release built successfully. The first build invocation used
the system compiler without the target; selecting the installed rustup toolchain
resolved that. A protocol test caught an incorrect info-identity padding length
during development; it was fixed before these results.

Packaged image: 28672 bytes, SHA-256
`fdedde94625c185bba3f53acdacb4e96afe9fcd8cdd4fab6d5e2f0ea515d63f5`.
Raw kernel: 53296 bytes, SHA-256
`6a6b27304326853bb6368b353ea0e451faa1d36eb7b671bd2d99f6aea15b3a0b`.
Handset enumeration and real USB capture remain unverified.

## Attribution and licensing

`baremetal/src/usb_phy.rs` is a modified Rust adaptation of GPL-2.0-only MediaTek
sequences, not an independent clean-room driver. Copyright MediaTek Inc.,
2015–2019; `phy-mtk-tphy.c` credits Chunfeng Yun. The adaptation was authored by
OpenAI GPT-6 Astra in 2026. Preserve those notices. The adapted code is provided
under GPL-2.0-only; see `LICENSES/GPL-2.0-only.txt`. Corresponding buildable source
is supplied alongside the experimental binary. This notice does not assert a
new blanket license for unrelated repositories or third-party files.

Pinned reference revision: `8745b22c4bb0d208914d62cfd5b8c6f7a7bdf991` at
<https://github.com/LineageOS/android_kernel_xiaomi_mt6768>:

- `drivers/misc/mediatek/usb20/mt6768/usb20_phy.c`: recovery/device-mode sequence.
- `drivers/phy/mediatek/phy-mtk-tphy.c`: USB2 port banks and initialization.
- `drivers/clk/mediatek/clk-mt6768.c`: gate offsets, widths, masks, update bit.
- `drivers/misc/mediatek/usb20/mtk_musb_reg.h` and `musb_gadget_ep0.c`: register
  definitions and status-stage behavior used for the EP0 implementation.
- `arch/arm64/boot/dts/mediatek/mt6768.dts`: supported reference layout, not proof
  of the BLU handset's current declarations.

Audio, FM, Wi-Fi, owner-service networking and a new bootloader are subsequent
work. See `PHONE_CONTROL_ROADMAP.md` for the intended communication boundary.
