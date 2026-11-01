# MarineInt for the AyeOS alpha

Upstream: <https://github.com/8b-is/aye>, revision
`479d2e9b1badddb90a1b5f0e4ad0ea116f491d5b`, `crates/marine-core` v1.0.0.
Authors from the manifest: 8b-is Team, Claude AI, Hue. Upstream offers
MIT OR Apache-2.0; this import exercises the MIT option for compatibility with
the phone's GPL-2.0-only driver adaptations. `LICENSE-MIT` contains the standard
MIT terms granted by that manifest; the repository's original Apache license is
also retained as `LICENSE-APACHE`. No upstream crate-local license file exists.
AyeOS adaptation: OpenAI GPT-6 Astra, 2026-11-01.

Original files are retained **byte-for-byte** in `reference/`, used only by the
host comparison harness (the original math file includes legacy float shims).

| Original file | SHA256 |
|---|---|
| marine_int.rs | 7523a876901e834218810986c26835f67c4e4776c82793b7c10f650a137b8801 |
| rational.rs | c94790fc5eedd75902428031db050b81a48ee396fee418e545d6f856b28d8a2f |
| internal_math.rs | 6be8f8f249e988f2480dd898af705cf54228453043ed3bbaa6410d00b0b805a3 |

Changes to the compiled subset:

- Retain `rational.rs` unchanged. Its public fields permit invalid rationals;
  the adapter only constructs the pinned valid default configuration.
- Replace MarineInt's VecDeque with the count it actually uses. No algorithm
  reads stored peak payloads. The last peak still retains index/amplitude/period.
- Extract only SAMPLE_SCALE, quantize, abs_i32 and EmaRational from internal_math.
  All compiled processing is integer-only; no allocator, unsafe or dependencies.
- Expose only default-config i16 input, reset and stats, with a **2048-sample
  epoch budget**. Exhaustion is explicit, never automatic silent re-anchoring.
  This bounds sample indexes and periods below the upstream millionth-scaled
  i32 EMA saturation threshold. It is a fixture API, not a live-stream claim.

The upstream quantization and checked-arithmetic fallback behavior are retained,
including a small nonzero decay floor and stale timing across silence. The
comparison tests cover silence, onset, extrema, jitter, silence/restart and reset.
Live integration needs an explicit discontinuity/re-anchor policy and measured
processing budgets. Matching these finite vectors establishes adaptation evidence,
not universal equivalence, audio fidelity or hardware operation.
