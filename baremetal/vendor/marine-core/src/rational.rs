//! Rational number type for integer-only math across MEM|8.
//!
//! This is the canonical fixed-form rational used by the rest of the AyeOS
//! migration (Wave fields, grid coordinates, salience, JSON wire format).
//! It exists to give us exact, integer-only arithmetic with predictable
//! overflow behaviour and a clean wire representation — no floats anywhere.
//!
//! # Invariants
//!
//! Every `Rational` value satisfies:
//!
//! 1. `den > 0` — denominator is `u32`, so this is enforced by the type.
//!    Sign always lives in `num`.
//! 2. `gcd(|num|, den) == 1` — always stored in reduced (canonical) form.
//!    The constructor [`Rational::new`] and every arithmetic op reduce.
//!
//! Because reduced form is canonical, `PartialEq` / `Eq` / `Hash` are
//! meaningful: two `Rational`s compare equal iff they represent the same
//! mathematical value.
//!
//! # Overflow policy
//!
//! Arithmetic ops are exposed as `checked_*` returning `Option<Rational>`.
//! Intermediate computation happens in `i64` / `u64`, the result is
//! reduced, and then we verify it fits back in `i32` / `u32`. If it does
//! not, we return `None`. The foundation type does not silently lose
//! precision — callers can layer saturating wrappers on top if they need
//! to. For the `+`, `-`, `*`, `/` operators we provide `core::ops`
//! implementations that panic on overflow (mirroring the `i32` defaults).

#![allow(clippy::needless_doctest_main)]

use core::cmp::Ordering;
use core::hash::{Hash, Hasher};
use core::ops::{Add, Div, Mul, Neg, Sub};

/// Exact rational number `num / den` with `num: i32`, `den: u32`.
///
/// Always stored in reduced form with `den > 0`. See module docs for
/// invariants and overflow policy.
#[derive(Clone, Copy, Debug)]
pub struct Rational {
    /// Numerator. Carries the sign of the whole value.
    pub num: i32,
    /// Denominator. Always `> 0` by invariant.
    pub den: u32,
}

impl Rational {
    /// The rational `0`, stored as `0/1`.
    pub const ZERO: Rational = Rational { num: 0, den: 1 };

    /// The rational `1`, stored as `1/1`.
    pub const ONE: Rational = Rational { num: 1, den: 1 };

    /// Construct a new `Rational` and reduce it to canonical form.
    ///
    /// Returns `None` if `den == 0`.
    #[inline]
    pub fn new(num: i32, den: u32) -> Option<Rational> {
        if den == 0 {
            return None;
        }
        Some(reduce_i32_u32(num, den))
    }

    /// Construct without checking for `den == 0` and without reducing.
    ///
    /// For use inside hot paths where the caller has already established
    /// the invariants. In debug builds, `den == 0` will panic.
    ///
    /// # Safety / invariants
    ///
    /// Caller must ensure `den != 0` and that `gcd(|num|, den) == 1`, or
    /// the resulting value will violate `Rational`'s invariants and
    /// produce nonsense from `Eq` / `Hash` / ordering.
    #[inline]
    pub fn new_unchecked(num: i32, den: u32) -> Rational {
        debug_assert!(den != 0, "Rational::new_unchecked called with den == 0");
        Rational { num, den }
    }

    /// Reduce to canonical form. Idempotent.
    #[inline]
    pub fn reduce(self) -> Rational {
        reduce_i32_u32(self.num, self.den)
    }

    /// Construct from an integer (`i / 1`). Never overflows.
    #[inline]
    pub const fn from_int(i: i32) -> Rational {
        Rational { num: i, den: 1 }
    }

    /// Returns `true` if this value is exactly zero.
    #[inline]
    pub const fn is_zero(self) -> bool {
        self.num == 0
    }

    /// Sign: `-1` if negative, `0` if zero, `+1` if positive.
    #[inline]
    pub const fn sign(self) -> i8 {
        if self.num < 0 {
            -1
        } else if self.num == 0 {
            0
        } else {
            1
        }
    }

    /// Absolute value.
    ///
    /// `i32::MIN` cannot be negated as `i32`, so for `num == i32::MIN`
    /// this saturates to `i32::MAX` — the only place this type silently
    /// rounds. Callers that care should check `self.num != i32::MIN`.
    #[inline]
    pub const fn abs(self) -> Rational {
        let num = if self.num == i32::MIN {
            i32::MAX
        } else if self.num < 0 {
            -self.num
        } else {
            self.num
        };
        Rational { num, den: self.den }
    }

    /// Clamp into `[lo, hi]`. Returns `lo` if `self < lo`, `hi` if
    /// `self > hi`, else `self`. If `lo > hi`, returns `lo` (mirrors
    /// `core::cmp::Ord::clamp` contract that `lo <= hi`).
    #[inline]
    pub fn clamp(self, lo: Rational, hi: Rational) -> Rational {
        if self < lo {
            lo
        } else if self > hi {
            hi
        } else {
            self
        }
    }

    /// Negate. Returns `None` only when `num == i32::MIN` (cannot be
    /// represented as a positive `i32`).
    #[inline]
    pub fn checked_neg(self) -> Option<Rational> {
        let num = self.num.checked_neg()?;
        Some(Rational { num, den: self.den })
    }

    /// Checked addition. Returns `None` on overflow (either in the
    /// intermediate `i64`/`u64` reductions or in the final fit-check
    /// back into `i32`/`u32`).
    #[inline]
    pub fn checked_add(self, other: Rational) -> Option<Rational> {
        // (n1 * d2 + n2 * d1) / (d1 * d2), then reduce.
        let n1 = self.num as i64;
        let n2 = other.num as i64;
        let d1 = self.den as i64;
        let d2 = other.den as i64;
        let num = n1.checked_mul(d2)?.checked_add(n2.checked_mul(d1)?)?;
        let den = (self.den as u64).checked_mul(other.den as u64)?;
        reduce_i64_u64(num, den)
    }

    /// Checked subtraction. Returns `None` on overflow.
    #[inline]
    pub fn checked_sub(self, other: Rational) -> Option<Rational> {
        let n1 = self.num as i64;
        let n2 = other.num as i64;
        let d1 = self.den as i64;
        let d2 = other.den as i64;
        let num = n1.checked_mul(d2)?.checked_sub(n2.checked_mul(d1)?)?;
        let den = (self.den as u64).checked_mul(other.den as u64)?;
        reduce_i64_u64(num, den)
    }

    /// Checked multiplication. Returns `None` on overflow.
    #[inline]
    pub fn checked_mul(self, other: Rational) -> Option<Rational> {
        let num = (self.num as i64).checked_mul(other.num as i64)?;
        let den = (self.den as u64).checked_mul(other.den as u64)?;
        reduce_i64_u64(num, den)
    }

    /// Checked division. Returns `None` if `other == 0` or on overflow.
    #[inline]
    pub fn checked_div(self, other: Rational) -> Option<Rational> {
        if other.num == 0 {
            return None;
        }
        // self / other = (self.num * other.den) / (self.den * other.num)
        // other.num may be negative — fold its sign into the numerator.
        let lhs_num = (self.num as i64).checked_mul(other.den as i64)?;
        let rhs_num = (self.den as i64).checked_mul(other.num as i64)?;
        // Make the denominator positive.
        let (num, den_i) = if rhs_num < 0 {
            (lhs_num.checked_neg()?, rhs_num.checked_neg()?)
        } else {
            (lhs_num, rhs_num)
        };
        // den_i is now > 0 (other.num != 0 ⇒ rhs_num != 0, and we made it positive).
        reduce_i64_u64(num, den_i as u64)
    }

    /// Floor of the rational as an `i32`. Returns `None` if the floor
    /// would overflow `i32`.
    #[inline]
    pub fn try_to_i32_floor(self) -> Option<i32> {
        let n = self.num as i64;
        let d = self.den as i64;
        // Euclidean-style floor that works for negative numerators:
        // floor(n/d) when d > 0 is (n - ((n % d + d) % d)) / d, but for
        // d > 0 this simplifies to:
        let q = n / d;
        let r = n % d;
        let floor = if r < 0 { q - 1 } else { q };
        i32::try_from(floor).ok()
    }

    /// Ceiling of the rational as an `i32`. Returns `None` if the
    /// ceiling would overflow `i32`.
    #[inline]
    pub fn try_to_i32_ceil(self) -> Option<i32> {
        let n = self.num as i64;
        let d = self.den as i64;
        let q = n / d;
        let r = n % d;
        let ceil = if r > 0 { q + 1 } else { q };
        i32::try_from(ceil).ok()
    }
}

// ---- Equality, ordering, hashing ----
//
// Because reduced form is canonical, structural equality is the same as
// value equality, so we can derive-style implement these by hand against
// `(num, den)`. We still hand-implement to make the contract explicit.

impl PartialEq for Rational {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.num == other.num && self.den == other.den
    }
}

impl Eq for Rational {}

impl Hash for Rational {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.num.hash(state);
        self.den.hash(state);
    }
}

impl PartialOrd for Rational {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Rational {
    /// Compare via cross-multiplication in `i64` — no float conversion.
    ///
    /// `a/b` vs `c/d` with `b, d > 0` reduces to comparing `a*d` vs
    /// `c*b`. Both `den`s are `u32` so the products fit in `i64`.
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        // (self.num as i64) * (other.den as i64) vs (other.num as i64) * (self.den as i64)
        let lhs = (self.num as i64) * (other.den as i64);
        let rhs = (other.num as i64) * (self.den as i64);
        lhs.cmp(&rhs)
    }
}

// ---- Operator overloads (panic on overflow, like primitive ints) ----

impl Add for Rational {
    type Output = Rational;
    #[inline]
    fn add(self, other: Rational) -> Rational {
        self.checked_add(other)
            .expect("Rational::add overflow (use checked_add for a fallible variant)")
    }
}

impl Sub for Rational {
    type Output = Rational;
    #[inline]
    fn sub(self, other: Rational) -> Rational {
        self.checked_sub(other)
            .expect("Rational::sub overflow (use checked_sub for a fallible variant)")
    }
}

impl Mul for Rational {
    type Output = Rational;
    #[inline]
    fn mul(self, other: Rational) -> Rational {
        self.checked_mul(other)
            .expect("Rational::mul overflow (use checked_mul for a fallible variant)")
    }
}

impl Div for Rational {
    type Output = Rational;
    #[inline]
    fn div(self, other: Rational) -> Rational {
        self.checked_div(other)
            .expect("Rational::div by zero or overflow (use checked_div for a fallible variant)")
    }
}

impl Neg for Rational {
    type Output = Rational;
    #[inline]
    fn neg(self) -> Rational {
        self.checked_neg()
            .expect("Rational::neg overflow (num == i32::MIN)")
    }
}

// ---- Internal helpers ----

/// GCD for `u64`. Binary Euclidean would be faster, but this is short
/// and obviously correct; the rational hot paths are not inner loops.
#[inline]
const fn gcd_u64(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a
}

/// Reduce `(num: i32, den: u32)` into canonical form. `den` is assumed
/// non-zero (caller checks).
#[inline]
fn reduce_i32_u32(num: i32, den: u32) -> Rational {
    if num == 0 {
        return Rational::ZERO;
    }
    // Take |num| as u32 carefully — i32::MIN's |x| is 2^31, fits in u32.
    let abs_num: u32 = if num == i32::MIN {
        1u32 << 31
    } else {
        num.unsigned_abs()
    };
    let g = gcd_u64(abs_num as u64, den as u64) as u32;
    // g divides both, so the divides below are exact and cannot overflow.
    let new_abs = abs_num / g;
    let new_den = den / g;
    // Re-attach the sign. If num == i32::MIN and g == 1 (so new_abs ==
    // 2^31), we cannot represent it as positive i32 — that would be a
    // bug in a caller passing such a value. We saturate the magnitude
    // down by one rather than panic; this matches `abs()`'s policy. In
    // practice this is unreachable for any reduced rational that ever
    // came from valid arithmetic, since checked_* will have rejected it.
    let signed: i32 = if num < 0 {
        // -(new_abs as i32) is fine because new_abs <= 2^31 and if
        // new_abs == 2^31 then i32::MIN is the answer.
        if new_abs == 1u32 << 31 {
            i32::MIN
        } else {
            -(new_abs as i32)
        }
    } else {
        // For positive, new_abs == 2^31 cannot be represented — saturate.
        if new_abs == 1u32 << 31 {
            i32::MAX
        } else {
            new_abs as i32
        }
    };
    Rational {
        num: signed,
        den: new_den,
    }
}

/// Reduce an `(i64, u64)` intermediate and try to fit back into
/// `(i32, u32)`. Returns `None` on overflow.
#[inline]
fn reduce_i64_u64(num: i64, den: u64) -> Option<Rational> {
    if den == 0 {
        return None;
    }
    if num == 0 {
        return Some(Rational::ZERO);
    }
    let abs_num: u64 = num.unsigned_abs();
    let g = gcd_u64(abs_num, den);
    let new_abs = abs_num / g;
    let new_den = den / g;
    // Fit checks.
    let den_u32 = u32::try_from(new_den).ok()?;
    let num_i32: i32 = if num < 0 {
        // new_abs must be <= i32::MAX as u64 + 1 to fit as a negative i32.
        if new_abs > (i32::MAX as u64) + 1 {
            return None;
        }
        if new_abs == (i32::MAX as u64) + 1 {
            i32::MIN
        } else {
            -(new_abs as i32)
        }
    } else {
        if new_abs > i32::MAX as u64 {
            return None;
        }
        new_abs as i32
    };
    Some(Rational {
        num: num_i32,
        den: den_u32,
    })
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_rejects_zero_denominator() {
        assert!(Rational::new(1, 0).is_none());
        assert!(Rational::new(0, 0).is_none());
        assert!(Rational::new(-5, 0).is_none());
    }

    #[test]
    fn new_reduces_to_canonical_form() {
        let r = Rational::new(6, 8).unwrap();
        assert_eq!(r.num, 3);
        assert_eq!(r.den, 4);

        // Negative numerators reduce too.
        let r = Rational::new(-6, 8).unwrap();
        assert_eq!(r.num, -3);
        assert_eq!(r.den, 4);

        // Already reduced stays put.
        let r = Rational::new(3, 7).unwrap();
        assert_eq!(r.num, 3);
        assert_eq!(r.den, 7);

        // Zero reduces to 0/1.
        let r = Rational::new(0, 99).unwrap();
        assert_eq!(r, Rational::ZERO);
        assert_eq!(r.den, 1);
    }

    #[test]
    fn sign_always_in_numerator() {
        // The type forces this (den: u32), but verify it survives reduction.
        let r = Rational::new(-10, 15).unwrap();
        assert_eq!(r.num, -2);
        assert_eq!(r.den, 3);
        assert!(r.sign() < 0);
    }

    #[test]
    fn constants_are_canonical() {
        assert_eq!(Rational::ZERO.num, 0);
        assert_eq!(Rational::ZERO.den, 1);
        assert_eq!(Rational::ONE.num, 1);
        assert_eq!(Rational::ONE.den, 1);
        assert!(Rational::ZERO.is_zero());
        assert!(!Rational::ONE.is_zero());
    }

    #[test]
    fn from_int_works() {
        assert_eq!(Rational::from_int(7), Rational::new(7, 1).unwrap());
        assert_eq!(Rational::from_int(-3), Rational::new(-3, 1).unwrap());
        assert_eq!(Rational::from_int(0), Rational::ZERO);
    }

    #[test]
    fn arithmetic_round_trip() {
        let cases = [
            (Rational::new(1, 2).unwrap(), Rational::new(1, 3).unwrap()),
            (Rational::new(-3, 4).unwrap(), Rational::new(5, 6).unwrap()),
            (Rational::new(7, 11).unwrap(), Rational::new(-2, 9).unwrap()),
            (Rational::ZERO, Rational::new(13, 17).unwrap()),
        ];
        for (a, b) in cases {
            let sum = a.checked_add(b).unwrap();
            let back = sum.checked_sub(b).unwrap();
            assert_eq!(back, a, "(a + b) - b should equal a for {:?} {:?}", a, b);
        }
    }

    #[test]
    fn add_sub_specific_values() {
        // 1/2 + 1/3 = 5/6
        let r = Rational::new(1, 2)
            .unwrap()
            .checked_add(Rational::new(1, 3).unwrap())
            .unwrap();
        assert_eq!(r, Rational::new(5, 6).unwrap());

        // 1/2 - 1/3 = 1/6
        let r = Rational::new(1, 2)
            .unwrap()
            .checked_sub(Rational::new(1, 3).unwrap())
            .unwrap();
        assert_eq!(r, Rational::new(1, 6).unwrap());

        // 2/4 + 1/2 = 1 (after reduction)
        let r = Rational::new(2, 4)
            .unwrap()
            .checked_add(Rational::new(1, 2).unwrap())
            .unwrap();
        assert_eq!(r, Rational::ONE);
    }

    #[test]
    fn mul_div_specific_values() {
        // (2/3) * (3/4) = 1/2
        let r = Rational::new(2, 3)
            .unwrap()
            .checked_mul(Rational::new(3, 4).unwrap())
            .unwrap();
        assert_eq!(r, Rational::new(1, 2).unwrap());

        // (1/2) / (3/4) = 2/3
        let r = Rational::new(1, 2)
            .unwrap()
            .checked_div(Rational::new(3, 4).unwrap())
            .unwrap();
        assert_eq!(r, Rational::new(2, 3).unwrap());

        // Division by zero returns None.
        assert!(Rational::ONE.checked_div(Rational::ZERO).is_none());

        // Division by a negative.
        let r = Rational::new(1, 2)
            .unwrap()
            .checked_div(Rational::new(-3, 4).unwrap())
            .unwrap();
        assert_eq!(r, Rational::new(-2, 3).unwrap());
    }

    #[test]
    fn neg_works() {
        assert_eq!(-Rational::new(3, 4).unwrap(), Rational::new(-3, 4).unwrap());
        assert_eq!(-Rational::ZERO, Rational::ZERO);
        // i32::MIN cannot be negated.
        let r = Rational {
            num: i32::MIN,
            den: 1,
        };
        assert!(r.checked_neg().is_none());
    }

    #[test]
    fn comparison_ordering() {
        // Basic.
        assert!(Rational::new(1, 3).unwrap() < Rational::new(1, 2).unwrap());
        assert!(Rational::new(2, 3).unwrap() > Rational::new(1, 2).unwrap());

        // Equality of equivalent unreduced forms — after reduction.
        assert_eq!(Rational::new(2, 4).unwrap(), Rational::new(1, 2).unwrap());

        // Negative cases.
        assert!(Rational::new(-1, 2).unwrap() < Rational::new(1, 3).unwrap());
        assert!(Rational::new(-1, 2).unwrap() < Rational::new(-1, 3).unwrap());
        assert!(Rational::new(-2, 3).unwrap() < Rational::new(-1, 2).unwrap());

        // Zero in the middle.
        assert!(Rational::new(-1, 1000).unwrap() < Rational::ZERO);
        assert!(Rational::ZERO < Rational::new(1, 1000).unwrap());
    }

    #[test]
    fn checked_add_overflow_returns_none() {
        // den overflow: gcd(d1, d2) == 1 forces d1 * d2 in the result.
        // u32::MAX is a prime-ish big denominator; squaring it overflows u32.
        let a = Rational::new(1, u32::MAX - 1).unwrap(); // u32::MAX - 1 = 2 * (some big)
        let b = Rational::new(1, u32::MAX).unwrap();
        // After reduction the denominator may still not fit in u32. Either
        // way the contract is "if it doesn't fit, return None"; we just
        // verify the function doesn't panic and is well-defined.
        let _ = a.checked_add(b); // may be Some or None, must not panic
                                  // Force an overflow we KNOW happens: i32::MAX/1 + i32::MAX/1.
        let big = Rational::from_int(i32::MAX);
        assert!(big.checked_add(big).is_none());
    }

    #[test]
    fn checked_mul_overflow_returns_none() {
        let big = Rational::from_int(i32::MAX);
        assert!(big.checked_mul(big).is_none());
    }

    #[test]
    fn abs_works() {
        assert_eq!(
            Rational::new(-3, 4).unwrap().abs(),
            Rational::new(3, 4).unwrap()
        );
        assert_eq!(Rational::ZERO.abs(), Rational::ZERO);
        assert_eq!(
            Rational::new(5, 7).unwrap().abs(),
            Rational::new(5, 7).unwrap()
        );
    }

    #[test]
    fn clamp_inside_and_outside_bounds() {
        let lo = Rational::new(-1, 1).unwrap();
        let hi = Rational::new(1, 1).unwrap();

        // Inside.
        assert_eq!(
            Rational::new(1, 2).unwrap().clamp(lo, hi),
            Rational::new(1, 2).unwrap()
        );
        assert_eq!(Rational::ZERO.clamp(lo, hi), Rational::ZERO);

        // Below.
        assert_eq!(Rational::new(-5, 1).unwrap().clamp(lo, hi), lo);

        // Above.
        assert_eq!(Rational::new(5, 1).unwrap().clamp(lo, hi), hi);

        // Exactly at bounds.
        assert_eq!(lo.clamp(lo, hi), lo);
        assert_eq!(hi.clamp(lo, hi), hi);
    }

    #[test]
    fn floor_and_ceil() {
        // 7/2 = 3.5 → floor 3, ceil 4
        let r = Rational::new(7, 2).unwrap();
        assert_eq!(r.try_to_i32_floor(), Some(3));
        assert_eq!(r.try_to_i32_ceil(), Some(4));

        // -7/2 = -3.5 → floor -4, ceil -3
        let r = Rational::new(-7, 2).unwrap();
        assert_eq!(r.try_to_i32_floor(), Some(-4));
        assert_eq!(r.try_to_i32_ceil(), Some(-3));

        // Exact integers: floor == ceil.
        let r = Rational::from_int(5);
        assert_eq!(r.try_to_i32_floor(), Some(5));
        assert_eq!(r.try_to_i32_ceil(), Some(5));

        // Zero.
        assert_eq!(Rational::ZERO.try_to_i32_floor(), Some(0));
        assert_eq!(Rational::ZERO.try_to_i32_ceil(), Some(0));
    }

    #[test]
    fn operator_overloads_match_checked() {
        let a = Rational::new(1, 2).unwrap();
        let b = Rational::new(1, 3).unwrap();
        assert_eq!(a + b, a.checked_add(b).unwrap());
        assert_eq!(a - b, a.checked_sub(b).unwrap());
        assert_eq!(a * b, a.checked_mul(b).unwrap());
        assert_eq!(a / b, a.checked_div(b).unwrap());
        assert_eq!(-a, a.checked_neg().unwrap());
    }

    #[test]
    fn hashing_and_eq_use_canonical_form() {
        use core::hash::BuildHasher;
        // We can't easily get a hasher in no_std tests without alloc/std,
        // but extern crate alloc is already in marine-core. Use a tiny
        // hand-rolled equality check that mirrors what HashMap relies on:
        // equal values must hash equal. We just verify Eq sees them as
        // equal — Hash being consistent follows from PartialEq using the
        // same fields as Hash.
        let a = Rational::new(2, 4).unwrap();
        let b = Rational::new(1, 2).unwrap();
        assert_eq!(a, b);
        // And the fields are literally identical post-reduction.
        assert_eq!(a.num, b.num);
        assert_eq!(a.den, b.den);
        let _ = core::hash::BuildHasherDefault::<DummyHasher>::default().build_hasher();
    }

    // Minimal hasher used only to prove BuildHasher path compiles in no_std.
    #[derive(Default)]
    struct DummyHasher(u64);
    impl Hasher for DummyHasher {
        fn finish(&self) -> u64 {
            self.0
        }
        fn write(&mut self, bytes: &[u8]) {
            for b in bytes {
                self.0 = self.0.wrapping_mul(31).wrapping_add(*b as u64);
            }
        }
    }
}
