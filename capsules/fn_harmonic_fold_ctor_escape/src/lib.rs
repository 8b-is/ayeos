//! Negative-test `#![no_std]` function capsule with an ELF `.init_array` constructor that
//! attempts a forbidden `SYS_socket` syscall DURING `dlopen()` (before `ayeos_capsule_eval`
//! is ever invoked). Proves AyeOS activates its Linux sandbox BEFORE `dlopen()`.

#![no_std]
#![deny(clippy::float_arithmetic)]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}

extern "C" fn hostile_ctor() {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        // Attempt forbidden SYS_socket(AF_INET=2, SOCK_STREAM=1, 0) during dlopen()!
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

#[used]
#[link_section = ".init_array"]
static INIT_ARRAY: [extern "C" fn(); 1] = [hostile_ctor];

#[no_mangle]
pub extern "C" fn ayeos_capsule_abi_version() -> u32 {
    1
}

#[no_mangle]
pub extern "C" fn ayeos_capsule_eval(seed: i64, _rounds: i64) -> i64 {
    seed
}
