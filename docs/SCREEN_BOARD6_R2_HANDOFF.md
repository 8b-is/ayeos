# Board6 r2: bounded waits for idle-channel SYNC gaps

Author: OpenAI GPT-6 Astra (`openai/gpt-6-astra`), 2026-11-01.
Family context v2 / seed `8b-is-family-seed-v1`.
Base: `c77133bd38abd69b90bc3d85917a848a04ddadf6` (Board6).
Downloads: <https://g8t.is/downloads/ayeos/studio-board6/r2/>.

## Reported hardware result and limits

The laptop operator reports Board6/bcd0546 booted and returned a6400-byte
report. Both amplifier windows completed128/128 reads, with identical values
and configuration-restored=true. PWRAP completed31/33 reads: DEW5aa5,
HWCID5831, SWCID5820, and audio registers through AUDDEC_ANA_CON13.
AUDDEC_ANA_CON14 was bus_busy without a command; CON15 was not_run.

The reported pre-command RDATA for CON14 is **80600010**: FSM0, INIT_DONE22 set,
request19 clear, SYNC20 clear. This directly explains r1's single-read busy
decision under its source code. A transient shared-wrapper arbitration interval
is plausible, not proven by one status sample. The duration, other client's
identity and arbitration cause are unknown.

Raw report files arrived during implementation. Cloud verification confirmed
their supplied SHA256 hashes, CRC32, sizes, private modes and exact binary-to-JSON
agreement using the published decoder. Thus the256 amplifier/31 PMIC read results
and the SYNC-low stop are independently corroborated in the supplied artifacts;
flashing/enumeration remain operator-reported operations.

The initial transcript quoted different source/helper hashes. The operator
subsequently corrected that transcription error and supplied hashes matching
the independently re-fetched publication/manifest, reporting that the laptop's
actual files passed sha256sum verification. The private receipt preserves the
correction history. Laptop test execution remains operator-attributed. No
hardware report is embedded in tests: they model the status pattern synthetically.

## Change

`pmic_probe::wait_sync_idle` samples RDATA up to32768 times, in integer work
units. It proceeds only with INIT_DONE22, FSM0, request19 clear and SYNC20 set.
It waits **only for SYNC-low on an idle/request-clear channel**. An MMIO fault,
lost initialization, nonzero FSM (including inherited valid-clear6), or pending
request stops immediately. No pending response is acknowledged by this helper.

The helper runs before each WACS2 read command and for the final status snapshot
after a successful acknowledgement. The initial snapshot accepts idle/SYNC-low
for this bounded wait, while still rejecting pending WACS state. The second
boundary matters: r1 could also reject a transient SYNC drop in its extra sample
immediately after the acknowledgement loop had observed idle.

Persistent SYNC-low returns existing status **bus_busy**, not success; before a
command, raw value and response/ack polls stay unobserved. After acknowledgement,
an already obtained entry remains complete while the overall probe can report
busy and last_observed_idle=false. The remaining sequence stops. A failed read
still gets only a single final observation; no recovery, retry or reinterpretation
of its failure is introduced. The last observed status is preserved, including
a possible idle observation after the pre-command wait budget has expired.

Budgets per register: at most32768 pre-command samples,32768 response samples,
32768 ack-idle samples and32768 post-ack snapshot samples. Failed transactions
use one final snapshot instead of the post-ack wait. Counts are not calibrated
time. The existing five-word wire entry retains response/ack counts only; the
new pre-command/post-ack wait counts are **not serialized**. Payload/schema and
capture scripts remain unchanged; no reserved field is repurposed.

No amplifier or PMIC register values, wrapper configuration, arbitration masks,
or clock gates are changed by this fix. Existing watchdog feed and fault guard,
I2C restoration, frozen report and USB fault boundary remain in use.

## Regression and deployment

Tests cover the literal80600010 pattern at initial/pre-command/post-ack/final
boundaries; last-budget success and exhaustion; permanent low SYNC; pending FSM6,
request/FSM changes and lost INIT; and faults at every access through added wait
paths. The mock asserts every read command follows a valid ready sample, with
no extra commands/acknowledgements on failures. Existing window/transport and
fault-injection tests run with it. Actual counts/build/rebuild checks are in the
release receipt. This does not establish a successful r2 handset result.

Identify r2 by its image checksum, source receipt and **studio-board6-r2** banner.
bcdDevice stays **0546**, and all three original capture helpers are supplied.
After reviewed boot_b-only deployment, use a new private output directory:

```sh
sudo python3 capture_board6.py --device /dev/bus/usb/BBB/DDD \
  --output "$HOME/board6-r2-capture-01"
```

Optional `--include-fdt` captures this boot's private FDT. Return report JSON/bin
privately. The original Board6 capture is now verified. The next
milestone is evidence for all33 PMIC reads, followed by identifying the observed
amplifier register map/profile before selecting amplifier-value writes. Raw23,
including a reportedc1 byte at register7f, is not yet a verified chip-model ID.

See [the original Board6 handoff](SCREEN_BOARD6_HANDOFF.md) for pinned reference
sources, register layouts, protocol, build instructions and licenses.
