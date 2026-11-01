// Single boot CPU with DAIF masked. Exception records use volatile raw pointers;
// no reference to this shared state crosses a hardware access. A secondary
// exception halts in assembly without overwriting the original evidence.
#[no_mangle]
static mut AYEOS_IN_EXCEPTION: u64 = 0;
static mut ALPHA_FAULT: diagnostics::Fault = diagnostics::Fault::ZERO;
static mut ALPHA_TEST_ARMED: bool = false;
static mut ALPHA_TEST_SEEN: bool = false;

extern "C" {
    fn ayeos_read8(address: usize) -> u8;
    fn ayeos_read16(address: usize) -> u16;
    fn ayeos_read32(address: usize) -> u32;
    fn ayeos_write8(address: usize, value: u8);
    fn ayeos_write16(address: usize, value: u16);
    fn ayeos_write32(address: usize, value: u32);
    fn ayeos_exception_test();
    fn ayeos_exception_test_resume();
    static __ayeos_fixups_start: diagnostics::Fixup;
    static __ayeos_fixups_end: diagnostics::Fixup;
}
fn safe_mmio_read8(address: usize) -> u8 {
    unsafe { ayeos_read8(address) }
}
fn safe_mmio_read16(address: usize) -> u16 {
    unsafe { ayeos_read16(address) }
}
fn safe_mmio_read32(address: usize) -> u32 {
    unsafe { ayeos_read32(address) }
}
fn safe_mmio_write8(address: usize, value: u8) {
    unsafe {
        ayeos_write8(address, value);
    }
}
fn safe_mmio_write16(address: usize, value: u16) {
    unsafe {
        ayeos_write16(address, value);
    }
}
fn safe_mmio_write32(address: usize, value: u32) {
    unsafe {
        ayeos_write32(address, value);
    }
}

/// Return an exact registered resume PC, or zero to enter the fatal renderer.
#[no_mangle]
unsafe extern "C" fn ayeos_handle_exception(record: *const diagnostics::Fault) -> u64 {
    let mut fault = read_volatile(record);
    let start = core::ptr::addr_of!(__ayeos_fixups_start);
    let end = core::ptr::addr_of!(__ayeos_fixups_end);
    let count = (end as usize - start as usize) / core::mem::size_of::<diagnostics::Fixup>();
    let sites = core::slice::from_raw_parts(start, count);
    let test = diagnostics::Fixup {
        pc: ayeos_exception_test as *const () as u64,
        resume: ayeos_exception_test_resume as *const () as u64,
    };
    let disposition = diagnostics::recovery(
        &fault,
        sites,
        test,
        read_volatile(core::ptr::addr_of!(ALPHA_TEST_ARMED)),
    );
    let (resume, action) = disposition.unwrap_or((0, 3));
    fault.action = action;
    if action == 1 {
        write_volatile(core::ptr::addr_of_mut!(ALPHA_TEST_SEEN), true);
        write_volatile(core::ptr::addr_of_mut!(ALPHA_TEST_ARMED), false);
    } else {
        let count = read_volatile(core::ptr::addr_of!(AYEOS_FAULT_COUNT));
        write_volatile(
            core::ptr::addr_of_mut!(AYEOS_FAULT_COUNT),
            count.saturating_add(1),
        );
    }
    let prior = read_volatile(core::ptr::addr_of!(ALPHA_FAULT));
    // Preserve first unexpected fault; a fatal record takes precedence.
    if prior.action <= 1 || action == 3 {
        write_volatile(core::ptr::addr_of_mut!(ALPHA_FAULT), fault);
    }
    resume
}
