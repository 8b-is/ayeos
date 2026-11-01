# Audio tone1: bounded SGEN and observed Off

2026-11-01 · OpenAI GPT-6 Astra (`openai/gpt-6-astra`).
Family context v2 / seed `8b-is-family-seed-v1`.
Base: `f9109f44c9fc1c7a11b35e162584b0ba306a8394`.

Feature `tone-report` adds one boot-time MT6358 SGEN attempt to the
[alpha foundation](AUDIO_ALPHA1.md). Identity: **audio-tone1, USB18d1:4ee0,
bcdDevice0562**. This is bare-metal Rust, without Linux/Android or an allocator.
The package remains5.6.0; feature, screen, command line and USB release distinguish
the artifact. **This implementation has not yet been tested on the handset.**

SGEN bypasses PCM. Marine still processes the1024-sample synthetic fixture;
live input, PCM transport and Phoenix processing remain subsequent milestones.
Neither register readback nor QEMU establishes audibility, output frequency,
sound level, pop behavior or physical shutdown. Return those observations separately.

## Evidence used

Register sequences derive from MediaTek's GPL MT6358 codec, pinned HelloVolla
revision `af6613a25d6e6dd40d5c4d3c31d5093b86ba1c90`:
[mt6358.c](https://github.com/HelloVolla/android_kernel_device_modules_volla_mt6878/blob/af6613a25d6e6dd40d5c4d3c31d5093b86ba1c90/sound/soc/codecs/mt6358.c),
SHA256 `28f3b0aa135cf47c4dafef4417b53001518549a179832ab842a22929e89a382f`.
Relevant functions are `mt_sgen_event`, `mt_lo_event`, `zcd_enable`,
`set_speaker_gain`, `mt_clksq_event`, and the DAPM supply ordering/routes.
The privately supplied BLU factory module's SGEN/lineout disassembly corroborates
this source sequence. Source and hardware evidence are different claims.

Regulator mapping is corroborated by the pinned OnePlus
[`mt6358-regulator.c`](https://github.com/OnePlusOSS/android_kernel_modules_and_devicetree_oneplus_mt6991/blob/76d6e1e5b8deb0bcd00355d7a9e68638a501d8bc/kernel/kernel_device_modules-6.6/drivers/regulator/mt6358-regulator.c),
SHA256 `8a399fcfbf2f534655a5315aafcdfa86f26e42aa25b3bbe7b16372664b8ebef1`.
That tree's newer codec differs from the HelloVolla codec; it was not silently
substituted for the matching reference. FIFO/WACS2 protocol provenance continues
from [Board5](SCREEN_BOARD5_HANDOFF.md) and [Board6](SCREEN_BOARD6_HANDOFF.md).

BLU factory ACF:1943 bytes, SHA256
`7730dd03385abc760c316df75b6aa9670461f0d0d8d3629937962fd8117b2726`.
Private verification checked table bounds, outer CRC8, nested checksums, index
selection and exact ordered Music/Off pairs. Handset FDT maps index0→58,
index1→59; both identify PID23/AW87394. No reset GPIO is configured.
Factory/installed-OTA ACF byte equality remains unknown. The14 newly uploaded
Awinic sources match the earlier pinned sources byte for byte. The separate
Infinix attachment is a129-byte LFS pointer, not profile data, and is unused.

## Ownership, gates and sequence

`tone_board.rs` requires the exact timer declaration (ARMv8,13MHz) and
`/pwrap@1000d000/mt6358-pmic/mt6358regulator/ldo_vaud28`, namedvaud28,
fixed2800000µV, always-on. This establishes the named board rail; no missing
codec supply phandle is invented. Runtime requires enable1ac4.bit0 and
status1ad2.bit15 already high. **VAUD28 is not changed or switched off.**

Before any audio register-value write, require:

- Existing I²C and PWRAP FDT profiles, complete pre-tone Board6 reads,
  I²C restoration, WACS2 idle, alpha tests7/0 and zero unexpected faults.
- Rechecked inherited controller clocks, pins, timing and idle state. Never
  retune shared clocks, enable DMA, scan the charger bus or consume inherited FSM6.
- `CNTFRQ_EL0=13000000` and advancing `CNTPCT_EL0` over at least130 ticks.
  This checks configuration/advancement, not independent oscillator calibration.
- MT6358 read-test5aa5, SWCID high byte58, both amplifier IDs23, VAUD28 as above,
  inactive inherited AFE, DAC/HP/receiver/lineout and ADC routes.

One boot CPU owns the buses during the attempt, with DAIF masked. The state
machine then:

1. Mutes/disables SGEN and opens LOL. Writes full-byte amplifier Off02=00,
   software reset00=aa (readback must remain ID23), then Off again on each chip.
   On AW87394 (`PID=0x23`), `SYSCTRL` (`0x02`) reads back `0x3e` (`EN_PA=0`,
   `EN_SPK=1`) in power-on/standby state or `0x00` if latched; both `0x00` and
   `0x3e` are accepted as PA Off readbacks.
2. Enables clock buffer, AUDGLB, CLKSQ, NCP/ZCD/AUDIF/AUD clocks, observes250µs,
   enables DAC/power/testmodel and AFE clocks, observes250µs, then AFE.
3. Uses the documented SGEN sequence229a=0006,2296=cba1,229a=0003,229a=000b;
   CFG022c0 maskff3f→0 and CFG122c2→0001. Selects SGEN, enables SRC/DAC while muted.
4. Applies the lineout NCP/LDO/bias/DACR→LOL sequence with250µs/100µs waits.
   Both lineout gain fields stay at the source's named **−40dB** value1f
   (`ZCD_CON1=0f9f`), replacing its initial−10dB/runtime gain choice. SGEN's
   undocumented attenuation/channel interpretations are not used.
5. Applies both18-pair BLU Music profiles in their original order. Only04 differs:
   3b at58,2b at59. Verifies each write; waits30ms for the configured ZCD timeout.
6. Records a timestamp **before** the unmute transaction. Requests stop when
   elapsed counter ticks reach3250000 (250ms), without extending that deadline
   for command duration. The actual silent interval needs physical measurement.

Normal and error cleanup use the same routine. Attempt SGEN mute/disable, LOL
open, both full-byte PA Off writes, SDM/scrambler stop0000/cba0, SRC/AFE/DACR/LOL
off, minimum gain, bias/ZCD/NV/LDO/NCP shutdown and clock gating. Preserve the
vendor250µs pre-clock-powerdown waits. Do not restore inherited audio enable bits.
Every logical update records its masked readback, including a mismatch.

Each cleanup operation gets an independent fault/recovery boundary. A failed58
Off cannot skip59 or PMIC stop; a dead WACS channel cannot skip PA Off. Recovery
resets only an owned failed I²C controller transaction and restores its17 saved
configuration registers. WACS2 may acknowledge a late response only after this
path submitted a command. No PMIC-wrapper reset or guessed GPIO fallback exists.

Final, fresh reads establish a17-bit Off mask: bothPA02 in `{0x00, 0x3e}` plus the15 predicates
in `tone::OFF` (SGEN mute/disabled, SDM/scrambler off, SRC/AFE off, LOL open/core/bias
off, DACR off, audio global off, IBIST/ZCD/NV/LDO/NCP off and clock gates).
**verified-Off means these observed predicates**, not zero power or an acoustic
measurement. I²C restoration and final WACS2 idle are checked separately.
A preflight rejection makes no Off claim.

## Bounds and fault limits

All timing arithmetic is integer:13ticks/µs; frequency13MHz; no wrap accepted.
Startup/tone checks a2-second deadline at logical operation boundaries. Each bus
wait has a32768-poll cap and, when usable, a20ms counter deadline; START-clear
has256 polls. Delays/tone loops have16777216 iterations. Cleanup ignores an
expired startup deadline, so it can still stop; its finite transaction/work
budgets apply independently. These are software work bounds, not guarantees
against a CPU/bus lockup or nonreturning load. The watchdog remains disabled;
it is not used as an unverified audio deadline.

Normal exact-MMIO faults abort startup and enter independent cleanup. A fatal
exception/panic after arming makes a bounded best-effort stop before displaying
the fatal screen. It temporarily permits the existing six exact MMIO fixups;
a second nonrecoverable exception halts. The first fatal record is preserved
across recoverable cleanup faults. Stack corruption, nested-handler faults,
an inaccessible counter, or a permanently blocked bus can defeat this attempt.
The screen reports whether emergency Off was verified; fatal USB export is not
promised. If sound persists, use the established physical power-off procedure.

## Frozen evidence and wire format

All work finishes before USB attachment. Bus reset never reruns a tone, test,
probe or cleanup. Existingb0/b1 private FDT, b2/b3 **pre-tone** Board6 andb4/b5
alpha channels remain. Alpha freezes after the tone, so its fault counter covers
tone work while its Board6 CRC still names the inherited pre-tone snapshot.

New configured vendor IN requests, length64:

- b6,value/index0: `ATIF`,u32 version1,size6400,wholeCRC32,chunk48,
  twelve-byte`audio-tone1\0`,32 zero bytes.
- b7,32-bit chunk number in value/index: `ATCH`,u32 byte offset,u16 length,
  u16 version1,u32 payloadCRC32,up to48 payload bytes,zero padding.
  134 chunks, final payload16 bytes. No OUT operation or tone replay command.

`ATRP` little-endian layout:

| Bytes | Fields |
|---|---|
| 0..15 | magic,u32version1,size6400,flags(bit0frozen,bit1dirty) |
| 16..43 | u32outcome,error,cleanup-error-count,Off-mask,controllers-restored,event-count,dropped |
| 44..63 | u32Board6CRC,alphaCRC,FDTCRC,required-Off-mask,requested250000µs |
| 64..103 |40ASCII Git revision bytes |
| 112..151 | u64frequency,start,unmute-attempt,first-stop-attempts-end,end ticks |
| 256..6399 | up to256 ordered24-byte events; unused bytes zero |

Events: u64tick,u8kind,u8phase,u16error,u16address,u16mask,u32expected,u32observed.
Kinds1/2/3/4/5=PMICread/update,PAread/write,delay. Phases0..4=preflight,startup,
tone,cleanup,verification. PA address packsclient in the high byte. Tick0 means
unavailable. Error0=success;1..12=gate,timer,deadline,fault,busy,timeout,NACK,
controller,FIFO,readback,restore,capacity. Failed transaction observations use
ffffffff; a readback mismatch retains the actual value. Delay expected isµs,
observed is elapsed ticks. Reset00=aa expects ID23 rather than an echo ofaa.
Other reserved bytes are zero. Outcomes1/2/3/4=complete,preflight-rejected,
aborted-with-Off,Off-unverified. Cleanup errors remain errors even if later final
reads establish Off. CRC detects corruption, not publisher identity.

## Build, checks and operator procedure

Run `bash scripts/check_tone.sh` and `bash scripts/check_alpha.sh`, existing
Board3–6/capture tests and the seven feature builds in CI. Tone checks exercise
normal ordering, failures before/after each logical bus effect, every MMIO access
in the four bus transactions, failed independent shutdowns, timer failure,
FDT mismatches, malformed wire data, partial captures, USB reset and EL1/EL2
fatal-record preservation during six cleanup data aborts. QEMU is an architectural
test, not MT6768 hardware emulation.

From `baremetal/`, build with `cargo build --release --locked --features tone-report`,
then `llvm-objcopy -O binary target/aarch64-unknown-none/release/ayeos-baremetal-mt6768 kernel.bin`.
Package with `python3 ../scripts/package_bringup.py --kernel kernel.bin --output NEW_DIR --board audio-tone1`.
An extracted source archive needs the published full `AYEOS_BUILD_REVISION` and
`AYEOS_BUILD_DIRTY=0`. The release receipt records exact toolchain and hashes.

Use a fresh download/capture directory and verify the publishedSHA256SUMS.
Retain the known-good Board6 recovery image. Listen **before reboot**: the one
tone attempt precedes USB enumeration. With the intended phone already in
bootloader mode, the only mutation sequence is:

```sh
timeout 30 fastboot -s "$SERIAL" getvar current-slot
timeout 60 fastboot -s "$SERIAL" flash boot_b studio-audio-tone1.img
timeout 30 fastboot -s "$SERIAL" --set-active=b
timeout 30 fastboot -s "$SERIAL" reboot
```

Run these with immediate failure handling (`set -e`) and the image's expected
SHA256 enforced **before** any device command. Stop if any command fails; keep
its full output. The release handoff gives the exact image hash and ready-to-use
procedure. The earlier uploaded automatic flasher still needs its separately
requested shared deadlines, subprocess timeouts and strict four-hex bcd check.

Keep all five capture helpers together; after finding the exact0562 USB node:

```sh
sudo python3 capture_tone.py --device /dev/bus/usb/BBB/DDD --output NEW_PRIVATE_DIR --include-fdt
```

Capture saves packets as received, errors/partial results, decoded reports and
SHA256SUMS; refuses an existing output directory. Return this bundle privately,
exact flash log/image hash, screen photo if available, and whether each speaker
sounded, stopped, popped or stayed silent. Expected successful register result:
clean publishedrevision,tests7/0,unexpectedfaults0,outcomecomplete,verified-Offtrue,
controllers-restoredtrue. Preserve and report rejection/failure equally.
