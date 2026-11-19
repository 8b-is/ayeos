//! Zero-dependency `#![no_std]` signed function capsule (`libayeos_fn_harmonic_fold.so`).
//! Zero C runtime (`-nostdlib`, zero `DT_NEEDED`), zero floating-point arithmetic.

#![no_std]
#![deny(clippy::float_arithmetic)]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}

#[inline(always)]
const fn fold_step(mut acc: u64) -> u64 {
    acc = acc.wrapping_add(432).rotate_left(7) ^ 0x0000_0528_0432_0528;
    acc.wrapping_mul(0x517C_C1B7_2722_0A95) ^ (acc.wrapping_mul(0x517C_C1B7_2722_0A95) >> 13)
}

#[no_mangle]
pub extern "C" fn ayeos_capsule_abi_version() -> u32 {
    1
}

/// 4x-unrolled pure-integer harmonic fold matching `baseline_harmonic_fold(seed, rounds)` bit-for-bit.
#[no_mangle]
pub extern "C" fn ayeos_capsule_eval(seed: i64, rounds: i64) -> i64 {
    let n = if rounds < 1 {
        1u64
    } else if rounds > 100_000 {
        100_000u64
    } else {
        rounds as u64
    };
    let mut acc: u64 = (seed as u64) ^ 0x9E37_79B9_7F4A_7C15;
    let mut rem = n;
    while rem >= 4 {
        acc = fold_step(acc);
        acc = fold_step(acc);
        acc = fold_step(acc);
        acc = fold_step(acc);
        rem -= 4;
    }
    while rem > 0 {
        acc = fold_step(acc);
        rem -= 1;
    }
    acc as i64
}
