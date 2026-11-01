//! Integer-only math helpers for the Marine Algorithm.
//!
//! These are the small primitives the integer implementation in
//! [`crate::marine_int`] needs: sample scaling, EMA, and Rational helpers
//! that don't fit on `Rational` itself.
//!
//! ## Sample scaling
//!
//! The integer pipeline operates on samples in `i32` form. The natural
//! encoding for audio is the original PCM range `[-32768, 32767]`, which
//! is exactly representable in `i16` and trivially in `i32`. The legacy
//! f32 shim multiplies by `SAMPLE_SCALE` (32768) and rounds half-away-from
//! -zero so the conversion is symmetric for positive and negative samples.
//!
//! Operating in `i32` (rather than `i16`) gives us headroom for callers
//! that pass slightly out-of-range f32 (e.g. clipped audio in `[-1.5, 1.5]`)
//! without overflow during scaling.

use crate::rational::Rational;

/// Sample scale factor. `f32 in [-1.0, 1.0]` maps to `i32 in
/// [-SAMPLE_SCALE, SAMPLE_SCALE]`. Chosen as `1 << 15` so we match the
/// PCM `i16` convention (`32768`) — i.e. an `i16` sample becomes the same
/// integer in our pipeline.
pub const SAMPLE_SCALE: i32 = 32768;

/// Nominal carrier-rate tick at the 73 Hz substrate layer.
///
/// `1e9 / 73 ≈ 13_698_630 ns`. This is the hardware-bound layer of the
/// nested-heartbeat topology: per-beat phase telemetry, Marine peak
/// detection, kernel timer / PIC EOI cadence, and the Custodian's
/// κ-bound on phase acceleration are all measured against this period.
pub const CARRIER_TICK_NS: u64 = 13_698_630;

/// Nominal envelope-rate tick at the 0.73 Hz cognitive layer.
///
/// `1e9 / 0.73 ≈ 1_369_863_014 ns`. This is the macro-state layer of
/// the nested heartbeat: WAL consolidation pacing, NRAM/REM sleep
/// transitions, associative-resonance window length, and the L3
/// working-grid "breathing" cycle are all measured against this period.
///
/// Exact integer floor of `1e9 / 0.73`. Existing mem8-core consumers
/// (`Wave::advance_one_tick`, `CONSCIOUSNESS_PERIOD_NS`) should pin to
/// this constant rather than carrying their own literal so the two
/// layers can't drift.
///
/// Ratio: `ENVELOPE_TICK_NS ≈ 100 * CARRIER_TICK_NS`, with a 14-ns
/// integer-rounding mismatch (1_369_863_000 vs 1_369_863_014) that's
/// well below any tolerance budget downstream.
pub const ENVELOPE_TICK_NS: u64 = 1_369_863_014;

/// Round-half-away-from-zero conversion of `f32 sample` (typically in
/// `[-1.0, 1.0]`) to scaled `i32`. Used by the deprecated f32 boundary
/// shim. Out-of-range inputs are clamped at `i32` extents.
pub fn sample_f32_to_i32(s: f32) -> i32 {
    let scaled = s * SAMPLE_SCALE as f32;
    let rounded = if scaled >= 0.0 {
        (scaled + 0.5) as i64
    } else {
        (scaled - 0.5) as i64
    };
    rounded.clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

/// Convert a scaled `i32` sample back to an `f32` in `[-1.0, 1.0]`. Lossy
/// (just an i32→f32 cast scaled down), used only on the way out of the
/// deprecated f32 API.
pub fn sample_i32_to_f32(s: i32) -> f32 {
    s as f32 / SAMPLE_SCALE as f32
}

/// Convert a `Rational` in `[0, 1]` (e.g. the salience score) to an
/// `f32` for the deprecated f32 API. We compute `num / den` directly
/// at the boundary — there's no float math anywhere inside the
/// algorithm.
pub fn rational_to_f32(r: Rational) -> f32 {
    r.num as f32 / r.den as f32
}

/// Convert an `f32` in roughly `[0, 1]` to a `Rational`. Used only for
/// the f32 `MarineConfig` shim. Precision is `1 / DENOM_F32_SHIM`.
pub fn f32_to_rational(x: f32) -> Rational {
    // Use a fixed denominator of 1_000_000 for the f32 shim. This is
    // far more precision than any of the existing config defaults
    // (0.3, 0.01, etc.) actually need, and it's well inside i32 range.
    const DENOM: i32 = 1_000_000;
    let num = (x * DENOM as f32).round_i32_via_half_away();
    // Reduce — Rational::new will return None if den == 0, but DENOM is
    // a positive constant. If `num` is i32::MIN we still get a valid
    // Rational; the reduction inside `new` handles it.
    Rational::new(num, DENOM as u32).expect("DENOM is non-zero")
}

/// Tiny extension trait so we can call `.round_i32_via_half_away()` on
/// an `f32` in no_std without pulling in libm. f32 already has the bit
/// operations we need at the language level; only `round()` lives in
/// libm. This implements round-half-away-from-zero by hand.
trait RoundI32 {
    fn round_i32_via_half_away(self) -> i32;
}

impl RoundI32 for f32 {
    fn round_i32_via_half_away(self) -> i32 {
        let v = if self >= 0.0 { self + 0.5 } else { self - 0.5 };
        // Saturating cast: f32 → i32 may overflow; clamp via i64.
        (v as i64).clamp(i32::MIN as i64, i32::MAX as i64) as i32
    }
}

/// Fixed-point quantization denominator for state that is repeatedly
/// multiplied (`current_salience`, EMA value). Each iteration of e.g.
/// `current_salience *= decay_rate` would otherwise grow the denominator
/// geometrically and quickly exceed `u32::MAX`, at which point
/// `Rational::checked_mul` returns `None` and we lose all state.
///
/// We round to this denominator after every such mutation. `1_000_000`
/// gives us roughly 6 decimal digits of precision on values in `[0, 1]`
/// — far more than the legacy `f32` path provided and well within `i32`
/// numerator range.
pub const QUANT_DENOM: u32 = 1_000_000;

/// Round a `Rational` to a fixed denominator [`QUANT_DENOM`]. The
/// result is the nearest `k / QUANT_DENOM` to the input. Use this to
/// stop denominator-explosion in repeated multiplication loops.
///
/// Behaviour on overflow: the multiplication `r.num * QUANT_DENOM` is
/// done in `i64`, then divided by `r.den` and rounded. The intermediate
/// is safely in `i64` for any input fitting in `Rational`. The final
/// numerator is clamped to `i32`. In practice all our quantized
/// rationals stay in `[0, 1]` so this never clips.
pub fn quantize(r: Rational) -> Rational {
    if r.is_zero() {
        return Rational::ZERO;
    }
    let num_scaled = (r.num as i64) * (QUANT_DENOM as i64);
    let den = r.den as i64;
    // Round half-away-from-zero. We compare 2*|rem| against den to
    // avoid even/odd half-step edge cases. den > 0 by invariant.
    let q = num_scaled / den;
    let rem = num_scaled % den;
    let rounded: i64 = if rem == 0 {
        q
    } else if rem.wrapping_abs() * 2 >= den {
        if num_scaled >= 0 {
            q + 1
        } else {
            q - 1
        }
    } else {
        q
    };
    let num_i32: i32 = rounded.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
    Rational::new(num_i32, QUANT_DENOM).unwrap_or(Rational::ZERO)
}

/// Multiply two `Rational`s and re-quantize to denominator [`QUANT_DENOM`],
/// sidestepping the `u32` denominator ceiling of [`Rational::checked_mul`].
///
/// **Why this exists:** values coming out of [`quantize`] have
/// `den == QUANT_DENOM = 1_000_000`. The product of two such values via
/// `Rational::checked_mul` would carry `den = 1e12`, which does not fit in
/// `u32`, so `checked_mul` returns `None` even though the *quantized*
/// result fits trivially. This helper computes `(a.num * b.num * QUANT_DENOM)
/// / (a.den * b.den)` directly in `i64`, rounds half-away-from-zero, and
/// re-normalises to `QUANT_DENOM`.
///
/// On any overflow the helper returns [`Rational::ZERO`], matching the
/// "decayed below precision floor" semantics callers rely on (e.g. the
/// Taylor `exp_neg` recurrence in `mem8-core`).
pub fn mul_quantized(a: Rational, b: Rational) -> Rational {
    if a.is_zero() || b.is_zero() {
        return Rational::ZERO;
    }
    let an = a.num as i64;
    let ad = a.den as i64;
    let bn = b.num as i64;
    let bd = b.den as i64;
    let num = match an.checked_mul(bn) {
        Some(v) => v,
        None => return Rational::ZERO,
    };
    let den = match ad.checked_mul(bd) {
        Some(v) => v,
        None => return Rational::ZERO,
    };
    let scaled = match num.checked_mul(QUANT_DENOM as i64) {
        Some(v) => v,
        None => return Rational::ZERO,
    };
    let q = scaled / den;
    let rem = scaled % den;
    let rounded = if rem == 0 {
        q
    } else if rem.wrapping_abs() * 2 >= den {
        if scaled >= 0 {
            q + 1
        } else {
            q - 1
        }
    } else {
        q
    };
    let num_i32 = rounded.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
    Rational::new(num_i32, QUANT_DENOM).unwrap_or(Rational::ZERO)
}

/// Square a `Rational` and re-quantize to denominator [`QUANT_DENOM`].
///
/// Equivalent to `mul_quantized(r, r)` but avoids the second `checked_mul`
/// of `r.num * r.num` going through `i64::checked_mul` twice. The
/// numerator squared is at most `~1e12` for `|r| <= 1` (the practical
/// range of quantized state), so `num² * QUANT_DENOM` fits in `i64`.
///
/// On overflow returns [`Rational::ZERO`] — the same precision-floor
/// convention as [`mul_quantized`].
pub fn square_quantized(r: Rational) -> Rational {
    if r.is_zero() {
        return Rational::ZERO;
    }
    let num = r.num as i64;
    let den = r.den as i64;
    let num_sq = match num.checked_mul(num) {
        Some(v) => v,
        None => return Rational::ZERO,
    };
    let den_sq = match den.checked_mul(den) {
        Some(v) => v,
        None => return Rational::ZERO,
    };
    let scaled = match num_sq.checked_mul(QUANT_DENOM as i64) {
        Some(v) => v,
        None => return Rational::ZERO,
    };
    let q = scaled / den_sq;
    let rem = scaled % den_sq;
    let rounded = if rem == 0 {
        q
    } else if rem.wrapping_abs() * 2 >= den_sq {
        q + 1
    } else {
        q
    };
    let num_i32 = rounded.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
    Rational::new(num_i32, QUANT_DENOM).unwrap_or(Rational::ZERO)
}

// ===========================================================================
// Wave 2b: integer transcendentals (sqrt, sin, cos).
// ===========================================================================
//
// These primitives are the integer counterparts of `f64::sqrt`, `f64::sin`,
// and `f64::cos`. They live in marine-core because marine-core is the
// integer-math foundation: zero external deps, `#![no_std]`, callable from
// kernels and bare-metal targets.
//
// All three are pure integer — no `f32`, no `f64`, no `libm`, no
// `num-traits::Float`. The Rational-level wrappers `sqrt_rational`,
// `sin_rational`, `cos_rational`, and `sin_cos_rational` round results back
// to denominator [`QUANT_DENOM`] so callers' fixed-point invariants hold.

/// Integer square root of a `u64`: largest `r` such that `r * r <= x`.
///
/// Algorithm: Newton-Raphson with bit-length-derived initial guess.
/// `next = (curr + x / curr) / 2`, iterating until convergence. Converges
/// quadratically; for `u64` inputs at most ~6 iterations.
///
/// Initial guess strategy:
/// - For `x == 0` return 0.
/// - Otherwise: `bits = 64 - x.leading_zeros()`, take
///   `r0 = 1 << ((bits + 1) / 2)`. This overestimates `sqrt(x)` by at most
///   a factor of √2, so Newton's quadratic convergence kicks in immediately
///   and the iteration never undershoots (Newton on a convex function from
///   above is monotone decreasing).
///
/// Termination: when `next >= curr`. The Newton sequence is strictly
/// decreasing until it brackets the true root, then oscillates by ±1; we
/// stop when monotonicity breaks and return the smaller of `curr` and
/// `next`. Final adjustment: drop by 1 if `r * r > x` (handles the
/// off-by-one at very small inputs).
pub fn isqrt(x: u64) -> u32 {
    if x == 0 {
        return 0;
    }
    if x < 4 {
        return 1;
    }
    // Initial guess: 2^((bits+1)/2), an overestimate of sqrt(x).
    let bits = 64 - x.leading_zeros();
    let shift = bits.div_ceil(2);
    let mut r: u64 = 1u64 << shift;

    // Newton iteration. Cap at 64 to be unconditionally bounded; in
    // practice we converge in 6 steps for any u64.
    for _ in 0..64 {
        let next = (r + x / r) / 2;
        if next >= r {
            break;
        }
        r = next;
    }
    // Verify and clamp. r*r may overflow u64 for huge x (sqrt of u64::MAX
    // is ~4.29e9, fits in u32). Use u128 for the verification mul.
    while (r as u128) * (r as u128) > x as u128 {
        r -= 1;
    }
    // Cap at u32::MAX (sqrt(u64::MAX) < u32::MAX).
    if r > u32::MAX as u64 {
        u32::MAX
    } else {
        r as u32
    }
}

/// Binary logarithm of a positive `Rational`, in **Q5 fixed point** —
/// returns `floor(32 * log2(num/den))` as a signed offset in 1/32-octave
/// units. This is the integer replacement for the f64
/// `(freq / 0.73).log2() * 32` frequency binning in
/// `Coordinate::from_wave`.
///
/// Pure integer: normalize the ratio into `[1, 2)` to capture the integer
/// octave, then extract 5 fractional bits by repeated squaring (5 bits ==
/// 1/32-octave resolution, matching the 32-grid-units/octave X axis
/// exactly). No transcendental, no float, no `libm`.
///
/// **Undefined for non-positive input** (`num <= 0`) and `den == 0`:
/// returns `0` so callers can guard sub-threshold / zero frequency without
/// a panic (mirrors the f64 path's `log2(0) = -inf` clamped to the axis
/// floor).
pub fn log2_q5_rational(r: Rational) -> i32 {
    if r.num <= 0 || r.den == 0 {
        return 0;
    }
    let mut num: u128 = r.num as u128;
    let mut den: u128 = r.den as u128;

    // Normalize ratio = num/den into [1, 2); `e` accumulates the integer octave.
    let mut e: i32 = 0;
    while num >= den << 1 {
        den <<= 1;
        e += 1;
    }
    while num < den {
        num <<= 1;
        e -= 1;
    }

    // Extract 5 fractional bits via repeated squaring of the [1, 2) mantissa.
    let mut frac: i32 = 0;
    for _ in 0..5 {
        // Keep magnitudes bounded so `num * num` cannot overflow u128.
        let bits = 128 - num.leading_zeros();
        if bits > 60 {
            let s = bits - 60;
            num >>= s;
            den >>= s;
            if den == 0 {
                den = 1;
            }
        }
        num *= num;
        den *= den;
        frac <<= 1;
        if num >= den << 1 {
            // mantissa² >= 2 → next fractional bit is 1; halve back into [1, 2).
            den <<= 1;
            frac |= 1;
        }
    }

    e * 32 + frac
}

/// Square root of a non-negative `Rational`, returned as a `Rational`
/// quantized to denominator [`QUANT_DENOM`].
///
/// Computes `sqrt(num/den)` as `sqrt(num * QUANT_DENOM² / den) / QUANT_DENOM`.
/// The internal `isqrt` runs on a `u64`, so we must keep `num * QUANT_DENOM² /
/// den` inside u64 — that holds for any `r` with `|num| < ~10^6` and
/// `den < ~10^6`, which covers every quantized Rational in this codebase.
///
/// Returns [`Rational::ZERO`] for negative inputs (sqrt of negative is
/// undefined; the caller's domain is amplitudes / squared distances which
/// are always non-negative). On overflow inside the intermediate
/// multiplication, also returns ZERO — the same precision-floor convention
/// as `mul_quantized`.
///
/// Precision: at most one ULP error at scale [`QUANT_DENOM`] from `isqrt`,
/// i.e. ~1e-6 absolute error on the result.
pub fn sqrt_rational(r: Rational) -> Rational {
    if r.num <= 0 {
        return Rational::ZERO;
    }
    // We compute floor(sqrt(num * QUANT_DENOM² / den)) and treat that as a
    // numerator over QUANT_DENOM. Round to nearest by checking the half-step.
    let num = r.num as u128;
    let den = r.den as u128;
    let qd = QUANT_DENOM as u128;
    // scaled = num * QUANT_DENOM * QUANT_DENOM / den. We want `sqrt(scaled)`
    // as a numerator over QUANT_DENOM.
    let scaled = match num.checked_mul(qd).and_then(|v| v.checked_mul(qd)) {
        Some(v) => v / den,
        None => return Rational::ZERO,
    };
    if scaled > u64::MAX as u128 {
        return Rational::ZERO;
    }
    let s = scaled as u64;
    let root = isqrt(s) as u64;
    // Round half-up: if (root+1)^2 - s <= s - root^2, pick root+1.
    let r2 = root * root;
    let r1_sq = (root + 1) * (root + 1);
    let chosen = if r1_sq <= s + ((root + 1) - root) {
        // shouldn't reach; isqrt is monotone. Keep as floor.
        root
    } else if (s - r2) > (r1_sq - s).saturating_sub(1) {
        root + 1
    } else {
        root
    };
    let num_i32 = if chosen > i32::MAX as u64 {
        i32::MAX
    } else {
        chosen as i32
    };
    Rational::new(num_i32, QUANT_DENOM).unwrap_or(Rational::ZERO)
}

// ---- CORDIC sin/cos in BAM units ------------------------------------------
//
// BAM ("binary angle measurement") encodes an angle as an i32 where the full
// range `[i32::MIN, i32::MAX+1)` maps linearly to `[-π, π)`. Equivalently:
// `1 << 30` BAM units == π/2. The CORDIC iteration uses BAM throughout, so
// the only place we touch `PI_RATIONAL` is the API boundary `radians ↔ BAM`.

/// π as a `Rational`. `355/113` is the best small-denominator approximant
/// of π — absolute error ~2.7e-7, well below our quantization floor of
/// 1e-6. Used only at the radians-to-BAM boundary.
pub const PI_RATIONAL: Rational = Rational { num: 355, den: 113 };

/// CORDIC fixed-point scale. Cos/sin internal computations use signed
/// fixed-point with this scale. `1 << 14 = 16384` leaves 16 bits of i32
/// headroom for the iteration overshoot.
const CORDIC_SCALE: i32 = 1 << 14;

/// CORDIC iteration count. 20 iterations on this scale give ~14 bits of
/// precision in the final cos/sin, equivalent to ~6e-5 absolute error —
/// well inside the 1e-4 target spelled out in the Wave 2b brief.
const CORDIC_N: usize = 20;

/// CORDIC gain compensation: the iteration multiplies the input vector
/// magnitude by `1/K` where `K = prod_{i=0..N} 1 / sqrt(1 + 2^{-2i})`.
/// To make the output land at `(CORDIC_SCALE * cos, CORDIC_SCALE * sin)`,
/// we seed `x_0 = round(CORDIC_SCALE * K)` so the magnitude growth ends
/// exactly at `CORDIC_SCALE`.
///
/// `K_20 = 0.6072529350088813...` → `CORDIC_SCALE * K =
/// 16384 * 0.6072529 ≈ 9948.196`, rounds to 9948.
const CORDIC_K_SEED: i32 = 9948;

/// Pre-computed `atan(2^-i)` in BAM units, for `i = 0..CORDIC_N`.
///
/// Generated from `round(atan(2.0f64.powi(-i)) * (1u64 << 32) as f64 /
/// (2.0 * core::f64::consts::PI))` at indices `0..20`. These are exact
/// integers at compile time — no float arithmetic at runtime.
///
/// Each entry is in `[0, 2^30]` (since `atan` of a positive value is
/// `< π/2 = 2^30` BAM units), so `i32` is plenty.
const CORDIC_ATAN_TABLE: [i32; CORDIC_N] = [
    536_870_912, // atan(1)         = π/4         BAM 2^30
    316_933_406, // atan(1/2)       ≈ 0.4636 rad
    167_458_907, // atan(1/4)       ≈ 0.2450 rad
    84_859_274,  // atan(1/8)       ≈ 0.1244 rad
    42_572_511,  // atan(1/16)      ≈ 0.0624 rad
    21_315_111,  // atan(1/32)      ≈ 0.0312 rad
    10_660_853,  // atan(1/64)      ≈ 0.0156 rad
    5_330_715,   // atan(1/128)
    2_665_409,   // atan(1/256)
    1_332_707,   // atan(1/512)
    666_354,     // atan(1/1024)
    333_177,     // atan(1/2048)
    166_589,     // atan(1/4096)
    83_294,      // atan(1/8192)
    41_647,      // atan(1/16384)
    20_824,      // atan(1/32768)
    10_412,      // atan(1/65536)
    5_206,       // atan(1/131072)
    2_603,       // atan(1/262144)
    1_301,       // atan(1/524288)
];

/// Compute `(sin, cos)` at scale [`CORDIC_SCALE`] for a BAM angle. Pure
/// integer. Returns `(sin_scaled, cos_scaled)`, each in approximately
/// `[-CORDIC_SCALE, CORDIC_SCALE]`.
///
/// BAM semantics: the input is interpreted modulo `2^32` (i.e. one full
/// turn). Quadrant reduction maps the angle into `[-π/2, π/2]` (i.e.
/// `[-2^30, 2^30]` in BAM) before running CORDIC, then flips the result
/// signs as needed.
fn cordic_sin_cos_bam(bam: i32) -> (i32, i32) {
    // ---- Quadrant reduction ----
    //
    // We need the residual in [-π/2, π/2] (BAM in [-2^30, 2^30]). If the
    // original lies in Q2 (π/2..π) or Q3 (-π..-π/2), subtract/add π and
    // remember to negate both sin and cos at the end (since
    // sin(θ+π) = -sin(θ), cos(θ+π) = -cos(θ)).
    const HALF_PI_BAM: i32 = 1 << 30;
    // π in BAM units equals `1 << 31` which is `i32::MIN` (mod 2^32) — the
    // signed encoding of -π. Adding `i32::MIN` is the same as adding π
    // modulo a full turn, which is what we want for the Q2/Q3 reduction.
    let mut a = bam;
    let mut flip = false;
    if a > HALF_PI_BAM {
        a = a.wrapping_add(i32::MIN); // shift by π
        flip = true;
    } else if a < -HALF_PI_BAM {
        a = a.wrapping_add(i32::MIN);
        flip = true;
    }

    // ---- CORDIC rotation ----
    let mut x: i32 = CORDIC_K_SEED;
    let mut y: i32 = 0;
    let mut z: i32 = a;
    for (i, &atan) in CORDIC_ATAN_TABLE.iter().enumerate() {
        let sigma = if z >= 0 { 1 } else { -1 };
        // x_{i+1} = x - σ * y * 2^-i
        // y_{i+1} = y + σ * x * 2^-i
        // z_{i+1} = z - σ * atan(2^-i)
        let dx = (y >> i) * sigma;
        let dy = (x >> i) * sigma;
        x -= dx;
        y += dy;
        z -= sigma * atan;
    }
    if flip {
        (-y, -x)
    } else {
        (y, x)
    }
}

/// Convert a `Rational` angle in radians to BAM units (`i32`).
///
/// The full circle `2π` corresponds to `2^32`, so `1 rad ≈ 2^32 / (2π)`
/// BAM units. We compute `angle.num * (2^32 / (2π)) / angle.den` using
/// `PI_RATIONAL` for π, all in `i64` to avoid overflow.
fn radians_to_bam(angle: Rational) -> i32 {
    // bam = angle * 2^32 / (2π) = angle.num * 2^32 / (angle.den * 2 * π).
    // Using PI_RATIONAL = 355/113 → 2π ≈ 710/113.
    // So bam = angle.num * 113 * 2^32 / (angle.den * 710).
    //
    // 113 * 2^32 = 485_331_036_672, fits in u64.
    let twoturn: i128 = (1_i128 << 32) * (PI_RATIONAL.den as i128); // 2^32 * den_pi
    let num = (angle.num as i128) * twoturn;
    let den = (angle.den as i128) * 2 * (PI_RATIONAL.num as i128);
    let q = num / den;
    // Wrap into i32 range — BAM is modular by design.
    (q & 0xFFFF_FFFF) as i32
}

/// Sine of a `Rational` angle in radians, returned as a `Rational`
/// quantized to denominator [`QUANT_DENOM`].
///
/// Precision: better than 1e-4 absolute error across the full range.
pub fn sin_rational(angle: Rational) -> Rational {
    let bam = radians_to_bam(angle);
    let (s, _c) = cordic_sin_cos_bam(bam);
    scaled_i32_to_rational(s)
}

/// Cosine of a `Rational` angle in radians, returned as a `Rational`
/// quantized to denominator [`QUANT_DENOM`].
pub fn cos_rational(angle: Rational) -> Rational {
    let bam = radians_to_bam(angle);
    let (_s, c) = cordic_sin_cos_bam(bam);
    scaled_i32_to_rational(c)
}

/// Fused `(sin, cos)` of a `Rational` angle in radians.
///
/// One CORDIC pass computes both — use this from `ComplexRational::from_polar`
/// and any caller that needs both. Each output is a `Rational` quantized to
/// [`QUANT_DENOM`].
pub fn sin_cos_rational(angle: Rational) -> (Rational, Rational) {
    let bam = radians_to_bam(angle);
    let (s, c) = cordic_sin_cos_bam(bam);
    (scaled_i32_to_rational(s), scaled_i32_to_rational(c))
}

/// Convert a fixed-point i32 at CORDIC_SCALE to a Rational quantized
/// to QUANT_DENOM.
#[inline]
fn scaled_i32_to_rational(s: i32) -> Rational {
    // s in roughly [-CORDIC_SCALE, CORDIC_SCALE]. Rational value is
    // s / CORDIC_SCALE, quantized to QUANT_DENOM:
    //   num = round(s * QUANT_DENOM / CORDIC_SCALE)
    let prod = (s as i64) * (QUANT_DENOM as i64);
    let half = CORDIC_SCALE as i64 / 2;
    let n = if prod >= 0 {
        (prod + half) / CORDIC_SCALE as i64
    } else {
        (prod - half) / CORDIC_SCALE as i64
    };
    let n_i32 = n.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
    Rational::new(n_i32, QUANT_DENOM).unwrap_or(Rational::ZERO)
}

/// Integer absolute value for `i32`. `i32::MIN.abs()` would overflow,
/// so we saturate to `i32::MAX`. This matches the policy
/// [`Rational::abs`] uses and is fine for our scaled-sample range
/// (`±SAMPLE_SCALE`) which is comfortably inside `[i32::MIN+1,
/// i32::MAX]`.
#[inline]
pub fn abs_i32(x: i32) -> i32 {
    if x == i32::MIN {
        i32::MAX
    } else if x < 0 {
        -x
    } else {
        x
    }
}

/// Rational EMA tracker.
///
/// Mirrors the legacy `Ema { value, alpha }` struct but in exact
/// rational arithmetic. The recurrence is:
///
/// ```text
/// value_{i+1} = alpha * new_value + (1 - alpha) * value_i
/// ```
///
/// The same "first update initializes" quirk from the legacy code is
/// preserved so the f32 shim path produces bit-identical behaviour
/// where it matters.
///
/// Overflow note: rational add/mul use `checked_*` internally. If a
/// caller funnels in unbounded EMA values the denominator can blow up
/// in principle — for Marine the values are tightly bounded (period ≤
/// a few thousand samples, amplitude in `[0, SAMPLE_SCALE]`) so this is
/// not a problem in practice. We saturate to the previous value on
/// overflow rather than panic.
#[derive(Debug, Clone)]
pub struct EmaRational {
    pub(crate) value: Rational,
    pub(crate) alpha: Rational,
    /// One-minus-alpha pre-computed for speed and to avoid repeated
    /// rational arithmetic in the hot path.
    one_minus_alpha: Rational,
    initialised: bool,
}

impl EmaRational {
    /// Construct an EMA with the given smoothing factor. `alpha` is
    /// assumed to lie in `[0, 1]`; values outside are accepted but
    /// produce nonsense per the standard EMA formula.
    pub fn new(alpha: Rational) -> Self {
        let one_minus_alpha = Rational::ONE.checked_sub(alpha).unwrap_or(Rational::ZERO);
        Self {
            value: Rational::ZERO,
            alpha,
            one_minus_alpha,
            initialised: false,
        }
    }

    /// Update with a new observation. Returns `true` if the new value
    /// fit cleanly into a reduced `Rational`; returns `false` and
    /// leaves the state unchanged on overflow (extremely rare for
    /// Marine inputs but possible if the denominator chain grows).
    pub fn update(&mut self, new_value: Rational) -> bool {
        if !self.initialised {
            // Quantize the seed so the denominator chain stays bounded
            // for the first update too.
            self.value = quantize(new_value);
            self.initialised = true;
            return true;
        }
        // alpha * new_value + (1 - alpha) * old
        let t1 = match self.alpha.checked_mul(new_value) {
            Some(v) => v,
            None => return false,
        };
        let t2 = match self.one_minus_alpha.checked_mul(self.value) {
            Some(v) => v,
            None => return false,
        };
        let sum = match t1.checked_add(t2) {
            Some(v) => v,
            None => return false,
        };
        // Quantize: without this, repeated EMA updates grow the
        // denominator geometrically and `checked_*` starts returning
        // `None`. With quantization we lose at most 1 part in
        // `QUANT_DENOM` (~1e-6) of precision per step — far better
        // than f32's mantissa drift, and bounded.
        self.value = quantize(sum);
        true
    }

    /// Current EMA value.
    #[inline]
    pub fn get(&self) -> Rational {
        self.value
    }

    /// Has the EMA been seeded with at least one observation?
    #[inline]
    pub fn is_initialised(&self) -> bool {
        self.initialised
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_round_trip_within_one_lsb() {
        for s_int in [-32768, -1000, -1, 0, 1, 1000, 32767] {
            let f = sample_i32_to_f32(s_int);
            let back = sample_f32_to_i32(f);
            assert!(
                (s_int - back).abs() <= 1,
                "round-trip drifted: {} -> {} -> {}",
                s_int,
                f,
                back
            );
        }
    }

    #[test]
    fn rational_to_f32_basics() {
        assert_eq!(rational_to_f32(Rational::ZERO), 0.0);
        assert_eq!(rational_to_f32(Rational::ONE), 1.0);
        let half = Rational::new(1, 2).unwrap();
        assert!((rational_to_f32(half) - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn f32_to_rational_approximates() {
        let r = f32_to_rational(0.3);
        // Should be very close to 3/10 once reduced; numerator over
        // 1_000_000 is at most off by one due to f32 mantissa.
        let recovered = rational_to_f32(r);
        assert!(
            (recovered - 0.3).abs() < 1e-5,
            "f32→Rational→f32 drifted: {}",
            recovered
        );
    }

    #[test]
    fn ema_initialises_with_first_value() {
        let mut e = EmaRational::new(Rational::new(3, 10).unwrap());
        assert!(!e.is_initialised());
        e.update(Rational::from_int(10));
        assert!(e.is_initialised());
        assert_eq!(e.get(), Rational::from_int(10));
    }

    #[test]
    fn ema_update_matches_formula() {
        // alpha = 3/10
        let mut e = EmaRational::new(Rational::new(3, 10).unwrap());
        e.update(Rational::from_int(10));
        e.update(Rational::from_int(20));
        // Expected: 0.3*20 + 0.7*10 = 6 + 7 = 13
        assert_eq!(e.get(), Rational::from_int(13));
    }

    #[test]
    fn abs_i32_handles_min() {
        assert_eq!(abs_i32(i32::MIN), i32::MAX);
        assert_eq!(abs_i32(-5), 5);
        assert_eq!(abs_i32(0), 0);
        assert_eq!(abs_i32(7), 7);
    }

    #[test]
    fn mul_quantized_basic() {
        // (1/2) * (1/4) = 1/8. We compare via f32 to dodge the reduced-form
        // storage `Rational::new` enforces (the result is stored as 1/8, not
        // 125_000 / QUANT_DENOM).
        let a = quantize(Rational::new(1, 2).unwrap());
        let b = quantize(Rational::new(1, 4).unwrap());
        let result = mul_quantized(a, b);
        assert!((rational_to_f32(result) - 0.125).abs() < 1e-5);
    }

    #[test]
    fn mul_quantized_sidesteps_u32_denom_ceiling() {
        // Pick two coprime-with-each-other-and-with-1e6 numerators near
        // QUANT_DENOM, so the inputs survive reduction and carry the full
        // `den = 1_000_000`. Multiplying via `Rational::checked_mul` would
        // then need `den = 1e12` (overflows u32) and return None.
        // `mul_quantized` must succeed.
        let a = Rational::new(999_983, QUANT_DENOM).unwrap();
        let b = Rational::new(999_979, QUANT_DENOM).unwrap();
        assert_eq!(a.den, QUANT_DENOM, "input a must resist reduction");
        assert_eq!(b.den, QUANT_DENOM, "input b must resist reduction");
        assert!(
            a.checked_mul(b).is_none(),
            "guard: setup must overflow checked_mul",
        );
        let result = mul_quantized(a, b);
        // ~0.999983 * 0.999979 ≈ 0.999962.
        assert!((rational_to_f32(result) - 0.999962).abs() < 1e-5);
    }

    #[test]
    fn mul_quantized_zero_short_circuits() {
        let some = Rational::new(123_456, QUANT_DENOM).unwrap();
        assert_eq!(mul_quantized(Rational::ZERO, some), Rational::ZERO);
        assert_eq!(mul_quantized(some, Rational::ZERO), Rational::ZERO);
    }

    #[test]
    fn square_quantized_matches_self_multiply() {
        // (3/4)^2 = 9/16 = 0.5625. Compare via f32 to dodge reduced-form.
        let r = quantize(Rational::new(3, 4).unwrap());
        let s = square_quantized(r);
        assert!((rational_to_f32(s) - 0.5625).abs() < 1e-5);
        // And it agrees with mul_quantized(r, r).
        assert_eq!(s, mul_quantized(r, r));
    }

    #[test]
    fn square_quantized_zero() {
        assert_eq!(square_quantized(Rational::ZERO), Rational::ZERO);
    }

    // -----------------------------------------------------------------
    // Wave 2b primitives: isqrt, sqrt_rational, sin/cos via CORDIC.
    //
    // Each is property-tested against an f64 reference. The reference
    // path uses `f64::sqrt` / `f64::sin` / `f64::cos`, which are
    // available in the test environment (`cargo test` is std). The
    // production primitives never touch f64.
    // -----------------------------------------------------------------

    #[test]
    fn isqrt_basic_values() {
        assert_eq!(isqrt(0), 0);
        assert_eq!(isqrt(1), 1);
        assert_eq!(isqrt(2), 1);
        assert_eq!(isqrt(3), 1);
        assert_eq!(isqrt(4), 2);
        assert_eq!(isqrt(8), 2);
        assert_eq!(isqrt(9), 3);
        assert_eq!(isqrt(100), 10);
        assert_eq!(isqrt(99), 9);
        assert_eq!(isqrt(101), 10);
        assert_eq!(isqrt(1_000_000), 1000);
        // Boundary near u64::MAX. sqrt(u64::MAX) = 4_294_967_295.999...
        assert_eq!(isqrt(u64::MAX), u32::MAX);
        // Exact squares.
        for r in [37u64, 1234u64, 65535u64, 1u64 << 20, 1u64 << 31] {
            assert_eq!(isqrt(r * r), r as u32);
            assert_eq!(isqrt(r * r - 1), r as u32 - 1);
        }
    }

    #[test]
    fn isqrt_matches_float_reference() {
        // Compare isqrt against f64::sqrt floor for a dense sweep.
        extern crate std;
        let mut worst: i64 = 0;
        for x in (0u64..10_000).chain([
            1_234_567u64,
            12_345_678,
            123_456_789,
            1_234_567_890,
            u32::MAX as u64,
            u64::MAX / 2,
        ]) {
            let r = isqrt(x);
            let f = (x as f64).sqrt().floor() as i64;
            let drift = (r as i64) - f;
            if drift.abs() > worst.abs() {
                worst = drift;
            }
            assert!(
                drift.abs() <= 1,
                "isqrt drift at x={}: int={}, float_floor={}",
                x,
                r,
                f
            );
        }
        // Worst-case in this sweep is ±1 (float rounding around the
        // boundary). Documented for the report.
        let _ = worst;
    }

    #[test]
    fn sqrt_rational_matches_float_reference() {
        extern crate std;
        // Cover a few orders of magnitude. The target is "1e-5 absolute
        // error relative to magnitude" per the brief.
        let cases: &[(i32, u32)] = &[
            (1, 1),         // sqrt(1) = 1
            (4, 1),         // 2
            (2, 1),         // ~1.4142
            (1, 2),         // ~0.7071
            (9, 16),        // 0.75
            (25, 1),        // 5
            (1, 1_000_000), // 1e-3
            (10_000, 1),    // 100
        ];
        for &(n, d) in cases {
            let r = Rational::new(n, d).unwrap();
            let got = sqrt_rational(r);
            let got_f = (got.num as f64) / (got.den as f64);
            let exp_f = ((n as f64) / (d as f64)).sqrt();
            let drift = (got_f - exp_f).abs();
            assert!(
                drift < 1e-5 || drift < exp_f * 1e-5,
                "sqrt({}/{}) = {} (got) vs {} (exp), drift {}",
                n,
                d,
                got_f,
                exp_f,
                drift
            );
        }
    }

    #[test]
    fn sqrt_rational_zero_and_negative() {
        assert_eq!(sqrt_rational(Rational::ZERO), Rational::ZERO);
        assert_eq!(sqrt_rational(Rational::new(-4, 1).unwrap()), Rational::ZERO);
    }

    #[test]
    fn pi_rational_close_to_pi() {
        // 355/113 - π ≈ 2.667e-7
        let pi_approx = (PI_RATIONAL.num as f64) / (PI_RATIONAL.den as f64);
        assert!((pi_approx - core::f64::consts::PI).abs() < 1e-6);
    }

    #[test]
    fn sin_cos_at_known_angles() {
        extern crate std;
        // Build angles as Rationals near key points.
        let cases: &[(i32, u32, f64, f64)] = &[
            (0, 1, 0.0, 1.0),       // sin(0)=0, cos(0)=1
            (1, 1, 0.8414, 0.5403), // sin(1), cos(1)
            (-1, 1, -0.8414, 0.5403),
            (3, 2, 0.9974, 0.0707),    // 1.5 rad
            (-3, 1, -0.1411, -0.9899), // -3 rad
            (1, 2, 0.4794, 0.8775),    // 0.5 rad
        ];
        for &(n, d, exp_s, exp_c) in cases {
            let a = Rational::new(n, d).unwrap();
            let (s, c) = sin_cos_rational(a);
            let s_f = (s.num as f64) / (s.den as f64);
            let c_f = (c.num as f64) / (c.den as f64);
            assert!(
                (s_f - exp_s).abs() < 1e-3,
                "sin({}/{}) = {} vs expected {}",
                n,
                d,
                s_f,
                exp_s
            );
            assert!(
                (c_f - exp_c).abs() < 1e-3,
                "cos({}/{}) = {} vs expected {}",
                n,
                d,
                c_f,
                exp_c
            );
        }
    }

    #[test]
    fn sin_cos_matches_float_reference_dense() {
        extern crate std;
        // Sweep across [-2π, 2π] and check sin/cos against f64 to 1e-3.
        let mut worst_sin: f64 = 0.0;
        let mut worst_cos: f64 = 0.0;
        for k in -200i32..=200 {
            let angle_f = (k as f64) * 0.05; // -10 .. +10 rad
            let a = Rational::new(k * 5, 100).unwrap();
            let (s, c) = sin_cos_rational(a);
            let s_f = (s.num as f64) / (s.den as f64);
            let c_f = (c.num as f64) / (c.den as f64);
            let ds = (s_f - angle_f.sin()).abs();
            let dc = (c_f - angle_f.cos()).abs();
            if ds > worst_sin {
                worst_sin = ds;
            }
            if dc > worst_cos {
                worst_cos = dc;
            }
        }
        assert!(
            worst_sin < 1e-3 && worst_cos < 1e-3,
            "CORDIC drift: worst_sin={}, worst_cos={}",
            worst_sin,
            worst_cos
        );
    }

    #[test]
    fn sin_cos_pythagorean_identity() {
        // sin^2 + cos^2 should be ≈ 1 across the input range.
        for k in 0i32..50 {
            let a = Rational::new(k, 10).unwrap();
            let (s, c) = sin_cos_rational(a);
            let s_f = (s.num as f64) / (s.den as f64);
            let c_f = (c.num as f64) / (c.den as f64);
            let sum = s_f * s_f + c_f * c_f;
            assert!((sum - 1.0).abs() < 5e-3, "sin²+cos² at {}/10 = {}", k, sum);
        }
    }

    #[test]
    fn log2_q5_rational_matches_float_floor() {
        // Compare against floor(32 * log2(num/den)) computed in f64.
        let cases: &[(i32, u32)] = &[
            (1, 1),     // log2 1   = 0
            (2, 1),     // 1 octave  -> 32
            (4, 1),     // 2 octaves -> 64
            (1, 2),     // -1 octave -> -32
            (3, 2),     // ~0.585 oct -> 18
            (73, 100),  // 0.73 Hz
            (146, 100), // 1.46 Hz (one octave above 0.73)
            (440, 1),   // 440 Hz
            (100, 7),   // ~14.28
        ];
        for &(n, d) in cases {
            let r = Rational::new(n, d).unwrap();
            let got = log2_q5_rational(r);
            let expected = (32.0 * ((n as f64) / (d as f64)).log2()).floor() as i32;
            // 1 Q5-unit (1/32 octave) tolerance for boundary rounding.
            assert!(
                (got - expected).abs() <= 1,
                "log2_q5({}/{}) = {}, expected ~{}",
                n,
                d,
                got,
                expected
            );
        }
        // Guards: non-positive / zero return the axis floor (0).
        assert_eq!(log2_q5_rational(Rational::ZERO), 0);
    }
}
