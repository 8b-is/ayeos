# Screen-first diagnostic handoff

Author: OpenAI GPT-6 Astra (`openai/gpt-6-astra`), 2026-10-31.
Family context v2 / seed `8b-is-family-seed-v1`.
Base: `67401149870e825353a9c7d62a94ecedd4ba8085`.
Branch: `agent/phone-bringup-20261031`.

## Artifact and evidence

Server artifact: `/data/os-verification-20261031/screen-board1/studio-board1.img`

- Development banner: `AYEOS 5.4.0-studio-board1 / SCREEN DIAGNOSTIC`
- Image size: 24576 bytes.
- Image SHA-256: `cc2440c978ca34f8af8e55c1a7d150f852e304780bb58b35e5c1436b198724b0`
- Raw kernel size: 41232 bytes.
- Raw kernel SHA-256: `66db3252383c287652ae1e08d1e004188e5284cad142f752acba45e62bea7be9`
- Receipt is alongside the image; hashes are not publisher authentication.

Validated: AArch64 locked release build with `board-report`; seven host tests
(malformed/truncated FDT, borrowed properties, text bounds, selected records and
omission accounting); packaging round-trip, Android-v4 header fields, zero
padding, deterministic output, invalid-input rejection; `git diff --check`.
Build emits unused inherited helper and unreachable Studio-path warnings.
No physical phone boot, screenshot, publication or flashing occurred here.

## Local operator next action

Keep the current phone build until the artifact has been transferred and its
size/hash checked. Report the exact proposed deployment procedure before use:
the historical updater erases init_boot slots and is unsuitable. No assumption
is made that the alternate boot slot is a stock recovery image, or that this
bootloader supports temporary RAM boot.

After a separately coordinated boot, photograph the entire stationary page.
Return the banner, x0 pointer, header/parser result, and footer; if it stops at
a Stage line, photograph that too. The diagnostic holds with watchdog servicing;
it has no button paging or USB updater loop. Resources are declarations, not
confirmed hardware readiness. The first 32 selected properties may omit later
nodes; subsequent pages can be selected based on the first capture.

The local report identifies the persistent ttyACM device as an Espressif device,
not the phone. AyeOS USB enumeration is unobserved. USB PHY teardown and polling
latency explanations remain hypotheses. The display and boot-memory assumptions
are inherited; skipping a faulting instruction does not prove a read succeeded.
Do not infer stock partition integrity or FDT availability from old log claims.
