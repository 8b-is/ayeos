# Audio alpha1: instrumented foundation

2026-11-01 · OpenAI GPT-6 Astra (`openai/gpt-6-astra`).
Family context v2 / seed `8b-is-family-seed-v1`.
Base: `61e1890a10c126dac0c6fc590ea5ed256da53ecf` (verified Board6 main).

This is the first implementation checkpoint toward **boot → self-test → live
samples → Marine measurements → Phoenix PCM output → verified stop → export**.
It links the genuine shared integer Marine engine into the bare-metal image,
adds useful fault evidence and verifies its exception path in an architectural
emulator. **Handset validation of this new image remains pending.**

## Implemented capability

- Feature `alpha-report`, package5.6.0, screen **audio-alpha1**, experimental USB
  **18d1:4ee0 / bcdDevice0561**. Build revision and dirty state appear on screen
  and in the report. The earlier Board6 feature keeps its vector/probe path.
- Boot-time CRC32 known-answer test,1024-sample Marine reference fixture and a
  one-shot `brk #0xa8` exception test. Bits1/2/4 report CRC/Marine/exception;
  expected `passed=7`, `failed=0`. Failure is retained as failure.
- Real `aye/crates/marine-core::MarineInt`, pinned to upstream
  `479d2e9b1badddb90a1b5f0e4ad0ea116f491d5b`. Replace its unused peak-history
  allocation with the count it actually consumes. Extract integer helpers;
  retain original source and license provenance in
  [`baremetal/vendor/marine-core/PROVENANCE.md`](../baremetal/vendor/marine-core/PROVENANCE.md).
  The phone API is bounded to2048 i16 samples per explicit reset. No allocator,
  upstream float shim, dependency download, network call or Linux service is
  involved in the phone's Marine processing.
- Sample-by-sample comparison with unmodified upstream on six2048-sample
  vectors, including silence, onset, extrema, jitter and silence/restart.
  The1024-sample boot fixture's upstream FNV64 is `800d3c2f169c4722`, hashing each
  returned reduced `(i32 numerator,u32 denominator)` in little-endian order.
  Upstream millionth quantization/fallbacks remain; finite equality is not a
  universal equivalence proof or an acoustic result.
- 16-entry stage journal, first unexpected fault and total unexpected-fault count;
  the expected breakpoint is excluded from that count. Freeze1024 bytes before
  USB initialization, tied to the6400-byte Board6 report by its CRC.
- Existing Board6 I²C/PWRAP diagnostics run once, before USB. The report and FDT
  protocols remain available under the alpha identity, with bounded host reads.

Marine currently receives **synthetic fixture samples**. PCM playback/capture,
Phoenix restoration, a bounded SGEN tone/verified Off and FM remain unfinished.
The physical audio acceptance demonstration above has not occurred. Owner
networking remains governed by [PHONE_CONTROL_ROADMAP.md](PHONE_CONTROL_ROADMAP.md).

## Exception mechanism and limits

Alpha uses a separate reset path: mask DAIF, select SP_ELx, establish the128KiB
stack, zero BSS, enable SIMD access, then install VBAR at EL1 or EL2. A2048-byte
aligned table routes all16 vectors to an848-byte frame. The entry saves all
GPRs, all32 SIMD registers, FPCR/FPSR, vector number, CurrentEL, ESR, ELR, FAR,
SPSR and entry SP before entering Rust. SP is the exception-entry stack (not a
lower-EL user stack). ESR/FAR are raw; FAR is meaningful only for applicable
exception classes with valid-address indications.

Recovery requires a same-EL synchronous AArch64 exception, IL=1, and either:

1. Same-EL data abort EC25 at one of six exact linker-retained MMIO load/store
   instructions, resuming at that instruction's registered `ret`; or
2. EC3c, immediatea8, the exact test PC and its one-shot armed flag.

A faulted read returns its initialized zero, **which is not an observation**.
The existing `UsbIo` counter boundary suppresses subsequent transaction accesses
and probes record their failure. USB starts with a separate post-probe boundary.
No arbitrary ELR+4 continuation exists in the alpha. Other exceptions enter a
fatal screen with revision/vector/ESR/PC/FAR/SP/SPSR and stop servicing USB.
Panic reports contain source-file FNV64 plus line/column; resolve against the
exact source archive (`file!()` path, typically `src/...`). Panic text is not
formatted recursively. A nested fault halts in assembly without overwriting
the original record. No framebuffer, stack or memory-map rescue is promised;
failure before screen discovery may leave only the prior boot screen. An early
hardware watchdog can still reset a failed boot.

Diagnostics are **single-boot RAM**, not durable storage or autonomous repair.
The frozen USB snapshot covers pre-attachment work; later USB faults use the
screen's live fault status. Fatal/panic evidence uses the screen, not a claim
that USB can survive a crash. The stage ring has sequence numbers/work outcomes,
not calibrated timestamps or measured latency. No fault test is host-triggered.

## Wire format

Only configured vendor IN requests, fixed64-byte replies:

- `b0/b1`: existing private FDT protocol.
- `b2/b3`: existing Board6 report protocol (`A6IF/A6CH`).
- `b4`: value/index0, length64 → `ADIF`,u32 version1,size1024,whole CRC32,chunk48,
  twelve-byte `audio-alpha1` label,32 zero bytes.
- `b5`:32-bit chunk number in value/index, length64 → `ADCH`,u32 byte offset,
  u16 payload length,u16 version1,u32 payload CRC32,up to48 data bytes,zero padding.
 22 chunks; final payload16 bytes. USB reset never reruns tests or probes.

`ADRP` report offsets (little endian, reserved bytes zero):

| Byte offset | Field |
|---|---|
| 0/4/8 | magic / u32 version1 / u32 size1024 |
| 12 | u32 flags: bit0 frozen volatile snapshot, bit1 dirty build |
| 16 | u32 capabilities: bit0 diagnostics, bit1 Marine fixture; value3 |
| 20/24/28 | u32 passed mask / failed mask / retained event count0..16 |
| 32 | u64 dropped event count |
| 40 | u32 Board6 whole-report CRC32 |
| 64..103 |40 ASCII hex Git revision bytes |
| 112 | u64 unexpected fault count |
| 128..191 |8 u64:vector,EL,ESR,PC,FAR,SPSR,entrySP,action |
| 192/200 | u64 Marine FNV64 / processed sample count |
| 256..767 |16 chronological32-byte events: u64 sequence,stage,result,detail |

Actions0/1/2 mean absent/expected-breakpoint/guarded-data-abort. Fatal action3
cannot occur in a normal frozen report. Stages1..7:entry,framebuffer,CRC,Marine,
exception,Board6,freeze. Test results0/1 mean fail/pass. Board6 event result is
I²C status, detail is PWRAP status; interpret their status tables, not booleans.
Sequence saturates at u64::MAX. The parser checks bounds, reserved fields,
fault/test consistency, journal order, per-chunk/whole CRC and cross-report CRC.
CRCs/FNV detect accidental changes; neither authenticates the publisher.

## Build and validate

Rust1.99.1 / target `aarch64-unknown-none`, Python3 stdlib, QEMU AArch64, LLVM
objcopy/objdump. From the repository root:

```sh
bash scripts/check_alpha.sh
```

This checks reference fingerprints,23 vendored-core tests,55 reference/diagnostic
tests,7 Python alpha tests (including a Rust-produced wire fixture), and four
QEMU runs: EL1/EL2 × fatal unarmed breakpoint/unregistered load. QEMU uses the
**production reset, vector assembly, fixups and Rust handler**, checks every
GPR/SIMD register and NZCV/FPCR/FPSR, installs a test-only translation table and
exercises all six data-abort fixups. Its board is `virt`, not MT6768.

Run the existing Board3–6 host/capture suites and all six feature builds too
(commands are in `.github/workflows/ci.yml`). From `baremetal/`:

```sh
cargo build --release --locked --features alpha-report
llvm-objcopy -O binary target/aarch64-unknown-none/release/ayeos-baremetal-mt6768 kernel.bin
python3 ../scripts/package_bringup.py --kernel kernel.bin --output ../new-package --board audio-alpha1
```

The published receipt supplies the exact revision and checksums. An extracted
archive has no Git metadata: supply `AYEOS_BUILD_REVISION=<receipt revision>` and
`AYEOS_BUILD_DIRTY=0` to reproduce it. Keep the ELF for PC-to-symbol/disassembly.
Compiler unused-code warnings from legacy paths remain; no hardware result is
inferred from build success.

## Laptop test: next blocking evidence

Use a new download directory, verify `SHA256SUMS`, retain the known-good image
and established bootloader recovery procedure. With the intended phone selected:

```sh
fastboot -s "$SERIAL" getvar current-slot
fastboot -s "$SERIAL" flash boot_b studio-audio-alpha1.img
fastboot -s "$SERIAL" --set-active=b
fastboot -s "$SERIAL" reboot
```

These are the reviewed **boot_b-only** commands. The old general flash/updater
script changes other partitions and is not the procedure for this artifact.
After boot, find the new USB node; keep all four capture helpers together:

```sh
sudo python3 capture_alpha.py --device /dev/bus/usb/BBB/DDD --output "$HOME/ayeos-alpha1-capture-01"
```

Optional `--include-fdt` requests this boot's FDT. The helper creates a private
directory, saves alpha packets as they arrive and records errors. Return raw
reports/JSON privately, image SHA256, exact flash output, USB identity and a
screen photo. Expected: revision matches,dirty=false,tests7/0,Marine1024/hash
above,unexpected faults0,Board6 amplifier256/256 and PMIC33/33 complete,
I²C restored,PWRAP last_observed_idle=true. Preserve failure evidence too.

The cloud cannot perform this physical step. After this new exception path is
handset-verified, continue bounded SGEN tone/stop, PCM transport and selected
integer Phoenix stages with reference and live-signal tests. Existing supply,
attenuation,timing and failure-cleanup questions must be resolved in that work.
