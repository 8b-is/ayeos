//! Integer subset of aye marine-core internal_math.rs at the pinned revision.
//! Extracted SAMPLE_SCALE, quantize, abs_i32 and EmaRational; algorithm unchanged.
//! State rounds half-away-from-zero to a reduced millionth, with upstream i32
//! saturation. The adapter limits epochs to 2048 samples and inputs to i16.
use crate::rational::Rational;
pub const SAMPLE_SCALE: i32 = 32768;
pub const QUANT_DENOM: u32 = 1_000_000;

pub fn quantize(r: Rational) -> Rational {
    if r.is_zero() {
        return Rational::ZERO;
    }
    let num_scaled = (r.num as i64) * (QUANT_DENOM as i64);
    let den = r.den as i64;
    let q = num_scaled / den;
    let rem = num_scaled % den;
    let rounded = if rem == 0 {
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
    let num_i32 = rounded.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
    Rational::new(num_i32, QUANT_DENOM).unwrap_or(Rational::ZERO)
}

pub fn abs_i32(x: i32) -> i32 {
    if x == i32::MIN {
        i32::MAX
    } else if x < 0 {
        -x
    } else {
        x
    }
}

#[derive(Debug, Clone)]
pub struct EmaRational {
    value: Rational,
    alpha: Rational,
    one_minus_alpha: Rational,
    initialised: bool,
}
impl EmaRational {
    pub fn new(alpha: Rational) -> Self {
        Self {
            value: Rational::ZERO,
            alpha,
            one_minus_alpha: Rational::ONE.checked_sub(alpha).unwrap_or(Rational::ZERO),
            initialised: false,
        }
    }
    pub fn update(&mut self, new_value: Rational) -> bool {
        if !self.initialised {
            self.value = quantize(new_value);
            self.initialised = true;
            return true;
        }
        let Some(t1) = self.alpha.checked_mul(new_value) else {
            return false;
        };
        let Some(t2) = self.one_minus_alpha.checked_mul(self.value) else {
            return false;
        };
        let Some(sum) = t1.checked_add(t2) else {
            return false;
        };
        self.value = quantize(sum);
        true
    }
    pub fn get(&self) -> Rational {
        self.value
    }
}
