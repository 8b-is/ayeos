// AAPCS64 exception entry: preserve all GPRs and SIMD registers before Rust.
// IRQs remain masked; one boot CPU owns the diagnostic state. This is not an
// exception-stack implementation: a corrupt/unmapped SP cannot be rescued here.
.section .text.ayeos_vectors, "ax"
.balign 2048
.global _exception_vectors
_exception_vectors:
.set slot, 0
.rept 16
    sub sp, sp, #848
    stp x0, x1, [sp]
    mov x0, #slot
    b ayeos_vector_common
    .balign 128
    .set slot, slot + 1
.endr

ayeos_vector_common:
    // Detect a secondary fault before touching SIMD or entering Rust again.
    // x0 is the slot; x1 was saved in the entry stub.
    adrp x1, AYEOS_IN_EXCEPTION
    add x1, x1, :lo12:AYEOS_IN_EXCEPTION
    str x0, [sp, #768]
    ldr x0, [x1]
    cbnz x0, ayeos_nested_halt
    mov x0, #1
    str x0, [x1]
    stp x2, x3, [sp, #16]
    stp x4, x5, [sp, #32]
    stp x6, x7, [sp, #48]
    stp x8, x9, [sp, #64]
    stp x10, x11, [sp, #80]
    stp x12, x13, [sp, #96]
    stp x14, x15, [sp, #112]
    stp x16, x17, [sp, #128]
    stp x18, x19, [sp, #144]
    stp x20, x21, [sp, #160]
    stp x22, x23, [sp, #176]
    stp x24, x25, [sp, #192]
    stp x26, x27, [sp, #208]
    stp x28, x29, [sp, #224]
    str x30, [sp, #240]
    stp q0, q1, [sp, #256]
    stp q2, q3, [sp, #288]
    stp q4, q5, [sp, #320]
    stp q6, q7, [sp, #352]
    stp q8, q9, [sp, #384]
    stp q10, q11, [sp, #416]
    stp q12, q13, [sp, #448]
    stp q14, q15, [sp, #480]
    stp q16, q17, [sp, #512]
    stp q18, q19, [sp, #544]
    stp q20, q21, [sp, #576]
    stp q22, q23, [sp, #608]
    stp q24, q25, [sp, #640]
    stp q26, q27, [sp, #672]
    stp q28, q29, [sp, #704]
    stp q30, q31, [sp, #736]
    mrs x0, fpcr
    mrs x1, fpsr
    str x0, [sp, #832]
    str x1, [sp, #840]
    mrs x1, CurrentEL
    lsr x1, x1, #2
    str x1, [sp, #776]
    cmp x1, #2
    b.eq 2f
    mrs x2, esr_el1
    mrs x3, elr_el1
    mrs x4, far_el1
    mrs x5, spsr_el1
    b 3f
2:
    mrs x2, esr_el2
    mrs x3, elr_el2
    mrs x4, far_el2
    mrs x5, spsr_el2
3:
    str x2, [sp, #784]
    str x3, [sp, #792]
    str x4, [sp, #800]
    str x5, [sp, #808]
    add x6, sp, #848
    str x6, [sp, #816]
    str xzr, [sp, #824]
    add x0, sp, #768
    bl ayeos_handle_exception
    cbz x0, 5f
    ldr x1, [sp, #776]
    cmp x1, #2
    b.eq 4f
    msr elr_el1, x0
    b ayeos_vector_restore
4:
    msr elr_el2, x0
ayeos_vector_restore:
    ldr x0, [sp, #832]
    ldr x1, [sp, #840]
    msr fpcr, x0
    msr fpsr, x1
    ldp q0, q1, [sp, #256]
    ldp q2, q3, [sp, #288]
    ldp q4, q5, [sp, #320]
    ldp q6, q7, [sp, #352]
    ldp q8, q9, [sp, #384]
    ldp q10, q11, [sp, #416]
    ldp q12, q13, [sp, #448]
    ldp q14, q15, [sp, #480]
    ldp q16, q17, [sp, #512]
    ldp q18, q19, [sp, #544]
    ldp q20, q21, [sp, #576]
    ldp q22, q23, [sp, #608]
    ldp q24, q25, [sp, #640]
    ldp q26, q27, [sp, #672]
    ldp q28, q29, [sp, #704]
    ldp q30, q31, [sp, #736]
    ldp x2, x3, [sp, #16]
    ldp x4, x5, [sp, #32]
    ldp x6, x7, [sp, #48]
    ldp x8, x9, [sp, #64]
    ldp x10, x11, [sp, #80]
    ldp x12, x13, [sp, #96]
    ldp x14, x15, [sp, #112]
    ldp x16, x17, [sp, #128]
    ldp x18, x19, [sp, #144]
    ldp x20, x21, [sp, #160]
    ldp x22, x23, [sp, #176]
    ldp x24, x25, [sp, #192]
    ldp x26, x27, [sp, #208]
    ldp x28, x29, [sp, #224]
    ldr x30, [sp, #240]
    adrp x0, AYEOS_IN_EXCEPTION
    add x0, x0, :lo12:AYEOS_IN_EXCEPTION
    str xzr, [x0]
    ldp x0, x1, [sp]
    add sp, sp, #848
    eret
5:
    bl ayeos_fatal_halt
ayeos_nested_halt:
    wfe
    b ayeos_nested_halt

// Six exact fault sites. On failed reads w0 retains its initialized zero;
// callers must inspect the fault counter before interpreting any observation.
.macro read_site name, insn
.global \name
\name:
    mov x1, x0
    mov w0, wzr
1:  \insn w0, [x1]
2:  ret
.pushsection .ayeos_fixups, "a"
.balign 8
    .quad 1b, 2b
.popsection
.endm
.macro write_site name, insn
.global \name
\name:
1:  \insn w1, [x0]
2:  ret
.pushsection .ayeos_fixups, "a"
.balign 8
    .quad 1b, 2b
.popsection
.endm
read_site ayeos_read8, ldrb
read_site ayeos_read16, ldrh
read_site ayeos_read32, ldr
write_site ayeos_write8, strb
write_site ayeos_write16, strh
write_site ayeos_write32, str

.global ayeos_exception_test
.global ayeos_exception_test_resume
ayeos_exception_test:
    brk #0xa8
ayeos_exception_test_resume:
    ret
