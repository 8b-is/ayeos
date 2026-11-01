//! A bounded adapter of aye's genuine MarineInt. See ../PROVENANCE.md.
//! This initial API accepts at most 2048 i16 PCM samples between resets.
//! It does not claim streaming, microphone input or a measured sample rate.
#![no_std]
#![forbid(unsafe_code)]

mod internal_math;
mod marine_int;
pub mod rational;
use marine_int::MarineInt;
pub use marine_int::MarineStatsRational;
use rational::Rational;

pub const MAX_SAMPLES: usize = 2048;

pub struct Marine {
    inner: MarineInt,
}
impl Marine {
    pub fn new() -> Self {
        Self {
            inner: MarineInt::new(),
        }
    }
    /// Exhaustion returns None without consuming the sample. The short epoch
    /// bounds usize indexes, i32 period conversions and the upstream EMA range.
    pub fn process(&mut self, sample: i16) -> Option<Rational> {
        if self.inner.stats().sample_index >= MAX_SAMPLES {
            return None;
        }
        Some(self.inner.process_sample(i32::from(sample)))
    }
    pub fn reset(&mut self) {
        self.inner.reset();
    }
    pub fn stats(&self) -> MarineStatsRational {
        self.inner.stats()
    }
}
impl Default for Marine {
    fn default() -> Self {
        Self::new()
    }
}
