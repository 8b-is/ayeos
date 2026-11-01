# Board5 r2: AP FIFO count decoding

Author: OpenAI GPT-6 Astra (`openai/gpt-6-astra`), 2026-10-31.
Family context v2 / seed `8b-is-family-seed-v1`.
Base: `f7fb98fc9e915ebf6fa09d966fe8dfa77aa1ea24` (Board5).
Downloads: <https://g8t.is/downloads/ayeos/studio-board5/r2/>.

## Hardware evidence and correction

The laptop operator reports Board5 flashed to `boot_b`, enumerated as
`18d1:4ee0`, bcdDevice 0545, and returned the complete 192-byte report. The cloud
builder independently verified the uploaded report's length, CRC32, SHA-256,
private permissions and exact binary-to-JSON decode agreement.

The report matched the FDT profile, with TOP CFG6 `01030202`, infra CG4 zero,
GPIO mode13 `10000111`, START zero, IRQ `0001`, FIFO_STAT `2201` and DEBUGSTAT
`0300`. This establishes the observed register values, not a completed SmartPA
read. Both amplifier probes were `not_run`; no I2C writes were attempted by r1.
IRQ bit0 is the completion latch, but the originating LK transaction cannot be
identified from this report. Calling it specifically a charger transfer is an
inference. Its FDT CRC `745db420` differs from the earlier Board4 capture; no
full FDT from this Board5 boot accompanied the report.

r1 incorrectly treated `FIFO_STAT & 0x1f` as a byte count. Its mock repeated the
same mistake. The AP FIFO count is extracted with `(FIFO_STAT >> 4) & 0x0f`:
`2201` gives zero; the proposed one-byte state `0110` gives one. Low flag bits
and high pointer bits are excluded. The raw initial FIFO snapshot remains in the
report; per-amplifier `fifo_count` remains the **decoded count**, not raw status.

Evidence supporting the correction:

- The previous pinned 6.6 `i2c-mt65xx.c:2275–2286` uses the low-five-bit mask
  specifically under `ch_offset_i2c == i2c_offset_scp`. It was incorrectly
  generalized to the AP FIFO. Its ordinary receive path at 2408–2420 reads the
  requested byte count directly and does not validate with that mask.
- MediaTek's older AP FIFO receive path explicitly uses the shifted count:
  [LineageOS/android_kernel_cyanogen_mt6735,
  revision f4e5f99bada5dc7d905a908c4e1d8218ac593a37,
  drivers/misc/mediatek/i2c/mt6735/i2c.c:473–480](https://github.com/LineageOS/android_kernel_cyanogen_mt6735/blob/f4e5f99bada5dc7d905a908c4e1d8218ac593a37/drivers/misc/mediatek/i2c/mt6735/i2c.c#L473-L480).
  This is related-controller corroboration, not an exact BLU vendor manual.
- The operator's observed `2201` is consistent with an empty FIFO and equal
  pointers under the supplied field map. A post-transfer `0110` on this handset
  is still a prediction to test, not an already captured result.

## Changes

- Both initial bus gating and post-transfer length validation use bits7:4.
- START bit0 must still be clear and the inherited FIFO count zero before any
  transaction. A nonempty inherited FIFO is not discarded to force progress.
- All 17 configuration registers are collected before bus, pinmux or inherited
  configuration rejection. A clock-off condition still prevents controller
  reads; an MMIO fault stops further reads and leaves unobserved fields null.
- The mock now uses AP raw states: `2201` inherited empty, `0110` one RX byte,
  empty flags after clearing/reading, and explicit multi-byte/empty failures.
- Regression exercises the captured initial snapshots with simulated configured
  timing and simulated ID replies. The original report did not contain timing
  values, so this test does not establish that the handset passes that next gate.

No report-format change is needed: `capture_board5.py` correctly requires a
decoded `fifo_count == 1` for a completed one-byte read. The existing scripts and
USB descriptor remain compatible. bcdDevice stays **0545**; identify r2 by its
image checksum, receipt revision and **studio-board5-r2** screen banner.

## Laptop test

Download/check the r2 `SHA256SUMS` and deploy with the reviewed `boot_b` procedure.
Keep both provided Python scripts together and use a new private output directory:

```sh
sudo python3 capture_board5.py --device /dev/bus/usb/BBB/DDD \
  --output "$HOME/board5-r2-capture-01"
```

Optional `--include-fdt` also captures that boot's complete private FDT. Return
the report JSON/binary, or a full r2 screen and fresh host USB logs. A subsequent
configuration/NACK/timeout result is useful evidence, not successful chip
identification. r2 has no handset result at publication time.

See [the original Board5 handoff](SCREEN_BOARD5_HANDOFF.md) for protocol,
bounded polling, restoration, fault-stop, build commands and licensing. Tone,
amplifier profile selection, codec routing and FM remain subsequent work.
