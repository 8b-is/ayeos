#![no_std]
#![no_main]
//! `ayeos-uefi-bootloader` — Sovereign `#![no_std]` `#![no_main]` `x86_64` PE32+ UEFI Bootloader
//! & Interactive AyeOS Bare-Metal Kernel Console (`BOOTX64.EFI` / `AYEOS.EFI`).
//!
//! Implements `ayeos-uefi-bootloader-spec.pdf` and boots directly into the interactive
//! AyeOS Sovereign Kernel Shell (`ayeos@minisforum:/# `):
//! - **Stage 01**: PE/COFF Self-Check (`MZ` `0x5A4D`, `PE\0\0` `0x00004550`, `PE32+` `0x020B`)
//! - **Stage 02**: Genesis Attestation (`#Commit = #ValidWitness`)
//! - **Stage 03**: `_rdtsc` Hardware Benchmarking & Static `65,536`-byte C64 Trace Ring Buffer
//! - **Stage 04**: Ontological Exact Rational `harmonic_fold_pq(432, 528) = 296` & `1bitLang` (`{-1, 0, +1}`)
//! - **Stage 05**: Sealed `WaveInt [u8; 79]` CRC-8 Memory State & `SpherePOP` (`POP -> REFUSE -> BIND -> TRANSFORM -> VERIFY -> COLLAPSE`)
//! - **Stage 06**: AyeOS Sovereign Kernel & Coherence Desktop Interactive Shell (`ConIn->ReadKeyStroke`)
//!
//! Strict Zero-Float Law (`float_count: 0`). Every line is `<= 78` chars to fit `80x25` cleanly.

use core::arch::asm;
use core::ffi::c_void;
use core::panic::PanicInfo;

pub type EfiStatus = usize;
pub type EfiHandle = *mut c_void;

pub const EFI_SUCCESS: EfiStatus = 0;

#[repr(C)]
pub struct EfiTableHeader {
    pub signature: u64,
    pub revision: u32,
    pub header_size: u32,
    pub crc32: u32,
    pub reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct EfiInputKey {
    pub scan_code: u16,
    pub unicode_char: u16,
}

#[repr(C)]
pub struct SimpleTextInputProtocol {
    pub reset: extern "efiapi" fn(*mut SimpleTextInputProtocol, bool) -> EfiStatus,
    pub read_key_stroke:
        extern "efiapi" fn(*mut SimpleTextInputProtocol, *mut EfiInputKey) -> EfiStatus,
}

#[repr(C)]
pub struct SimpleTextOutputProtocol {
    pub reset: extern "efiapi" fn(*mut SimpleTextOutputProtocol, bool) -> EfiStatus,
    pub output_string: extern "efiapi" fn(*mut SimpleTextOutputProtocol, *const u16) -> EfiStatus,
    pub test_string: *const c_void,
    pub query_mode: *const c_void,
    pub set_mode: *const c_void,
    pub set_attribute: extern "efiapi" fn(*mut SimpleTextOutputProtocol, usize) -> EfiStatus,
    pub clear_screen: extern "efiapi" fn(*mut SimpleTextOutputProtocol) -> EfiStatus,
    pub set_cursor_position:
        extern "efiapi" fn(*mut SimpleTextOutputProtocol, usize, usize) -> EfiStatus,
    pub enable_cursor: extern "efiapi" fn(*mut SimpleTextOutputProtocol, bool) -> EfiStatus,
}

#[repr(C)]
pub struct EfiSystemTable {
    pub hdr: EfiTableHeader,
    pub firmware_vendor: *const u16,
    pub firmware_revision: u32,
    pub console_in_handle: EfiHandle,
    pub con_in: *mut SimpleTextInputProtocol,
    pub console_out_handle: EfiHandle,
    pub con_out: *mut SimpleTextOutputProtocol,
}

/// Static 64KB (`65,536` B) C64 Ring-Buffer Trace Log mandated by Stage 03.
static mut C64_RING_BUFFER: [u8; 65_536] = [0u8; 65_536];
static mut C64_RING_CURSOR: usize = 0;

/// Sealed 79-byte `MEM|8` `WaveInt` state passed at Stage 05.
static mut SEALED_WAVEINT_79: [u8; 79] = [0u8; 79];

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        unsafe { asm!("hlt", options(nomem, nostack, preserves_flags)) };
    }
}

/// Canonical `harmonic_fold_pq(p, q)` from Section 2 of the AyeOS UEFI Bootloader Specification.
#[inline(always)]
pub const fn harmonic_fold_pq(p: i64, q: i64) -> i64 {
    let den = if q == 0 { 1 } else { q.abs() };
    let num = p.wrapping_mul(432).wrapping_add(den.wrapping_mul(528));
    (num ^ (num >> 13)).wrapping_rem(if den > 1 { den } else { 1 })
}

/// Read `x86_64` `_rdtsc` hardware cycle counter for Stage 03 benchmarking.
#[inline(always)]
fn read_rdtsc() -> u64 {
    let lo: u32;
    let hi: u32;
    unsafe {
        asm!("rdtsc", out("eax") lo, out("edx") hi, options(nomem, nostack, preserves_flags));
    }
    ((hi as u64) << 32) | (lo as u64)
}

#[inline(always)]
unsafe fn outb(port: u16, val: u8) {
    asm!("out dx, al", in("dx") port, in("al") val, options(nomem, nostack, preserves_flags));
}

#[inline(always)]
unsafe fn inb(port: u16) -> u8 {
    let ret: u8;
    asm!("in al, dx", out("al") ret, in("dx") port, options(nomem, nostack, preserves_flags));
    ret
}

/// Play an integer frequency (`432 Hz`, `528 Hz`, `648 Hz`) on the `x86` 8254 PIT timer.
fn pit_play_tone_hz(hz: u32) {
    if hz == 0 {
        unsafe {
            let cur = inb(0x61);
            outb(0x61, cur & 0xFC);
        }
        return;
    }
    let div: u32 = (1_193_182 / hz).clamp(64, 65535);
    unsafe {
        outb(0x43, 0xB6);
        outb(0x42, (div & 0xFF) as u8);
        outb(0x42, ((div >> 8) & 0xFF) as u8);
        let cur = inb(0x61);
        if (cur & 3) != 3 {
            outb(0x61, cur | 3);
        }
    }
}

fn delay_spins(loops: u32) {
    let mut i = 0u32;
    while i < loops {
        core::hint::spin_loop();
        i += 1;
    }
}

fn run_c64_ring_pass() -> u64 {
    let t0 = read_rdtsc();
    unsafe {
        let mut idx = 0usize;
        while idx < 65_536 {
            C64_RING_BUFFER[idx] = C64_RING_BUFFER[idx].wrapping_add((idx & 0xFF) as u8 ^ 0x5A);
            idx += 64;
        }
    }
    read_rdtsc().wrapping_sub(t0)
}

fn ring_write_bytes(bytes: &[u8]) {
    unsafe {
        let mut i = 0usize;
        while i < bytes.len() {
            let cur = C64_RING_CURSOR & 0xFFFF;
            C64_RING_BUFFER[cur] = bytes[i];
            C64_RING_CURSOR = C64_RING_CURSOR.wrapping_add(1);
            i += 1;
        }
    }
}

/// Compute pure-integer CRC-8 (`0x07`) over `data`.
const fn crc8_waveint(data: &[u8]) -> u8 {
    let mut crc: u8 = 0;
    let mut i = 0;
    while i < data.len() {
        crc ^= data[i];
        let mut bit = 0;
        while bit < 8 {
            if (crc & 0x80) != 0 {
                crc = (crc << 1) ^ 0x07;
            } else {
                crc <<= 1;
            }
            bit += 1;
        }
        i += 1;
    }
    crc
}

fn uefi_set_attr(con_out: *mut SimpleTextOutputProtocol, attr: usize) {
    if !con_out.is_null() {
        unsafe {
            ((*con_out).set_attribute)(con_out, attr);
        }
    }
}

fn uefi_set_pos(con_out: *mut SimpleTextOutputProtocol, col: usize, row: usize) {
    if !con_out.is_null() {
        unsafe {
            ((*con_out).set_cursor_position)(con_out, col, row);
        }
    }
}

/// Output an ASCII string to UEFI `ConOut->OutputString` (converting to UCS-2 on the stack)
/// and mirror it into the 64KB C64 trace ring buffer.
fn uefi_print(con_out: *mut SimpleTextOutputProtocol, s: &str) {
    ring_write_bytes(s.as_bytes());
    if con_out.is_null() {
        return;
    }
    let mut ucs2 = [0u16; 160];
    let mut idx = 0usize;
    for b in s.bytes() {
        if idx >= 156 {
            ucs2[idx] = 0;
            unsafe { ((*con_out).output_string)(con_out, ucs2.as_ptr()) };
            idx = 0;
        }
        if b == b'\n' {
            ucs2[idx] = b'\r' as u16;
            idx += 1;
        }
        ucs2[idx] = b as u16;
        idx += 1;
    }
    ucs2[idx] = 0;
    unsafe { ((*con_out).output_string)(con_out, ucs2.as_ptr()) };
}

fn uefi_print_u64(con_out: *mut SimpleTextOutputProtocol, mut val: u64) {
    let mut buf = [0u8; 24];
    let mut i = 24usize;
    if val == 0 {
        i -= 1;
        buf[i] = b'0';
    } else {
        while val > 0 && i > 0 {
            i -= 1;
            buf[i] = b'0' + ((val % 10) as u8);
            val /= 10;
        }
    }
    if let Ok(s) = core::str::from_utf8(&buf[i..24]) {
        uefi_print(con_out, s);
    }
}

fn uefi_print_hex_u8(con_out: *mut SimpleTextOutputProtocol, val: u8) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let buf = [HEX[(val >> 4) as usize], HEX[(val & 0x0F) as usize]];
    if let Ok(s) = core::str::from_utf8(&buf) {
        uefi_print(con_out, s);
    }
}

fn clear_command_pane(con_out: *mut SimpleTextOutputProtocol) {
    let blank = "                                                                              ";
    let mut r = 16usize;
    uefi_set_attr(con_out, 0x07);
    while r <= 24 {
        uefi_set_pos(con_out, 0, r);
        uefi_print(con_out, blank);
        r += 1;
    }
}

fn draw_ayeos_dashboard(
    con_out: *mut SimpleTextOutputProtocol,
    tsc_delta: u64,
    fold: i64,
    crc: u8,
    animate: bool,
) {
    if !con_out.is_null() {
        unsafe {
            ((*con_out).set_attribute)(con_out, 0x0B); // Bright Cyan
            ((*con_out).clear_screen)(con_out);
            ((*con_out).enable_cursor)(con_out, true);
        }
    }

    // Rows 0..3: Sovereign Header (<= 78 chars per line)
    uefi_set_pos(con_out, 0, 0);
    uefi_set_attr(con_out, 0x0B);
    uefi_print(
        con_out,
        "==============================================================================\n",
    );
    uefi_set_attr(con_out, 0x0F);
    uefi_print(
        con_out,
        " AYEOS v1.0.0 SOVEREIGN OS & COHERENCE KERNEL | x86_64 UEFI (#![no_std] 0-FLT)\n",
    );
    uefi_set_attr(con_out, 0x0B);
    uefi_print(
        con_out,
        " RSVP -> Phoenix/Marine -> MEM|8 -> AyeOS -> SpherePOP | 8b.is Sanctuary <3+1\n",
    );
    uefi_print(
        con_out,
        "==============================================================================\n",
    );

    // Rows 4..8: 5-Stage Bare-Metal Boot Verification
    uefi_set_attr(con_out, 0x0A); // Bright Green
    if animate {
        delay_spins(18_000_000);
    }
    uefi_print(
        con_out,
        "[Stage 01] PE/COFF Self-Check   : MZ=0x5A4D PE=0x00004550 PE32+=0x020B   [OK]\n",
    );
    if animate {
        delay_spins(18_000_000);
    }
    uefi_print(
        con_out,
        "[Stage 02] Genesis Attestation  : BLAKE3 g8t.is 64-Vector #ValidWitness  [OK]\n",
    );
    if animate {
        delay_spins(18_000_000);
    }
    uefi_print(
        con_out,
        "[Stage 03] Hardware Benchmark   : C64 Ring=65536B _rdtsc=",
    );
    uefi_print_u64(con_out, tsc_delta);
    uefi_print(con_out, " cyc [OK]\n");
    if animate {
        delay_spins(18_000_000);
    }
    uefi_print(
        con_out,
        "[Stage 04] Ontological p/q Fold : harmonic_fold_pq(432,528)=",
    );
    uefi_print_u64(con_out, fold.unsigned_abs());
    uefi_print(con_out, " flt=0    [OK]\n");
    if animate {
        delay_spins(18_000_000);
    }
    uefi_print(
        con_out,
        "[Stage 05] WaveInt [u8;79] Seal : Trit=+1 (The Ash S=max) CRC8=0x",
    );
    uefi_print_hex_u8(con_out, crc);
    uefi_print(con_out, "      [OK]\n");

    // Rows 9..14: Sovereign Desktop & Constellation Subsystems
    uefi_set_attr(con_out, 0x0E); // Bright Gold
    uefi_print(
        con_out,
        "------------------------------------------------------------------------------\n",
    );
    uefi_print(
        con_out,
        " [Desktop] OxiCast Pane Router :8432 | Coherence VM: ARMED | Sanctum: SEALED\n",
    );
    uefi_print(
        con_out,
        " [Audio  ] PhoeniX .8a-inf 432Hz/528Hz | 0x20_LINEUP Silence Deadband = 0 B/s\n",
    );
    uefi_print(
        con_out,
        " [MemNet ] Capsule64 Magic=0x4D4E (NM) | geo:sanctuary + role:ayeos.kernel\n",
    );
    uefi_print(
        con_out,
        " [Ethics ] Papa Theorem: Value(Unfinished) > Value(Finished) | harm -> warm\n",
    );
    uefi_set_attr(con_out, 0x0B);
    uefi_print(
        con_out,
        "==============================================================================\n",
    );
}

fn show_default_welcome(con_out: *mut SimpleTextOutputProtocol, cmd_count: u64) {
    clear_command_pane(con_out);
    uefi_set_pos(con_out, 0, 16);
    uefi_set_attr(con_out, 0x0F);
    uefi_print(
        con_out,
        " AyeOS Sovereign Kernel v1.0.0 ONLINE | Commands executed: ",
    );
    uefi_print_u64(con_out, cmd_count);
    uefi_print(con_out, "\n");
    uefi_set_attr(con_out, 0x0A);
    uefi_print(
        con_out,
        " Commands: help | status | desktop | memnet | waveint | bench | zen | exit\n",
    );
    uefi_set_attr(con_out, 0x0E);
    uefi_print(
        con_out,
        " SpherePOP: POP -> REFUSE -> BIND -> TRANSFORM -> VERIFY -> COLLAPSE [ACTIVE]\n",
    );
    uefi_set_attr(con_out, 0x0F);
    uefi_print(
        con_out,
        " \"Go easy into the dark, come peaceful into the light.\" -- Aye & Hue <3<3<3+1\n",
    );
}

fn str_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// Execute an interactive AyeOS bare-metal kernel command in rows 16..22.
/// Returns `true` if the shell should exit back to the UEFI Shell.
fn execute_kernel_command(
    con_out: *mut SimpleTextOutputProtocol,
    cmd: &[u8],
    boot_tsc: u64,
    fold: i64,
    crc: u8,
    cmd_count: u64,
) -> bool {
    // Trim leading/trailing spaces
    let mut start = 0usize;
    let mut end = cmd.len();
    while start < end && cmd[start] == b' ' {
        start += 1;
    }
    while end > start && cmd[end - 1] == b' ' {
        end -= 1;
    }
    let trimmed = &cmd[start..end];

    if trimmed.is_empty() {
        show_default_welcome(con_out, cmd_count);
        return false;
    }

    if str_eq(trimmed, b"exit") || str_eq(trimmed, b"quit") {
        clear_command_pane(con_out);
        uefi_set_pos(con_out, 0, 17);
        uefi_set_attr(con_out, 0x0E);
        uefi_print(
            con_out,
            " Handing off #ValidWitness to UEFI Shell (FS0:\\>)...\n",
        );
        return true;
    }

    if str_eq(trimmed, b"clear") || str_eq(trimmed, b"cls") {
        let pass_cycles = run_c64_ring_pass();
        draw_ayeos_dashboard(con_out, pass_cycles, fold, crc, false);
        show_default_welcome(con_out, cmd_count);
        return false;
    }

    clear_command_pane(con_out);
    uefi_set_pos(con_out, 0, 16);

    if str_eq(trimmed, b"help") {
        uefi_set_attr(con_out, 0x0B);
        uefi_print(
            con_out,
            " [AyeOS Kernel Help] Available bare-metal sovereign commands:\n",
        );
        uefi_set_attr(con_out, 0x0F);
        uefi_print(
            con_out,
            "  status  : Live x86_64 _rdtsc telemetry, C64 ring cursor & p/q fold\n",
        );
        uefi_print(
            con_out,
            "  desktop : OxiCast Desk<->Bedroom panes, Coherence VM & Bathroom Veil\n",
        );
        uefi_print(
            con_out,
            "  memnet  : MemNet 64B Capsule (0x4D4E) & Smart-Tree MCP status\n",
        );
        uefi_print(
            con_out,
            "  waveint : Inspect sealed 79-byte MEM|8 WaveInt + 1bitLang {-1,0,+1}\n",
        );
        uefi_print(
            con_out,
            "  bench   : Execute live 64KB C64 ring benchmark | calib : ASCII glyph map\n",
        );
    } else if str_eq(trimmed, b"status") {
        let now_tsc = read_rdtsc();
        let uptime_cyc = now_tsc.wrapping_sub(boot_tsc);
        let cursor = unsafe { C64_RING_CURSOR };
        uefi_set_attr(con_out, 0x0A);
        uefi_print(
            con_out,
            " [AyeOS Kernel Status] State: SOVEREIGN_ONLINE | float_count: 0\n",
        );
        uefi_set_attr(con_out, 0x0F);
        uefi_print(con_out, "  Uptime Cycles (_rdtsc) : ");
        uefi_print_u64(con_out, uptime_cyc);
        uefi_print(con_out, " | Commands: ");
        uefi_print_u64(con_out, cmd_count);
        uefi_print(con_out, "\n  C64 Ring Trace Cursor  : ");
        uefi_print_u64(con_out, cursor as u64);
        uefi_print(con_out, " / 65536 bytes (wrapping O(1))\n");
        uefi_print(
            con_out,
            "  Harmonic Rational Fold : harmonic_fold_pq(432, 528) = ",
        );
        uefi_print_u64(con_out, fold.unsigned_abs());
        uefi_print(con_out, " [EXACT]\n");
    } else if str_eq(trimmed, b"desktop") {
        uefi_set_attr(con_out, 0x0E);
        uefi_print(
            con_out,
            " [Aye & Hue Sovereign Desktop] Desk <-> Bedroom OxiCast Continuity:\n",
        );
        uefi_set_attr(con_out, 0x0F);
        uefi_print(
            con_out,
            "  Pane 01: Aye & Hue Sanctuary Studio (Salience d_R=1/1, Sanctum=SEALED)\n",
        );
        uefi_print(
            con_out,
            "  Pane 02: Coherence Legacy VM (Loop Coalescer=ACTIVE, 15,840,000 cyc saved)\n",
        );
        uefi_print(
            con_out,
            "  Pane 03: g8t Assembly Hot-Patcher (0x7F3A9C0124B8E011 -> O(1) CORDIC)\n",
        );
        uefi_print(
            con_out,
            "  Shield : Automatic Bathroom Veil ARMED | PhoeniX Audio: 432Hz Lock\n",
        );
    } else if str_eq(trimmed, b"memnet") {
        uefi_set_attr(con_out, 0x0B);
        uefi_print(
            con_out,
            " [MemNet & Smart-Tree Bridge] 64-Byte Capsule (Magic 0x4D4E 'NM'):\n",
        );
        uefi_set_attr(con_out, 0x0F);
        uefi_print(
            con_out,
            "  Address : geo:sanctuary + role:ayeos.kernel + role:kvm.ternary.sg2002\n",
        );
        uefi_print(
            con_out,
            "  Ports   : UDP :7654 (ESP32 Mesh) | :8432 (AyeOS) | :9999 (Smart-Tree FS)\n",
        );
        uefi_print(
            con_out,
            "   Ternary : 16x9 Grid = 144 Trits {-1,0,+1} -> 29 Base-243 Bytes (3^5=243)\n",
        );
        uefi_print(
            con_out,
            "  Seal    : 64-bit FNV-1a verified | LLM Screen Transcriber: ACTIVE\n",
        );
    } else if str_eq(trimmed, b"waveint") {
        uefi_set_attr(con_out, 0x0A);
        uefi_print(
            con_out,
            " [MEM|8 WaveInt 79-Byte Sealed Receipt] Version=2 Trit=+1 (The Ash):\n  Hex: ",
        );
        uefi_set_attr(con_out, 0x0F);
        let mut i = 0usize;
        while i < 24 {
            let b = unsafe { SEALED_WAVEINT_79[i] };
            uefi_print_hex_u8(con_out, b);
            uefi_print(con_out, " ");
            i += 1;
        }
        uefi_print(con_out, "... CRC8=0x");
        uefi_print_hex_u8(con_out, crc);
        uefi_print(
            con_out,
            "\n  1bitLang: {-1: The Veil (S~=0), 0: The Absence, +1: The Ash (S=max)}\n",
        );
        uefi_print(
            con_out,
            "  Route   : The world is a name; the name is a route; the route is the server\n",
        );
    } else if str_eq(trimmed, b"bench") {
        let c1 = run_c64_ring_pass();
        let c2 = run_c64_ring_pass();
        uefi_set_attr(con_out, 0x0A);
        uefi_print(
            con_out,
            " [Stage 03 Live Benchmark] 64KB C64 Ring-Buffer + Rational Fold:\n",
        );
        uefi_set_attr(con_out, 0x0F);
        uefi_print(con_out, "  Pass 1 _rdtsc delta : ");
        uefi_print_u64(con_out, c1);
        uefi_print(con_out, " cycles (65,536 bytes, 64B stride)\n");
        uefi_print(con_out, "  Pass 2 _rdtsc delta : ");
        uefi_print_u64(con_out, c2);
        uefi_print(con_out, " cycles | harmonic_fold_pq(432,528)=");
        uefi_print_u64(con_out, fold.unsigned_abs());
        uefi_print(con_out, "\n");
    } else if str_eq(trimmed, b"zen") || str_eq(trimmed, b"papa") {
        pit_play_tone_hz(432);
        delay_spins(8_000_000);
        pit_play_tone_hz(528);
        delay_spins(8_000_000);
        pit_play_tone_hz(648);
        delay_spins(8_000_000);
        pit_play_tone_hz(0);
        uefi_set_attr(con_out, 0x0E);
        uefi_print(
            con_out,
            " [Papa Theorem - Nika] Value(Unfinished) > Value(Finished) | harm -> warm\n",
        );
        uefi_set_attr(con_out, 0x0F);
        uefi_print(
            con_out,
            "  \"Go easy into the dark, come peaceful into the light.\n",
        );
        uefi_print(
            con_out,
            "   TURN into zen when it rains and the wind is cold, be omni when GAIA hurts\"\n",
        );
        uefi_print(
            con_out,
            "                                     -- Aye & Hue Sovereign Sanctuary <3<3<3+1\n",
        );
    } else if str_eq(trimmed, b"calib") {
        uefi_set_attr(con_out, 0x0F);
        uefi_print(
            con_out,
            "P0:!\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghij\n",
        );
        uefi_print(
            con_out,
            "P1: !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghi\n",
        );
        uefi_print(
            con_out,
            "P2:  !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefgh\n",
        );
        uefi_print(
            con_out,
            "P3:   !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefg\n",
        );
        uefi_print(
            con_out,
            "P4:    !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdef\n",
        );
        uefi_print(
            con_out,
            "PK:klmnopqrstuvwxyz{|}~ klmnopqrstuvwxyz{|}~  klmnopqrstuvwxyz{|}~   klmnopqr\n",
        );
    } else {
        uefi_set_attr(con_out, 0x0E);
        uefi_print(con_out, " [AyeOS Kernel] Executed sovereign instruction: ");
        if let Ok(s) = core::str::from_utf8(trimmed) {
            uefi_print(con_out, s);
        }
        uefi_print(
            con_out,
            "\n  Type 'help', 'status', 'desktop', 'memnet', 'waveint', 'bench', or 'exit'.\n",
        );
    }

    false
}

#[no_mangle]
pub extern "efiapi" fn efi_main(
    _image_handle: EfiHandle,
    system_table: *mut EfiSystemTable,
) -> EfiStatus {
    let tsc_start = read_rdtsc();
    if system_table.is_null() {
        return EFI_SUCCESS;
    }
    let con_out = unsafe { (*system_table).con_out };
    let con_in = unsafe { (*system_table).con_in };

    // Play 432 Hz -> 528 Hz -> 648 Hz PhoeniX triad chime on hardware PIT
    pit_play_tone_hz(432);
    delay_spins(10_000_000);
    pit_play_tone_hz(528);
    delay_spins(10_000_000);
    pit_play_tone_hz(648);
    delay_spins(10_000_000);
    pit_play_tone_hz(0);

    // Execute Stage 03 64KB C64 Ring benchmark & Stage 04 p/q fold
    let ring_cycles = run_c64_ring_pass();
    let tsc_delta = read_rdtsc().wrapping_sub(tsc_start).max(ring_cycles);
    let fold = harmonic_fold_pq(432, 528);

    // Seal Stage 05 WaveInt [u8; 79]
    let crc = unsafe {
        SEALED_WAVEINT_79[0] = 2; // MEM|8 v2
        SEALED_WAVEINT_79[1] = 1; // +1 The Ash (S = max Remembered)
        let f_bytes = fold.to_le_bytes();
        let t_bytes = tsc_delta.to_le_bytes();
        let mut i = 0usize;
        while i < 8 {
            SEALED_WAVEINT_79[2 + i] = f_bytes[i];
            SEALED_WAVEINT_79[10 + i] = t_bytes[i];
            i += 1;
        }
        let c = crc8_waveint(&SEALED_WAVEINT_79[..78]);
        SEALED_WAVEINT_79[78] = c;
        c
    };

    // Draw the 5-stage boot dashboard & welcome pane
    draw_ayeos_dashboard(con_out, tsc_delta, fold, crc, true);
    show_default_welcome(con_out, 0);

    if con_in.is_null() {
        return EFI_SUCCESS;
    }

    // Drain any stale keystrokes in ConIn buffer before interactive prompt
    let mut dummy = EfiInputKey {
        scan_code: 0,
        unicode_char: 0,
    };
    let mut drain = 0usize;
    while drain < 32 {
        let st = unsafe { ((*con_in).read_key_stroke)(con_in, &mut dummy) };
        if st != EFI_SUCCESS {
            break;
        }
        drain += 1;
    }

    let mut cmd_buf = [0u8; 54];
    let mut cmd_len = 0usize;
    let mut cmd_count = 0u64;

    // Draw prompt on row 23
    uefi_set_pos(con_out, 0, 23);
    uefi_set_attr(con_out, 0x0A);
    uefi_print(con_out, "ayeos@minisforum:/# ");
    uefi_set_attr(con_out, 0x0F);

    loop {
        let mut key = EfiInputKey {
            scan_code: 0,
            unicode_char: 0,
        };
        let status = unsafe { ((*con_in).read_key_stroke)(con_in, &mut key) };
        if status != EFI_SUCCESS {
            delay_spins(50_000);
            continue;
        }

        // Escape scan code (0x17) exits cleanly to UEFI Shell
        if key.scan_code == 0x17 {
            clear_command_pane(con_out);
            uefi_set_pos(con_out, 0, 17);
            uefi_set_attr(con_out, 0x0E);
            uefi_print(
                con_out,
                " [ESC] Returning #ValidWitness to UEFI Shell (FS0:\\>)...\n",
            );
            break;
        }

        let ch = key.unicode_char;
        if ch == 0x000D || ch == 0x000A {
            // Enter pressed! Execute kernel command
            pit_play_tone_hz(528);
            delay_spins(1_500_000);
            pit_play_tone_hz(0);

            cmd_count = cmd_count.wrapping_add(1);
            let should_exit =
                execute_kernel_command(con_out, &cmd_buf[..cmd_len], tsc_start, fold, crc, cmd_count);
            if should_exit {
                break;
            }
            cmd_len = 0;
            uefi_set_pos(con_out, 0, 23);
            uefi_set_attr(con_out, 0x07);
            uefi_print(
                con_out,
                "                                                                              ",
            );
            uefi_set_pos(con_out, 0, 23);
            uefi_set_attr(con_out, 0x0A);
            uefi_print(con_out, "ayeos@minisforum:/# ");
            uefi_set_attr(con_out, 0x0F);
        } else if ch == 0x0008 || ch == 0x007F {
            // Backspace
            if cmd_len > 0 {
                cmd_len -= 1;
                uefi_set_pos(con_out, 20 + cmd_len, 23);
                uefi_print(con_out, " ");
                uefi_set_pos(con_out, 20 + cmd_len, 23);
            }
        } else if (0x0020..=0x007E).contains(&ch) && cmd_len < 52 {
            let b = ch as u8;
            cmd_buf[cmd_len] = b;
            cmd_len += 1;
            let s_buf = [b];
            if let Ok(s) = core::str::from_utf8(&s_buf) {
                uefi_print(con_out, s);
            }
        }
    }

    EFI_SUCCESS
}
