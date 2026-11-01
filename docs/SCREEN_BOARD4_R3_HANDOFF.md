# Board4 r3: W1C interrupt acknowledgement and EP0 completion

Author: OpenAI GPT-6 Astra (`openai/gpt-6-astra`), 2026-10-31.
Family context v2 / seed `8b-is-family-seed-v1`.
Base: `40614fb160a9a08c1b445fc3b652d06a18478185` (r2).

Downloads: <https://g8t.is/downloads/ayeos/studio-board4/r3/>.
Use **r3** for the next handset test. The image retains the Board4 EP0 descriptors
and capture protocol; the screen banner identifies `studio-board4-r3`.
The previous images remain available at their original paths.

## Reported handset evidence and its limits

The laptop operator reports that **r1** reached `Attached` with a rapidly rising
reset counter. Supplied Linux logs for `usb 3-2` show full-speed device attempts
98–101, descriptor read/all and read/8 errors `-71`, power cycling and eventual
failure to enumerate. This is stronger evidence than declarations alone: the
reported code path passed the FDT profile and initialization readbacks, and the
host detected a full-speed connection. It does not establish calibrated clocks,
PMIC rail stability, complete descriptors or successful FDT transfer.

Those observations are operator-supplied. This cloud builder has not exercised
the handset, and the errors do not by themselves prove a single physical cause.

## Confirmed software defect and correction

The MediaTek USB20 interrupt registers are **write-one-to-clear**:
INTRUSB at +0x0a is 8 bits; INTRTX at +0x02 is 16 bits. r1/r2 only read them.
The earlier mock incorrectly cleared them on read, hiding the missing writes.
That model error and the implementation defect are corrected in r3.

`acknowledge_interrupts()` samples both registers and writes back exactly the
sampled nonzero values with their native widths. Both initialization and polling
use it. Hardware barriers and fault-stop behavior remain in `UsbIo`. The reset
counter counts observed reset-latch events; coalesced hardware events are not an
exact physical reset count. A latched bit is no longer counted every idle poll.

Reference evidence:

- Pinned 4.19 reference `8745b22c4bb0d208914d62cfd5b8c6f7a7bdf991`,
  `drivers/misc/mediatek/usb20/mt6768/usb20.c:1025–1045`, explicitly labels W1C
  and writes INTRTX/INTRUSB after reading them.
- [Pinned 6.6 reference](https://github.com/OnePlusOSS/android_kernel_modules_and_devicetree_oneplus_mt6991/tree/76d6e1e5b8deb0bcd00355d7a9e68638a501d8bc/kernel/kernel_device_modules-6.6),
  `drivers/misc/mediatek/usb/usb20/musb_core.c:4215–4230`, does the same.
- The 6.6 `musb_gadget_ep0.c:811–818,874–884` checks DATAEND and writes FADDR
  in the interrupt-driven STATUSIN state. This supports combining EP0 interrupt
  evidence with CSR readiness; it does **not** establish the supplied assertion
  that DATAEND is universally write-only and immediately reads zero.

## Transfer ordering

The polled state machine now requires an EP0 interrupt event after arming a reply.
Zero CSR bits alone never commit SET_ADDRESS or SET_CONFIGURATION. A sampled
completion is retained while DATAEND/TXPKTRDY remains busy, so acknowledging the
interrupt does not lose the pending completion. Reset, SETUPEND and stall abort
pending address/configuration work.

A SETUP can arrive between the initial interrupt snapshot and the FIFO read.
Before arming its reply, r3 drains the SETUP's remaining EP0 latch, preventing
it from being misused as that new reply's completion. Back-to-back status/SETUP
and visible zero-length OUT status are covered by host tests. Read replies also
remain active until event-driven progress; the protocol still uses one 64-byte
packet per numbered chunk and no bulk endpoints.

## Status screen scheduling

The previous `flush_dcache_range()` implementation is a single `dsb sy`; its
address/length arguments are unused. It does **not** sweep a 4.7 MB framebuffer.
Nevertheless, redrawing an entire status row on every false reset wastes work.

r3 services USB before display scheduling and writes at most **one 9×16 cell
(144 pixels)** in a quiet polling iteration. Active transfers pause rendering.
A row is a stable padded snapshot, with address, configuration, reset count,
fault flag, request count and stall count. It may lag the live state.

The scheduler requires 256 quiet polls and allows a new snapshot only after
65,536 polling iterations since the previous snapshot began. These are bounded
work budgets, **not milliseconds or measured latency**. Each cell is followed
by a memory barrier; the attached service loop performs no range flush or whole
row redraw. Initial full-screen rendering remains before soft connection.
The literal glyph coordinates are x=20+9×column, y=68+18×10, height 16.

## Build, checks, and next handset test

Build from `baremetal/` using Rust 1.99.1:
`cargo build --release --locked --features usb-report`.
The source archive contains the complete crate, new `usb_status.rs`, tests,
packager, capture script and license notices. Adjacent `receipt.json` records
the exact source revision, checks and artifact hashes.

Host tests now model persistent W1C latches and both busy and zero/strobe-style
DATAEND readback. They cover one reset across 10,000 polls, a later reset,
startup draining, preservation of unsampled bits, no premature address/config
commit, retained completion while busy, abort/reset cancellation, late SETUP
latches, complete simulated enumeration/chunk readback, and bounded paused
status rendering. These tests are not a substitute for electrical validation.

Deploy with the previously reviewed local procedure, verify the r3 banner, and
use the unchanged `capture_board4.py` with the exact `/dev/bus/usb/BBB/DDD` node.
See [the original handoff](SCREEN_BOARD4_HANDOFF.md) for capture commands and
private-file handling. Never use the old OTA/reboot command for this diagnostic.

Return either the verified private FDT capture, or one full r3 screen photo and
the fresh Linux USB log. Note address/configuration, requests/stalls and whether
the reset count settles between actual host retries. A status row that pauses
during an unfinished transfer is expected. Successful enumeration and transfer
still require handset verification; audio/FM/Wi-Fi and owner-service work remain
subsequent steps.
