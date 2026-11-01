# Board6: inherited amplifier windows and MT6358 readback

Author: OpenAI GPT-6 Astra (`openai/gpt-6-astra`), 2026-11-01.
Family context v2 / seed `8b-is-family-seed-v1`.
Base: `cbe55b8e9c1d5a4a65e06fd3a040e7c33e56f026`, hardware-tested Board5 r2.
Downloads: <https://g8t.is/downloads/ayeos/studio-board6/>.

## Purpose and evidence

The operator's Board5 r2 run read register00 at both addresses58/59 as decimal35
(hex23). The uploaded192-byte report's hashes, CRC and exact JSON decoding were
independently verified. That establishes two successful reads in the reported run,
not identification of AW87339. The pinned Awinic driver associates that model
with PID39, alongside other models. A newer PID23 is a user-suggested possibility;
an exact BLU driver/profile/datasheet establishing it has not been found.

`hardware/blu_g75_g1170/audio_device.xml`, introduced in
`e432c24abe817f89bcd9e81f72017e6ea0a058a9`, SHA256
`1042146900aca0cbf8395656997d1d7a89447ef08ef034829727f8132f959193`, selects:

- card `mt6768mt6358`, initial `Audio_Speaker_class_Switch=CLASSAB`;
- speaker: `DAC In Mux=Normal Path`, `LOL Mux=Playback`, both profiles `Music`;
- receiver: `HPL Mux=Audio Playback`, profile1 `Receiver`; RCV Mux commented out.

These are intended mixer routes/profile names. They do not contain the profile
register values, prove physical wiring, or prove a particular gain. The supplied
stock `lsmod` line remains operator-attributed evidence. The matching reference
driver loads `aw87xxx_acf.bin`; no zero-external-data full-audio claim is made.

## Amplifier capture

Before USB attachment, Board6 attempts one combined one-byte-register-address
write plus one-byte read for each register00..7f at address58, then59. No burst
auto-increment is assumed. It reuses Board5's strict FDT/clock/pin/inherited-timing
checks, native-width I2C sequence, AP FIFO bits7:4, and restoration/readback of17
saved configuration registers. There are256 slots, each recording status, raw
value, IRQ, decoded FIFO count and poll count. Register00 results also remain in
the embedded Board5 summary; its overall status covers the entire window.

**These are inherited sequential observations, not POR defaults or an atomic
snapshot.** LK has already run. Register readability, reserved-address behavior
and read side effects on unidentified23 silicon are not established. The code
submits no amplifier register-value writes, chip reset, profile load or tone.

The first failed read stops both remaining windows. Non-fault transfer errors
reset the I2C controller, then restore/read back the saved configuration as in
Board5. Unattempted entries remain NotRun with null observations, never zero.
An MMIO fault stops subsequent probe MMIO; no restoration is claimed after it.
PWRAP can run after a non-fault I2C failure, but never after a probe MMIO fault.

## Verified PWRAP mechanism

Reference, **not proven exact BLU source**:
[OnePlusOSS/android_kernel_modules_and_devicetree_oneplus_mt6991 at
76d6e1e5b8deb0bcd00355d7a9e68638a501d8bc](https://github.com/OnePlusOSS/android_kernel_modules_and_devicetree_oneplus_mt6991/tree/76d6e1e5b8deb0bcd00355d7a9e68638a501d8bc).
Paths below start at `kernel/kernel_device_modules-6.6/`.

- `drivers/soc/mediatek/mtk-pmic-wrap.c:982–1106`: explicit MT6768 layout.
  Base1000d000; MUX0, WRAP_EN4, DIO8, arbitration6c, MAN_EN7c,
  WACS2_EN9c, INIT_DONE2a0; WACS2 **CMDc20 / RDATAc24 / VLDCLRc28**.
- Same file32–58: RDATA low16 is returned data, FSM bits18:16;
  idle0, request2, wait4, valid-clear6. SYNC_IDLE bit20. MT6768's descriptor
  at4618–4630 selects **INIT_DONE bit22**, not21.
- Same file3502–3531: 16-bit read command `(address >> 1) << 16`; write bit31
  remains zero; wait FSM6, take low16 and acknowledge VLDCLR=1.
- Same file266–285 and61: MT6358 DEW_READ_TEST at040e, expected5aa5.
- `drivers/mfd/mt6397-core.c:744–750,812`: identify using SWCID high byte.
  `include/linux/mfd/mt6358/registers.h:10`: SWCID000a.
- `sound/soc/codecs/mt6358.h:276,369–401`: AFE_UL_DL_CON0=2288,
  AUDENC_ANA_CON0..12=2388..23a0, AUDDEC_ANA_CON0..15=2408..2426, step2.
- The previously pinned `drivers/clk/mediatek/clk-mt6768.c:1169–1173,1303–1306`
  locates PMIC_AP gate at infra1090 bit1. No clock enabling is attempted.

Independent related MT6768 header corroboration:
[LineageOS/android_kernel_xiaomi_mt6768 at
8745b22c4bb0d208914d62cfd5b8c6f7a7bdf991](https://github.com/LineageOS/android_kernel_xiaomi_mt6768/blob/8745b22c4bb0d208914d62cfd5b8c6f7a7bdf991/drivers/misc/mediatek/include/mt-plat/mt6768/include/mach/upmu_hw.h),
lines18–19 defines HWCID0008 and SWCID000a;1755,1848–1880 corroborate the audio
addresses. Linuxv6.6 `include/linux/mfd/mt6397/core.h` defines MT6358_CHIP_ID=58;
its MT6358 codec header also agrees on these audio addresses.

Thus **literal HWCID=6358 is not an assumed success condition**. Board6 records
both raw IDs and requires SWCID high byte58 before the audio-register reads.
DEW_READ_TEST must equal5aa5 before any ID read. HWCID itself has no guessed value.

The old `main.rs` PWRAP code uses offsets0c0/0c4/0c8, FSM2 and audio addresses
2188/2208. Those disagree with these references. Board6's early diagnostic path
never calls that legacy sequence; its constants have not been migrated for
other build modes. On this layout0c0/0c4/0c8 are interrupt status/clear registers.

## Inherited-state gates and limits

- Validate parsed FDT root MT6768, 2/2 root cells, direct-root infra/pwrap
  addresses/sizes, exact MT6768 wrapper compatible and MT6358 child; reject
  missing/duplicate candidates, disabled nodes and unsupported translation.
- The private Board4 capture matches. Its wrapper has one clock specifier but
  two clock names. Board6 does not infer a clock index from this discrepancy.
- Read infra CG0 first; gated PMIC_AP stops wrapper accesses. Snapshot11 values:
  infraCG0, MUX, WRAP_EN, DIO, arbitration, MAN_EN, WACS2_EN, INIT_DONE2,
  RDATA, raw interrupt0/1 flags. Raw flags are observed, not cleared.
- Require wrapper mux, WRAP_EN/WACS2_EN/INIT_DONE2=1, manual disabled,
  RDATA INIT_DONE22, FSM0, SYNC_IDLE20, request19 clear. An inherited FSM6 is
  **not consumed or cleared**. Other clients/arbitration configuration is retained.
- Only WACS2 read-command and valid-clear writes are issued to PWRAP. No PMIC
  value writes, wrapper reset, SPI timing/clock/cipher setup, or IRQ acknowledgement.
- Every read rechecks idle/init, waits at most32768 RDATA polls for FSM6, and
  waits at most32768 polls for idle after acknowledgement. Counts are work budgets,
  not milliseconds. A timeout stops the PMIC sequence; it does not turn stale
  low16 into data. A value received before an ack timeout remains raw evidence
  with an error status. A pending timed-out operation is not forcibly reset.
- A final RDATA snapshot and `last_observed_idle` distinguish completion from
  leaving an operation pending. This is not a proof of permanent quiescence or
  absence of all PMIC bus errors. Read-test and ID validation constrain the claim.
- The existing watchdog restart register is fed before each register transaction;
  that write uses the same fault guard. No new watchdog mode change is introduced.

## USB and screen

Identity **18d1:4ee0, bcdDevice0546**; banner **studio-board6**. Full-speed EP0-only,
vendor-specific interface, host-requested frozen data. No CDC/fastboot protocol.
USB uses its own post-probe fault boundary and never re-executes probes on reset.

Requestb2 returns64-byte `A6IF`: version1, report length6400, whole CRC32, chunk48,
label `board6-aud` plus two zero bytes, remaining zeros. Requestb3 takes a32-bit
chunk number in value/index and returns `A6CH`, byte offset, u16 payload length,
u16 version1, payload CRC32 and up to48 payload bytes, zero padded. There are
**134 chunks:133 full48-byte payloads and a final16-byte payload**. Requestsb0/b1
retain optional private FDT capture. Only configured, bounded IN requests work.

Report layout, little-endian u32 words (6400 bytes):

| Words | Content |
|---|---|
| 0..7 | A6RP,version1,size6400,FDT CRC,256 amp entries,33 PMIC entries,budget32768,inherited flag1 |
| 8..15 | Reserved zero |
| 16..63 | Embedded192-byte Board5-format summary, including register00 results |
| 64..1343 | 256 entries of5 words: status/value/IRQ/FIFO count/polls;58 then59, ascending00..7f |
| 1344..1375 | PWRAP status/profile/budget/idle;11 initial snapshots;final RDATA;16 reserved zeros |
| 1376..1540 | 33 entries: status/value/last response RDATA/response polls/ack-idle polls |
| 1541..1599 | Reserved zero |

PMIC order: DEW_READ_TEST,HWCID,SWCID,AFE_UL_DL_CON0,AUDENC0..12,AUDDEC0..15.
Unknown observation=ffffffff. PMIC statuses0..10: not_run,complete,fdt_mismatch,
fault,clock_off,inherited_config_unsupported,bus_busy,timeout,read_test_mismatch,
swcid_mismatch,ack_idle_timeout. Amplifier statuses retain Board5 meanings.

The fallback page shows every amplifier byte (`--` means incomplete), all33 PMIC
values with status, counts, restoration, initial/final WACS2 and report CRC. USB
is the complete record of per-read errors/polls and controller snapshots.

## Build, tests and operator handoff

From `baremetal/`, with Rust1.99.1's AArch64 target available:

```sh
env PATH=/home/hue/.cargo/bin:/usr/local/bin:/usr/bin:/bin \
  cargo build --release --locked --features board6-report
```

From the repository root:

```sh
rustc --edition=2021 --cfg 'feature="audio-report"' \
  --cfg 'feature="board6-report"' --test baremetal/tests/audio_board5.rs -o /tmp/opencode/board6-tests
/tmp/opencode/board6-tests
python3 -m unittest discover -s scripts -p 'test_capture_board*.py' -v
```

Tests include all256 addresses/registers, partial failures at boundaries,
I2C restoration, fixed PMIC read commands, inherited gates/busy states, FSM2
timeout versus FSM6 data, bad read-test/ID, ack timeout, fault injection at every
successful-path access, FDT mismatch/truncation, final short chunk/CRC/padding,
reset-stable report and descriptor rejection. Receipt records actual runs and
independent archive rebuild. Simulation does not verify handset behavior.

After checksummed download and reviewed **boot_b-only** deployment, verify the
Board6 banner and bcd0546. Keep **all three capture scripts** together:

```sh
sudo python3 capture_board6.py --device /dev/bus/usb/BBB/DDD \
  --output "$HOME/board6-capture-01" --include-fdt
```

The directory must be new. Return `board6-report.bin` and `.json` privately;
`--include-fdt` is optional, and the DTB stays private. If USB fails, return a
full-resolution screen photograph and fresh host USB logs. Existing images and
the reviewed recovery path remain available. No handset result exists at build
time; audio output, amplifier identity/profile and FM remain later milestones.

License: PWRAP/I2C adaptations GPL-2.0-only; retain MediaTek/Flora Fu/Xudong Chen
and pin/clock source attribution. Corresponding buildable source and
`LICENSES/GPL-2.0-only.txt` accompany publication.
