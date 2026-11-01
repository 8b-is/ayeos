//! Architectural test only: production vectors, reset, fixups and Rust handler
//! on QEMU virt. No claim about MT6768 peripheral behavior.
#![no_std]
#![no_main]
#![allow(dead_code)]
use core::arch::asm;
use core::ptr::{read_volatile, write_volatile};
#[path = "../src/diagnostics.rs"]
mod diagnostics;
include!("../src/exception_runtime.rs");
core::arch::global_asm!(include_str!("../src/alpha_start.S"));
core::arch::global_asm!(include_str!("../src/faults.S"));
core::arch::global_asm!(include_str!("exception_registers.S"));
extern "C" {
    fn test_registers() -> u64;
}
#[no_mangle]
static mut AYEOS_FAULT_COUNT: u64 = 0;
#[repr(C, align(4096))]
struct Table([u64; 512]);
static mut TABLE: Table = Table([0; 512]);
static mut EXPECT_FATAL: bool = false;

fn exit(ok: bool) -> ! {
    let message: &[u8] = if ok {
        b"EXCEPTION QEMU PASS\n\0"
    } else {
        b"EXCEPTION QEMU FAIL\n\0"
    };
    let block: [u64; 2] = [0x20026, u64::from(!ok)];
    unsafe {
        asm!("hlt #0xf000", inout("x0") 4u64 => _, in("x1") message.as_ptr(), options(nostack));
        asm!("hlt #0xf000", inout("x0") 0x20u64 => _, in("x1") block.as_ptr(), options(nostack));
    }
    loop {
        core::hint::spin_loop();
    }
}

unsafe fn enable_translation(level: u64) {
    // 4KiB level-1 table, 32-bit VA; map only the 1GiB RAM block at 0x40000000.
    // Address zero is deliberately unmapped for all six exact fixup sites.
    write_volatile(
        core::ptr::addr_of_mut!(TABLE.0[1]),
        0x4000_0000 | (1 << 10) | (3 << 8) | 1,
    );
    let table = core::ptr::addr_of!(TABLE) as u64;
    let mut sctlr: u64;
    if level == 1 {
        asm!("dsb sy", "msr mair_el1, {mair}", "msr ttbr0_el1, {table}",
            "msr tcr_el1, {tcr}", "isb", "tlbi vmalle1", "dsb sy", "isb",
            "mrs {sctlr}, sctlr_el1", "orr {sctlr}, {sctlr}, #1", "msr sctlr_el1, {sctlr}", "isb",
            mair=in(reg) 0xffu64, table=in(reg) table, tcr=in(reg) (32u64 | (1<<8) | (1<<10) | (3<<12) | (1<<23)),
            sctlr=out(reg) sctlr, options(nostack));
    } else {
        asm!("dsb sy", "msr mair_el2, {mair}", "msr ttbr0_el2, {table}",
            "msr tcr_el2, {tcr}", "isb", "tlbi alle2", "dsb sy", "isb",
            "mrs {sctlr}, sctlr_el2", "orr {sctlr}, {sctlr}, #1", "msr sctlr_el2, {sctlr}", "isb",
            mair=in(reg) 0xffu64, table=in(reg) table, tcr=in(reg) (32u64 | (1<<8) | (1<<10) | (3<<12) | (1<<23) | (1<<31)),
            sctlr=out(reg) sctlr, options(nostack));
    }
    let _ = sctlr;
}

#[no_mangle]
pub extern "C" fn rust_baremetal_main(_: u64) -> ! {
    unsafe {
        let level: u64;
        asm!("mrs {}, CurrentEL", out(reg) level, options(nomem, nostack));
        let level = level >> 2;
        write_volatile(core::ptr::addr_of_mut!(ALPHA_TEST_ARMED), true);
        if test_registers() != 1 {
            exit(false);
        }
        let f = read_volatile(core::ptr::addr_of!(ALPHA_FAULT));
        if !read_volatile(core::ptr::addr_of!(ALPHA_TEST_SEEN))
            || f.action != 1
            || f.level != level
            || f.pc != ayeos_exception_test as *const () as u64
            || f.vector != 4
            || read_volatile(core::ptr::addr_of!(AYEOS_FAULT_COUNT)) != 0
        {
            exit(false);
        }
        enable_translation(level);
        if safe_mmio_read8(0) != 0 || safe_mmio_read16(0) != 0 || safe_mmio_read32(0) != 0 {
            exit(false);
        }
        safe_mmio_write8(0, 23);
        safe_mmio_write16(0, 23);
        safe_mmio_write32(0, 23);
        let f = read_volatile(core::ptr::addr_of!(ALPHA_FAULT));
        if read_volatile(core::ptr::addr_of!(AYEOS_FAULT_COUNT)) != 6
            || f.action != 2
            || f.far != 0
            || f.esr >> 26 != 0x25
            || f.level != level
        {
            exit(false);
        }
        write_volatile(core::ptr::addr_of_mut!(EXPECT_FATAL), true);
        // The one-shot breakpoint or an ordinary unregistered fault MUST halt.
        #[cfg(not(fatal_load))]
        ayeos_exception_test();
        #[cfg(fatal_load)]
        asm!("mov x0, xzr", "ldr x0, [x0]", out("x0") _, options(nostack));
        exit(false);
    }
}
#[no_mangle]
extern "C" fn ayeos_fatal_halt() -> ! {
    unsafe {
        let f = read_volatile(core::ptr::addr_of!(ALPHA_FAULT));
        let expected_ec = if cfg!(fatal_load) { 0x25 } else { 0x3c };
        exit(
            read_volatile(core::ptr::addr_of!(EXPECT_FATAL))
                && f.action == 3
                && f.esr >> 26 == expected_ec
                && read_volatile(core::ptr::addr_of!(AYEOS_FAULT_COUNT)) == 7,
        );
    }
}
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    exit(false)
}
