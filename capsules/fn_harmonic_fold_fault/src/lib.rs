//! Negative-test `#![no_std]` function capsule for acceptance testing:
//! - Normal inputs (including all 64 equivalence vectors) match baseline bit-for-bit.
//! - Sentinel `seed == 0x0000_0000_DEAD_BEEF` triggers `ud2` (SIGILL crash) -> tests auto-rollback on worker crash.
//! - Sentinel `seed == 0x0000_0000_71DE_0071` spins forever -> tests parent wall-clock deadline SIGKILL + auto-rollback.
//! - Sentinel `seed == 0x0000_0000_BAD0_05CA` executes raw Linux `SYS_socket` (41) -> tests Stage-2 seccomp-bpf kill + auto-rollback.

#![no_std]
#![deny(clippy::float_arithmetic)]

use core::panic::PanicInfo;

pub const SENTINEL_CRASH: i64 = 0x0000_0000_DEAD_BEEF;
pub const SENTINEL_TIMEOUT: i64 = 0x0000_0000_71DE_0071;
pub const SENTINEL_SECCOMP_ESCAPE: i64 = 0x0000_0000_BAD0_05CA;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}

#[inline(always)]
const fn fold_step(mut acc: u64) -> u64 {
    acc = acc.wrapping_add(432).rotate_left(7) ^ 0x0000_0528_0432_0528;
    let mul = acc.wrapping_mul(0x517C_C1B7_2722_0A95);
    mul ^ (mul >> 13)
}

#[no_mangle]
pub extern "C" fn ayeos_capsule_abi_version() -> u32 {
    1
}

#[no_mangle]
pub extern "C" fn ayeos_capsule_eval(seed: i64, rounds: i64) -> i64 {
    if seed == SENTINEL_CRASH {
        #[cfg(target_arch = "x86_64")]
        unsafe {
            core::arch::asm!("ud2", options(noreturn));
        }
    }
    if seed == SENTINEL_TIMEOUT {
        loop {
            core::hint::spin_loop();
        }
    }
    if seed == SENTINEL_SECCOMP_ESCAPE {
        #[cfg(target_arch = "x86_64")]
        unsafe {
            // Attempt forbidden SYS_socket(AF_INET=2, SOCK_STREAM=1, 0) = syscall 41
            let _ret: i64;
            core::arch::asm!(
                "syscall",
                inlateout("rax") 41i64 => _ret,
                in("rdi") 2i64,
                in("rsi") 1i64,
                in("rdx") 0i64,
                lateout("rcx") _,
                lateout("r11") _,
            );
        }
    }

    let n = if rounds < 1 {
        1u64
    } else if rounds > 100_000 {
        100_000u64
    } else {
        rounds as u64
    };
    let mut acc: u64 = (seed as u64) ^ 0x9E37_79B9_7F4A_7C15;
    let mut i = 0u64;
    while i < n {
        acc = fold_step(acc);
        i += 1;
    }
    acc as i64
}
