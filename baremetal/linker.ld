/*
 * Bare-Metal AArch64 Linker Script for MediaTek mt6768 / Helio G81 (BLU G75 G1170)
 * Loaded by MediaTek LK at physical DRAM address 0x40080000.
 */
ENTRY(_head)

SECTIONS
{
    . = 0x40080000;
    _kernel_start = .;

    .head.text : {
        KEEP(*(.head.text))
    }

    .text : ALIGN(4096) {
        *(.text._start)
        *(.text .text.*)
    }

    .rodata : ALIGN(4096) {
        *(.rodata .rodata.*)
    }

    .data : ALIGN(4096) {
        *(.data .data.*)
    }

    .bss (NOLOAD) : ALIGN(4096) {
        __bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        . = ALIGN(16);
        __bss_end = .;
    }

    . = ALIGN(4096);
    _stack_bottom = .;
    . += 0x20000; /* 128 KiB bare-metal EL1 stack */
    _stack_top = .;

    _kernel_end = .;
    _kernel_size = _kernel_end - _kernel_start;

    /DISCARD/ : {
        *(.comment)
        *(.eh_frame*)
        *(.note*)
    }
}
