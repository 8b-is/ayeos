# Board5: SmartPA identity and inherited I2C4 state

Author: OpenAI GPT-6 Astra (`openai/gpt-6-astra`), 2026-10-31.
Family context v2 / seed `8b-is-family-seed-v1`.
Base: Board4 r3 `f593aab681ec3a03faf2ce0b4ab8c8897ce41781`.

Download: <https://g8t.is/downloads/ayeos/studio-board5/>.
This first Board5 image is an **audio-path diagnostic**, not an audible tone or
FM implementation. Board4 r3 has operator-reported real handset enumeration and
full FDT transfer; Board5 requires its own hardware verification.

## What the captured FDT establishes

The private live FDT has 207221 bytes, 979 nodes and 4702 properties. Its
whole-file integrity was verified separately from the operator's USB logs.
Inspection confirms:

- `/sound` references the MT6768 AFE platform and its speaker child references
  the first `awinic,aw87xxx_pa` node through `sound-dai`.
- AFE at `0x11220000`, the audio syscon, clock references and pinctrl states
  describe resources. They do not establish enabled clocks or an active route.
- I2C4 at `0x11004000`, DMA region `0x11000680`, requested 400000 Hz,
  `clock-div=1`, open-drain, IRQ-select and repeated-start/stop properties.
- Infra clock provider references ID 11 (main/I2C4) and 38 (DMA).
- SmartPAs at addresses `0x58`/`0x59`, indices 0/1. No amplifier reset GPIO or
  supply property is declared on these two nodes.
- **Charger devices also occupy I2C4**, at `0x6a` and `0x4b`. The probe addresses
  only the two SmartPAs; it does not scan the bus.
- CONNSYS and BTIF declarations match the supplied resource map. The FDT does
  not establish a running MCU, a particular patch filename or an FM transport.
  Dynamic reserved-memory sizes/alignment are allocation requests, not fixed
  physical firmware-load destinations.

An Awinic reference matching `awinic,aw87xxx_pa` reads register `0x00` for chip
identity and loads `aw87xxx_acf.bin`. That file supplies amplifier profile data;
it is not necessarily executable MCU firmware. Thus a claim that the entire
audio path needs no external data is premature. Once the exact variant and
board profile are established, the required register sequence may be embedded
in a standalone driver. No amplifier profile is guessed here.

## Implemented operation

Build feature `audio-report` includes the verified Board4 USB engine. Before USB
attachment, a bounded probe checks a strict FDT profile with resolved clock
phandles (not fixed phandle numbers), direct-root address mapping, enabled nodes,
exact I2C regions, and both amplifier declarations. Unsupported consumer GPIO,
supply, power-domain or pinctrl dependencies stop the probe.

The runtime reads TOP CLK_CFG_6, infra CG4 and GPIO105/106 mode state. It requires
inherited enabled clocks, SCL4/SDA4 in mode 1, idle START/FIFO, nonzero TIMING,
high-speed mode disabled and open-drain IO_CONFIG. Missing prerequisites produce
an explicit report rather than a guessed clock rate or bus reconfiguration.
The requested FDT rate is **not** reported as a measured/inherited bus rate.

On a matching configured bus, each ID read uses the **MT6768 I2C v2** layout:
FIFO PIO, one-byte register address `0x00`, repeated-start direction change,
one-byte read, two transactions, native-width W1C status acknowledgement.
Register widths are 16-bit except 8-bit FIFO data. DMA is disabled for these
transfers. The first error stops further probes. Status distinguishes NACK,
arbitration loss, controller error, timeout, FIFO length mismatch and MMIO fault.
A raw ID of zero/255 remains a raw result, not an identified working amplifier.

Each transfer has 32768 status-poll iterations and up to 256 START-clear polls;
cleanup has up to 256 START-clear polls. These are work budgets, not calibrated
durations. A bus transaction timeout is distinct from a CPU interconnect stall;
the skipped-exception mechanism cannot guarantee recovery from every physical
bus hang. The loop has no allocation or unbounded retry.

After a transfer error, the I2C controller gets a soft reset (not an amplifier
reset). Saved configuration is restored and read back where possible. A fault
stops subsequent audio MMIO; no cleanup success is claimed after a fault. USB
uses its own fault-stop boundary and exposes the frozen report. The probe never
runs again in the USB polling loop. Existing bounded USB status rendering is
retained. No PWRAP/codec, AFE, CONNSYS, amplifier-value, GPIO-mode, clock-parent,
DMA-buffer, charger-address or boot-partition writes are part of this image.

## USB and report format

VID:PID `18d1:4ee0`, **bcdDevice `0545`**, screen banner `studio-board5`.
The VID/PID remains experimental/reused. Descriptor checks prevent the old
capture script from treating this image as Board4. Keep `capture_board4.py`
beside `capture_board5.py`; it supplies the shared ioctl/FDT capture helpers.

All vendor requests are host-initiated IN reads, exactly 64 bytes:

- `0xb0`/`0xb1`: unchanged Board4 FDT transport.
- `0xb2`, value/index zero: `A5IF`, u32-LE version 1, report size 192,
  whole-report CRC32, chunk size 48, identity `board5-aud\0\0`, zero padding.
- `0xb3`: chunk number = value | (index << 16); `A5CH` followed by the same
  offset/u16-length/u16-version/CRC32/payload envelope as Board4.
  Four chunks reconstruct the report. Out-of-range and malformed requests stall.

Report: 48 little-endian u32 words, with `0xffffffff` meaning not observed:

| Word | Meaning |
|---|---|
| 0..2 | `A5RP`, version 1, byte length 192 |
| 3 | Boot FDT CRC32 |
| 4 | Overall status |
| 5 | FDT profile matched (0/1) |
| 6 | Transfer polling budget |
| 7 | Reserved zero |
| 8..14 | TOP CFG6, infra CG4, GPIO mode13, I2C START, IRQ, FIFO, DEBUGSTAT |
| 15..31 | Saved registers at offsets `04 08 10 14 18 1c 20 28 2c 30 34 3c 44 48 90 e8 f8` |
| 32..36 | Address58: status, raw ID, IRQ, FIFO count, status-poll count |
| 37..41 | Address59: same fields |
| 42 | Configuration restored and read back (0/1) |
| 43..47 | Reserved zero |

Status IDs: 0 not run, 1 complete, 2 FDT mismatch, 3 fault, 4 clock off,
5 pinmux mismatch, 6 bus busy, 7 inherited configuration unsupported,
8 timeout, 9 NACK, 10 arbitration lost, 11 controller error,
12 FIFO length error, 13 restoration failed. Overall failure and individual
probe results are independent; inspect both. Restore=0 also applies when no
configuration changes were attempted.

## Laptop operation

Verify `SHA256SUMS`, deploy using the reviewed local `boot_b` procedure, then
check the Board5 banner and select its exact USB device node:

```sh
sudo python3 capture_board5.py --device /dev/bus/usb/BBB/DDD \
  --output "$HOME/board5-capture-01"
```

The output directory must be new. The script writes private `board5-report.bin`
and `board5-report.json`. Add `--include-fdt` to request and verify the complete
private FDT as well; its CRC must match the report's boot FDT fingerprint.
Return the report (or full screen and host logs if USB fails). If a prerequisite
is missing, that report is the next initialization input, not a successful ID
read. Public artifacts contain no captured private FDT or stock firmware.

## Pinned reference and licensing

Reference repository:
<https://github.com/OnePlusOSS/android_kernel_modules_and_devicetree_oneplus_mt6991/tree/76d6e1e5b8deb0bcd00355d7a9e68638a501d8bc>.
This is a compatible MediaTek reference, **not proven BLU vendor source**.

Under `kernel/kernel_device_modules-6.6/`:

- `drivers/i2c/busses/i2c-mt65xx.c:254–285,576–590`: v2 offsets and MT6768 data;
  `1999–2111,2257–2303,2408–2420`: PIO, combined read and W1C polling.
  Copyright 2014 MediaTek; Xudong Chen; GPL-2.0-only.
- `drivers/clk/mediatek/clk-mt6768.c:1058–1060,1181–1185,1358`:
  I2C mux and I2C4 gate location.
- `drivers/pinctrl/mediatek/pinctrl-mtk-mt6768.h:1007–1029,1737`:
  GPIO105/106 and mode-register layout; MediaTek/Light Hsieh.
- `sound/soc/mediatek/mt6768/mt6768-mt6358.c` and `sound/soc/codecs/mt6358.c`
  remain route/calibration/power-sequence references for the later audible step.

Under `vendor/oplus/kernel/audio/codecs/aw87xxx/`:
`aw87xxx_device.h:46`, `aw87xxx_device.c:1253–1276` establish register-zero ID;
`aw87xxx.c:100,1378,2155` establish the ACF filename, loader and compatible.
No Awinic profile data is redistributed in this image.

`audio_probe.rs` is a GPL-2.0-only Rust adaptation of the cited MediaTek sequence,
not a clean-room claim. Corresponding buildable source and
`LICENSES/GPL-2.0-only.txt` accompany the binary.

## Reproduction and next milestone

From `baremetal/`: `cargo build --release --locked --features audio-report`.
Extract with `llvm-objcopy -O binary`, then package using
`scripts/package_bringup.py --board board5 --kernel RAW --output NEW_DIRECTORY`.
Use Rust 1.99.1 and the `aarch64-unknown-none` target. Validation commands:

```sh
rustc --edition=2021 --cfg 'feature="audio-report"' --test baremetal/tests/audio_board5.rs -o /tmp/board5-tests
/tmp/board5-tests
rustc --edition=2021 --test baremetal/tests/usb_board4.rs -o /tmp/board4-tests
/tmp/board4-tests
python3 -m unittest discover -s scripts -p 'test_capture_board*.py' -v
```

Next audible milestone needs the real IDs, board-specific amplifier profile,
verified PWRAP/MT6358 initialization, route selection and bounded tone/stop path.
FM additionally needs verified CONNSYS power/reset/firmware and WMT/STP exchange.
These remain unresolved; this release makes no audible-output or radio claim.
