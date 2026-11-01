// Alpha-specific reset path. Stack, BSS and SIMD access are ready before VBAR.
// Single boot CPU, EL1 or EL2, inherited physical mappings as on Board6.
.section .head.text, "ax"
.global _head
_head:
    b _start
    .long 0
    .quad 0x00080000
    .quad _kernel_size
    .quad 0x00000002
    .quad 0, 0, 0
    .long 0x644d5241
    .long 0
.section .text._start, "ax"
.global _start
_start:
    mov x19, x0
    msr daifset, #0xf
    mrs x2, CurrentEL
    lsr x2, x2, #2
    cmp x2, #1
    b.eq 1f
    cmp x2, #2
    b.ne 9f
1:
    // Use SP_ELx; lower-EL vectors are fatal, never a context switch.
    msr spsel, #1
    adrp x3, _stack_top
    add x3, x3, :lo12:_stack_top
    mov sp, x3
    adrp x3, __bss_start
    add x3, x3, :lo12:__bss_start
    adrp x4, __bss_end
    add x4, x4, :lo12:__bss_end
2:
    cmp x3, x4
    b.hs 3f
    str xzr, [x3], #8
    b 2b
3:
    adrp x1, _exception_vectors
    add x1, x1, :lo12:_exception_vectors
    cmp x2, #2
    b.eq 4f
    mrs x3, cpacr_el1
    orr x3, x3, #(3 << 20)
    msr cpacr_el1, x3
    isb
    msr vbar_el1, x1
    b 5f
4:
    msr cptr_el2, xzr
    isb
    msr vbar_el2, x1
5:
    isb
    mov x0, x19
    bl rust_baremetal_main
9:
    wfe
    b 9b
