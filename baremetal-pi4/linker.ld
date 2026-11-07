/*
 * 8b-is AyeOS Bare-Metal AArch64 Linker Script for Raspberry Pi 4 (BCM2711)
 * Loaded by VideoCore VI start4.elf directly at physical address 0x00080000.
 * Zero Linux kernel · Zero C · 100% Pure Rust.
 */
ENTRY(_start)

SECTIONS
{
    . = 0x00080000;

    .text.boot : {
        KEEP(*(.text.boot))
    }

    .text : ALIGN(16) {
        *(.text .text.*)
    }

    .rodata : ALIGN(16) {
        *(.rodata .rodata.*)
    }

    .data : ALIGN(16) {
        *(.data .data.*)
    }

    .bss (NOLOAD) : ALIGN(16) {
        __bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        . = ALIGN(16);
        __bss_end = .;
    }

    /DISCARD/ : {
        *(.comment)
        *(.eh_frame*)
        *(.note*)
    }
}
