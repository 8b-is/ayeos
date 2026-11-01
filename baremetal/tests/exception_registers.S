// Returns 1 iff all x0..x30, q0..q31, NZCV, FPCR and FPSR survive the production
// BRK exception path. Preserve the caller's AAPCS64 callee-saved state too.
.section .text.test_registers, "ax"
.global test_registers
test_registers:
    sub sp, sp, #1280
    stp x19, x20, [sp]
    stp x21, x22, [sp, #16]
    stp x23, x24, [sp, #32]
    stp x25, x26, [sp, #48]
    stp x27, x28, [sp, #64]
    stp x29, x30, [sp, #80]
    stp d8, d9, [sp, #96]
    stp d10, d11, [sp, #112]
    stp d12, d13, [sp, #128]
    stp d14, d15, [sp, #144]
    mrs x0, fpcr
    mrs x1, fpsr
    stp x0, x1, [sp, #160]
    mov x0, #0xa0000000
    msr nzcv, x0
    mov x0, #0x400000
    msr fpcr, x0
    mov x0, #0x10
    msr fpsr, x0
.irp n,0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31
    movi v\n\().16b, #\n
.endr
.irp n,0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29
    mov x\n, #(0x100+\n)
.endr
    adr x30, 1f
    b ayeos_exception_test
1:
.irp n,0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30
    str x\n, [sp, #(256+8*\n)]
.endr
    add x0, sp, #512
.irp n,0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31
    str q\n, [x0, #(16*\n)]
.endr
    mrs x0, nzcv
    str x0, [sp, #1024]
    mrs x0, fpcr
    str x0, [sp, #1032]
    mrs x0, fpsr
    str x0, [sp, #1040]
    add x0, sp, #256
    mov x1, #0
2:
    ldr x2, [x0, x1, lsl #3]
    add x3, x1, #0x100
    cmp x2, x3
    b.ne 8f
    add x1, x1, #1
    cmp x1, #30
    b.ne 2b
    ldr x2, [sp, #496]
    adr x3, 1b
    cmp x2, x3
    b.ne 8f
    add x0, sp, #512
    mov x1, #0
    mov x4, #0x0101010101010101
3:
    ldp x2, x3, [x0], #16
    mul x5, x1, x4
    cmp x2, x5
    b.ne 8f
    cmp x3, x5
    b.ne 8f
    add x1, x1, #1
    cmp x1, #32
    b.ne 3b
    ldr x2, [sp, #1024]
    mov x3, #0xa0000000
    cmp x2, x3
    b.ne 8f
    ldr x2, [sp, #1032]
    mov x3, #0x400000
    cmp x2, x3
    b.ne 8f
    ldr x2, [sp, #1040]
    cmp x2, #0x10
    b.ne 8f
    mov x0, #1
    b 9f
8:
    mov x0, #0
9:
    ldp x19, x20, [sp]
    ldp x21, x22, [sp, #16]
    ldp x23, x24, [sp, #32]
    ldp x25, x26, [sp, #48]
    ldp x27, x28, [sp, #64]
    ldp x29, x30, [sp, #80]
    ldp d8, d9, [sp, #96]
    ldp d10, d11, [sp, #112]
    ldp d12, d13, [sp, #128]
    ldp d14, d15, [sp, #144]
    ldp x1, x2, [sp, #160]
    msr fpcr, x1
    msr fpsr, x2
    add sp, sp, #1280
    ret
