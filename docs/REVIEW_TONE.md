# Offline tone1 evidence review

`python3 scripts/review_tone.py PRIVATE_CAPTURE_DIR --expected-revision FULL_RELEASE_REVISION`

This command **only reads existing files**. It does not open USB, flash, reset,
reboot, publish, repair evidence, or write a review into the capture directory.
JSON goes to stdout and a short summary to stderr. Exit 0 means
`register_checks_passed`; exit 1 means invalid/incomplete evidence or failed
register acceptance. Invalid CLI arguments exit 2.

For the published audio-tone1/9277657 image, the full expected revision is
`927765706c5c9a2a7d302c4e679e25762888057a`, not its main-branch merge revision.
Supply the intended **image's** revision explicitly; the reviewer does not infer
it from the checkout or trust a reported revision as the acceptance target.

## Required input and verdicts

Required files are `tone-report.bin` (6400 bytes), `alpha-report.bin` (1024 bytes),
`board6-report.bin` (6400 bytes), and `SHA256SUMS`. The manifest must name every
other file exactly once with its SHA256 and two spaces before a safe local
basename. No traversal, duplicate entries, manifest self-reference, symlinks,
subdirectories, or nonregular files are accepted. A missing or mismatching
manifest is an evidence error; do not generate a replacement to hide it.

Original raw bytes are decoded afresh using the capture decoders. The reviewer
checks revision/dirty agreement and all tone/alpha/Board6/FDT CRC links. Saved
report JSON, if supplied, must match the fresh decode; it never overrides raw
bytes. If tone, alpha, or Board6 transport info/chunks are supplied, that entire
packet group must be present, valid and reconstruct the raw report. Tone USB
descriptors, when present, must match exactly. Raw-only bundles can be reviewed,
but do not establish USB transport provenance. The existing capture helper does
not save Board6 transport packets; their absence is not itself an error.

Optional `board6-fdt.dtb` is checked for magic, declared size, bounded length and
CRC agreement, not a complete device-tree semantic revalidation. Do not request
or expose a private FDT unnecessarily. Output excludes register dumps, FDT
content and raw capture-error text. A `capture-error.txt` marker always blocks
acceptance; the output reports its presence and, when readable, size/hash.

Statuses:

- `evidence_invalid`: malformed, incomplete, contradictory or corrupt evidence,
  a capture-error marker, unsafe files or a manifest problem.
- `register_checks_not_passed`: internally consistent evidence that fails the
  explicitly requested release/register acceptance policy. Failed/preflight
  reports are retained as diagnostic evidence, not reinterpreted as success.
- `register_checks_passed`: expected clean revision; alpha tests 7/0 and no
  unexpected faults; every pre-tone PA/PMIC read complete, matched profiles,
  restored I2C configuration and observed PWRAP idle; tone complete, all final
  Off predicates observed and controllers reported restored.

**Every status retains `acoustic_status=operator_observation_required`.**
Neither exit 0, CRC/SHA checks, the journal nor QEMU establishes audibility,
frequency, loudness, physical shutdown, pop behavior, image provenance or safe
hardware. Self-consistent fabricated evidence can pass; checksums are not
signatures. Require separate operator observations and deployment provenance.
Board6 remains a **pre-tone** snapshot, not post-tone register evidence.

## Decoder invariants and compatibility

The stricter `capture_tone.py` validates the successful operation sequence from
`baremetal/src/tone.rs`: preflight reads/gates, startup, profiles, unmute, cleanup
and the 17 final observations. It checks register widths, kind-specific fields,
readback/error consistency, successful delays, and complete-result timestamps.
This is a pinned tone1-v1 algorithm check. A future algorithm change needs an
explicit decoder/policy update even if its wire shape or USB identity is reused.
Presentation-only firmware changes do not affect this sequence.

The six concrete gaps addressed are:

1. A complete claim containing only the 17 final reads is rejected.
2. Complete event ticks outside the report interval or inconsistent with phase,
   ordering, delays or the first stop attempts are rejected.
3. Oversized PA/PMIC observations (for example PA `0x100` masking to Off) fail.
4. Any 40-hex revision remains syntactically valid diagnostic evidence, but the
   reviewer refuses acceptance unless it matches the explicitly supplied target.
5. Post-arm `restored=false` must include at least one cleanup error.
6. Preflight rejection cannot claim cleanup errors, cleanup/end timing or phases.

Failure journals deliberately do **not** require successful timing or a full
startup sequence: timers can stop, become unavailable, or go backwards. Failed
post-arm work must still include all independent final observation attempts.
A preflight restore failure with zero cleanup errors is legitimate in the
producer and remains decodable. Register restoration is a firmware-reported
flag, not independently demonstrated by a restoration register journal.

## Resource/privacy limits

At most 256 directory entries, 64 KiB manifest, 2 MiB per other file, and 16 MiB
aggregate file content are admitted. Raw reports have exact smaller size limits.
All arithmetic is integer. Unsafe entries are rejected without following them;
opened descriptors are rechecked, bounded, and checked for size/timestamp
changes during reading. Use trusted ancestor directories and an owner-controlled,
quiescent evidence directory: this is not a filesystem snapshot or a defense
against every concurrent hostile replacement. Keep the original private bundle
unchanged and direct review output elsewhere if retention is wanted.

## Validation

From the repository root with the installed Rust toolchain on PATH:

```sh
PATH=/home/hue/.cargo/bin:$PATH bash scripts/check_tone.sh
```

This exercises the real successful Rust wire fixture, strict decoder negatives,
and a temporary export of over 500 real producer failure journals using the
existing Rust bus model. It also runs the existing Rust and two exception QEMU
checks. No producer files are edited by the export test.

Using the `tone-wire.bin` path printed by that run:

```sh
PATH=/home/hue/.cargo/bin:$PATH AYEOS_TONE_TEST_DUMP=/path/to/tone-wire.bin \
  python3 -m unittest discover -s scripts -p 'test_capture*.py' -v
AYEOS_TONE_TEST_DUMP=/path/to/tone-wire.bin \
  python3 -m unittest discover -s scripts -p 'test_review_tone.py' -v
```

Without `AYEOS_TONE_TEST_DUMP`, Rust-dependent Python cases are explicitly
skipped. Reviewer positives use synthetic alpha/Board6 fixtures; they are not
handset evidence. Tests cover acceptance failures, corruption after manifest
rehashing, partial reports, transport reconstruction, manifests, unsafe files,
resource bounds, CLI exits and preservation of evidence bytes/mtime.
