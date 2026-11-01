//! Integer-only implementation of the Marine Algorithm.
//! AyeOS modification: replace unused peak payload history with its bounded count.
//! Upstream and attribution are recorded in ../PROVENANCE.md.
//!
//! This is the long-term home of the algorithm. The deprecated f32
//! [`crate::Marine`] is a thin shim on top of this module. Everything
//! here operates in `i32` (samples) and [`Rational`] (config thresholds,
//! EMA state, salience score) — no `f32`, no `f64`, no transcendentals.
//!
//! ## Pipeline mirrors `crate::Marine`
//!
//! 1. Pre-gate: `|sample| < clip_threshold_scaled` → decay current salience
//! 2. Peak detection: `second_last < last > current` (or symmetric valley)
//! 3. Jitter: |period - EMA(period)|, |amplitude - EMA(amplitude)|
//! 4. Harmonic alignment: distances to k·EMA(period) and EMA(period)/k
//! 5. Salience: `w_e · E_norm + w_j · 1/(1+J) + w_h · H`, clamped to [0,1]
//!
//! ## Sample range
//!
//! Inputs are `i32` in scaled-PCM units (`SAMPLE_SCALE = 32768` maps
//! `1.0f32` → `32768`). Internally, amplitudes are converted to a
//! `Rational` in `[0, 1]` by dividing by `SAMPLE_SCALE` — that's where
//! the natural `[0,1]` energy axis comes from, matching the legacy
//! semantics where `peak.amplitude` was the f32 absolute sample.

use crate::internal_math::{abs_i32, quantize, EmaRational, SAMPLE_SCALE};
use crate::rational::Rational;

// ----------------------------------------------------------------------------
// Default constants (rational form).
// ----------------------------------------------------------------------------

/// `0.3` smoothing factor.
pub const DEFAULT_EMA_ALPHA: Rational = Rational { num: 3, den: 10 };

/// `0.01` clip threshold expressed as fraction of full-scale.
pub const DEFAULT_CLIP_THRESHOLD: Rational = Rational { num: 1, den: 100 };

/// `0.05` harmonic detection epsilon.
pub const DEFAULT_HARMONIC_EPSILON: Rational = Rational { num: 1, den: 20 };

/// Number of harmonics to check.
pub const DEFAULT_HARMONIC_COUNT: usize = 8;

/// Salience weight on energy.
pub const DEFAULT_WEIGHT_ENERGY: Rational = Rational { num: 3, den: 10 };
/// Salience weight on stability (1/jitter).
pub const DEFAULT_WEIGHT_JITTER: Rational = Rational { num: 4, den: 10 };
/// Salience weight on harmonic alignment.
pub const DEFAULT_WEIGHT_HARMONIC: Rational = Rational { num: 3, den: 10 };

/// `0.9` decay coefficient when no signal is present.
pub const DEFAULT_SALIENCE_DECAY_RATE: Rational = Rational { num: 9, den: 10 };

/// Threshold below which jitter is considered "perfect stability".
/// Equivalent to the legacy `0.001` float constant. Stored as `1/1000`.
const STABILITY_EPS: Rational = Rational { num: 1, den: 1000 };

// ----------------------------------------------------------------------------
// Public types (integer-typed).
// ----------------------------------------------------------------------------

/// A single peak detected in the signal, integer form.
///
/// `amplitude` is a `Rational` in `[0, 1]` (peak height divided by
/// `SAMPLE_SCALE`). `period` is the sample distance since the previous
/// peak.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeakRational {
    /// Sample index where peak occurred.
    pub index: usize,
    /// Amplitude of the peak, normalised to `[0, 1]`.
    pub amplitude: Rational,
    /// Period since the previous peak (in samples).
    pub period: usize,
}

/// Configuration parameters for the Marine Algorithm (integer form).
///
/// All thresholds are exact rationals. The legacy
/// [`crate::MarineConfig`] is a thin conversion layer over this struct.
#[derive(Debug, Clone)]
pub struct MarineConfigRational {
    /// Clip threshold as a fraction of full-scale amplitude.
    /// Signals with `|sample| / SAMPLE_SCALE < clip_threshold` are ignored.
    pub clip_threshold: Rational,
    /// EMA smoothing factor in `[0, 1]`.
    pub ema_alpha: Rational,
    /// Tolerance for harmonic matching, as a fraction of the fundamental
    /// period.
    pub harmonic_epsilon: Rational,
    /// Number of harmonics to check (typically 2-8).
    pub harmonic_count: usize,
    /// Weight on the energy term of the salience score.
    pub weight_energy: Rational,
    /// Weight on the stability term (1 / (1 + jitter)).
    pub weight_jitter: Rational,
    /// Weight on the harmonic alignment term.
    pub weight_harmonic: Rational,
    /// Per-sample salience decay rate when the signal is below threshold.
    /// `1` = no decay, `0` = instant decay.
    pub salience_decay_rate: Rational,
}

impl Default for MarineConfigRational {
    fn default() -> Self {
        Self {
            clip_threshold: DEFAULT_CLIP_THRESHOLD,
            ema_alpha: DEFAULT_EMA_ALPHA,
            harmonic_epsilon: DEFAULT_HARMONIC_EPSILON,
            harmonic_count: DEFAULT_HARMONIC_COUNT,
            weight_energy: DEFAULT_WEIGHT_ENERGY,
            weight_jitter: DEFAULT_WEIGHT_JITTER,
            weight_harmonic: DEFAULT_WEIGHT_HARMONIC,
            salience_decay_rate: DEFAULT_SALIENCE_DECAY_RATE,
        }
    }
}

/// Statistics about the integer Marine algorithm state.
#[derive(Debug, Clone, Copy)]
pub struct MarineStatsRational {
    /// Number of samples processed since construction or reset.
    pub sample_index: usize,
    /// EMA-tracked average period (samples).
    pub avg_period: Rational,
    /// EMA-tracked average amplitude in `[0, 1]`.
    pub avg_amplitude: Rational,
    /// Number of recent peaks held in the harmonic-analysis ring.
    pub peak_count: usize,
    /// Current salience score, clamped to `[0, 1]`.
    pub current_salience: Rational,
}

// ----------------------------------------------------------------------------
// MarineInt — the integer implementation.
// ----------------------------------------------------------------------------

/// Integer-only Marine state machine.
///
/// API parallels [`crate::Marine`] but operates in `i32`/`Rational`.
/// The legacy `f32` API in [`crate`] is implemented in terms of this
/// struct.
#[derive(Debug, Clone)]
pub struct MarineInt {
    pub(crate) config: MarineConfigRational,
    /// Clip threshold pre-scaled to `i32` sample units (avoid recomputing
    /// per sample). Equivalent to `clip_threshold * SAMPLE_SCALE`.
    clip_threshold_scaled: i32,
    last_sample: i32,
    second_last_sample: i32,
    pub(crate) sample_index: usize,
    last_peak: Option<PeakRational>,
    period_ema: EmaRational,
    amplitude_ema: EmaRational,
    recent_peak_count: usize,
    pub(crate) current_salience: Rational,
}

impl MarineInt {
    /// Build with default configuration.
    pub fn new() -> Self {
        Self::with_config(MarineConfigRational::default())
    }

    /// Build with the supplied configuration.
    pub fn with_config(config: MarineConfigRational) -> Self {
        let clip_threshold_scaled = clip_threshold_to_i32(config.clip_threshold);
        Self {
            period_ema: EmaRational::new(config.ema_alpha),
            amplitude_ema: EmaRational::new(config.ema_alpha),
            last_sample: 0,
            second_last_sample: 0,
            sample_index: 0,
            last_peak: None,
            recent_peak_count: 0,
            current_salience: Rational::ZERO,
            clip_threshold_scaled,
            config,
        }
    }

    /// Process one scaled-PCM sample. Returns the current salience as a
    /// rational in `[0, 1]`.
    pub fn process_sample(&mut self, sample: i32) -> Rational {
        // Step 1: Pre-gate.
        if abs_i32(sample) < self.clip_threshold_scaled {
            self.advance_sample_state(sample);
            // current_salience *= salience_decay_rate
            //
            // Repeated multiplication of two non-quantized rationals
            // would grow the denominator geometrically (e.g.
            // `9/10 * 9/10 * ...` becomes `9^k / 10^k`) and quickly
            // exceed `u32::MAX`, at which point `checked_mul` returns
            // `None`. We quantize back to a fixed denominator after
            // each iteration to keep the state bounded.
            self.current_salience = quantize(
                self.current_salience
                    .checked_mul(self.config.salience_decay_rate)
                    .unwrap_or(Rational::ZERO),
            );
            return self.current_salience;
        }

        // Step 2: Peak / valley detection.
        let is_peak = self.second_last_sample < self.last_sample && self.last_sample > sample;
        let is_valley = self.second_last_sample > self.last_sample && self.last_sample < sample;

        if is_peak || is_valley {
            // Amplitude is |last_sample| normalised to [0,1] by SAMPLE_SCALE.
            let amp_int = abs_i32(self.last_sample);
            let peak_amplitude = amplitude_i32_to_rational(amp_int);

            let period = if let Some(last_peak) = self.last_peak {
                self.sample_index - 1 - last_peak.index
            } else {
                1
            };

            let peak = PeakRational {
                index: self.sample_index - 1,
                amplitude: peak_amplitude,
                period,
            };

            let (period_jitter, amplitude_jitter) = self.compute_jitter(&peak);
            let harmonic_score = self.compute_harmonic_alignment(&peak);

            // Quantize the freshly computed salience back to a fixed
            // denominator so subsequent decay-rate multiplications
            // don't blow up the denominator.
            self.current_salience = quantize(self.compute_salience(
                peak_amplitude,
                period_jitter,
                amplitude_jitter,
                harmonic_score,
            ));

            self.update_peak_state(peak);
        }

        self.advance_sample_state(sample);
        self.current_salience
    }

    /// Compute period and amplitude jitter for a new peak.
    ///
    /// Period jitter is in "samples" (a `Rational` because the EMA is
    /// rational). Amplitude jitter is in `[0, 1]`.
    fn compute_jitter(&self, peak: &PeakRational) -> (Rational, Rational) {
        if self.last_peak.is_none() {
            return (Rational::ZERO, Rational::ZERO);
        }
        let period_r = Rational::from_int(peak.period as i32);
        let period_jitter = sub_abs(period_r, self.period_ema.get());
        let amplitude_jitter = sub_abs(peak.amplitude, self.amplitude_ema.get());
        (period_jitter, amplitude_jitter)
    }

    /// Compute harmonic alignment score in `[0, 1]`.
    fn compute_harmonic_alignment(&self, peak: &PeakRational) -> Rational {
        if self.recent_peak_count < 2 {
            return Rational::ZERO;
        }
        let fundamental = self.period_ema.get();
        // < 1 sample period is degenerate.
        if fundamental < Rational::ONE {
            return Rational::ZERO;
        }
        // epsilon = harmonic_epsilon * fundamental
        let epsilon = match self.config.harmonic_epsilon.checked_mul(fundamental) {
            Some(v) => v,
            None => return Rational::ZERO,
        };
        if epsilon.is_zero() {
            return Rational::ZERO;
        }

        let mut harmonic_matches = Rational::ZERO;
        let period_r = Rational::from_int(peak.period as i32);

        // Forward harmonics 2f .. kf.
        for k in 2..=self.config.harmonic_count {
            let k_r = Rational::from_int(k as i32);
            let expected = match fundamental.checked_mul(k_r) {
                Some(v) => v,
                None => continue,
            };
            let diff = sub_abs(period_r, expected);
            if diff < epsilon {
                // match_quality = 1 - diff/epsilon
                if let Some(ratio) = diff.checked_div(epsilon) {
                    if let Some(quality) = Rational::ONE.checked_sub(ratio) {
                        // contribution = quality / k
                        if let Some(contrib) = quality.checked_div(k_r) {
                            if let Some(sum) = harmonic_matches.checked_add(contrib) {
                                harmonic_matches = sum;
                            }
                        }
                    }
                }
            }
        }

        // Sub-harmonics f/2 .. f/k.
        for k in 2..=self.config.harmonic_count {
            let k_r = Rational::from_int(k as i32);
            let expected = match fundamental.checked_div(k_r) {
                Some(v) => v,
                None => continue,
            };
            let diff = sub_abs(period_r, expected);
            if diff < epsilon {
                if let Some(ratio) = diff.checked_div(epsilon) {
                    if let Some(quality) = Rational::ONE.checked_sub(ratio) {
                        if let Some(contrib) = quality.checked_div(k_r) {
                            if let Some(sum) = harmonic_matches.checked_add(contrib) {
                                harmonic_matches = sum;
                            }
                        }
                    }
                }
            }
        }

        // Normalise by 2 * harmonic_count.
        let total = Rational::from_int((self.config.harmonic_count as i32) * 2);
        harmonic_matches
            .checked_div(total)
            .unwrap_or(Rational::ZERO)
    }

    /// Compute the final salience score, clamped to `[0, 1]`.
    fn compute_salience(
        &self,
        energy: Rational,
        period_jitter: Rational,
        amplitude_jitter: Rational,
        harmonic_score: Rational,
    ) -> Rational {
        let total_jitter = period_jitter
            .checked_add(amplitude_jitter)
            .unwrap_or(Rational::ZERO);

        // stability = if total_jitter > eps { 1/(1+J) } else { 1 }
        let stability = if total_jitter > STABILITY_EPS {
            // 1 / (1 + J)
            let one_plus = Rational::ONE
                .checked_add(total_jitter)
                .unwrap_or(Rational::ONE);
            Rational::ONE
                .checked_div(one_plus)
                .unwrap_or(Rational::ZERO)
        } else {
            Rational::ONE
        };

        // S = w_e * E + w_j * stability + w_h * H
        let part_e = self
            .config
            .weight_energy
            .checked_mul(energy)
            .unwrap_or(Rational::ZERO);
        let part_j = self
            .config
            .weight_jitter
            .checked_mul(stability)
            .unwrap_or(Rational::ZERO);
        let part_h = self
            .config
            .weight_harmonic
            .checked_mul(harmonic_score)
            .unwrap_or(Rational::ZERO);

        let s = part_e
            .checked_add(part_j)
            .and_then(|x| x.checked_add(part_h))
            .unwrap_or(Rational::ZERO);

        s.clamp(Rational::ZERO, Rational::ONE)
    }

    /// Update internal state after detecting a peak.
    fn update_peak_state(&mut self, peak: PeakRational) {
        if peak.period > 0 {
            self.period_ema
                .update(Rational::from_int(peak.period as i32));
        }
        self.amplitude_ema.update(peak.amplitude);

        // The pinned upstream algorithm only inspects the deque's length.
        // The public adapter fixes harmonic_count=8; no history allocation.
        self.recent_peak_count = (self.recent_peak_count + 1).min(self.config.harmonic_count);
        self.last_peak = Some(peak);
    }

    /// Slide the sample window forward by one tick.
    fn advance_sample_state(&mut self, sample: i32) {
        self.second_last_sample = self.last_sample;
        self.last_sample = sample;
        self.sample_index += 1;
    }

    /// Current salience without processing a new sample.
    #[inline]
    pub fn current_salience(&self) -> Rational {
        self.current_salience
    }

    /// Reset all state. The configuration is preserved.
    pub fn reset(&mut self) {
        self.last_sample = 0;
        self.second_last_sample = 0;
        self.sample_index = 0;
        self.last_peak = None;
        self.period_ema = EmaRational::new(self.config.ema_alpha);
        self.amplitude_ema = EmaRational::new(self.config.ema_alpha);
        self.recent_peak_count = 0;
        self.current_salience = Rational::ZERO;
    }

    /// Statistics snapshot.
    pub fn stats(&self) -> MarineStatsRational {
        MarineStatsRational {
            sample_index: self.sample_index,
            avg_period: self.period_ema.get(),
            avg_amplitude: self.amplitude_ema.get(),
            peak_count: self.recent_peak_count,
            current_salience: self.current_salience,
        }
    }
}

impl Default for MarineInt {
    fn default() -> Self {
        Self::new()
    }
}

// ----------------------------------------------------------------------------
// Helpers
// ----------------------------------------------------------------------------

/// Convert a fractional clip threshold (e.g. `1/100`) to a scaled-PCM
/// `i32` value (e.g. `~328`). Floors the result for "below threshold
/// means definitely below the configured fraction".
fn clip_threshold_to_i32(thr: Rational) -> i32 {
    // value = thr * SAMPLE_SCALE = (thr.num * SAMPLE_SCALE) / thr.den
    let num = thr.num as i64 * SAMPLE_SCALE as i64;
    let den = thr.den as i64;
    // Floor.
    let q = num / den;
    let r = num % den;
    let floored = if r < 0 { q - 1 } else { q };
    floored.clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

/// Convert a scaled-PCM amplitude in `[0, SAMPLE_SCALE]` to a
/// normalised `Rational` in `[0, 1]`.
fn amplitude_i32_to_rational(amp: i32) -> Rational {
    // amp / SAMPLE_SCALE, reduced. amp is already absolute value, so
    // it's >= 0; SAMPLE_SCALE is positive.
    Rational::new(amp, SAMPLE_SCALE as u32).unwrap_or(Rational::ZERO)
}

/// `|a - b|`, computed safely against rational overflow by falling
/// back to `ZERO`. In practice both operands are tightly bounded.
fn sub_abs(a: Rational, b: Rational) -> Rational {
    match a.checked_sub(b) {
        Some(v) => v.abs(),
        None => Rational::ZERO,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marine_creation_starts_at_zero() {
        let m = MarineInt::new();
        assert_eq!(m.current_salience(), Rational::ZERO);
        assert_eq!(m.sample_index, 0);
    }

    #[test]
    fn silence_decays_to_near_zero() {
        let mut m = MarineInt::new();
        for _ in 0..1000 {
            m.process_sample(0);
        }
        // Decay rate 9/10 ^ 1000 is astronomically small; current_salience
        // should be exactly zero because we never reached the clip
        // threshold to begin with.
        assert_eq!(m.current_salience(), Rational::ZERO);
    }

    #[test]
    fn clip_threshold_filters_weak_signals() {
        let cfg = MarineConfigRational {
            clip_threshold: Rational::new(1, 2).unwrap(), // 0.5
            ..Default::default()
        };
        let mut m = MarineInt::with_config(cfg);

        // 0.1 in scaled units = 3277. Below 0.5 * 32768 = 16384.
        for _ in 0..100 {
            let s = m.process_sample(3277);
            // Expect zero (we never crossed the gate).
            assert_eq!(s, Rational::ZERO);
        }
    }

    #[test]
    fn step_signal_detects_peak() {
        // Use a tiny custom config so the clip gate doesn't kill us.
        let mut m = MarineInt::new();
        // Send a triangle: up-up-down-up-up-down ...
        let mut max = Rational::ZERO;
        let samples: [i32; 12] = [
            1000, 5000, 10000, 5000, 1000, // first triangle
            5000, 10000, 15000, 10000, 5000, 1000, 0,
        ];
        for s in samples {
            let v = m.process_sample(s);
            if v > max {
                max = v;
            }
        }
        // Should have produced some salience by the second peak.
        assert!(max > Rational::ZERO, "no salience accrued");
    }

    #[test]
    fn reset_clears_state() {
        let mut m = MarineInt::new();
        for i in 0..50 {
            m.process_sample((i % 7) * 4000 - 14000);
        }
        m.reset();
        assert_eq!(m.current_salience(), Rational::ZERO);
        assert_eq!(m.sample_index, 0);
    }

    #[test]
    fn clip_threshold_scaling_is_correct() {
        // 1/100 of full-scale = 327 (floor of 32768/100 = 327.68).
        let t = clip_threshold_to_i32(Rational::new(1, 100).unwrap());
        assert_eq!(t, 327);
        // 1/2 of full-scale = 16384.
        let t = clip_threshold_to_i32(Rational::new(1, 2).unwrap());
        assert_eq!(t, 16384);
        // 0 stays 0.
        let t = clip_threshold_to_i32(Rational::ZERO);
        assert_eq!(t, 0);
    }
}
