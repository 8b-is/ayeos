//! Bounded, single-boot diagnostic data. Integer fields; no allocator or I/O.
//! A RAM record is not persistent storage. Only the owner-requested frozen
//! snapshot is transferred; no USB command triggers a probe or exception.

pub const BYTES: usize = 1024;
pub const EVENTS: usize = 16;
pub const TEST_CRC: u32 = 1;
pub const TEST_MARINE: u32 = 2;
pub const TEST_EXCEPTION: u32 = 4;
pub const EXPECTED_TESTS: u32 = TEST_CRC | TEST_MARINE | TEST_EXCEPTION;

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct Fault {
    pub vector: u64,
    pub level: u64,
    pub esr: u64,
    pub pc: u64,
    pub far: u64,
    pub spsr: u64,
    pub sp: u64,
    pub action: u64, // 0 absent, 1 self-test, 2 guarded data abort, 3 fatal
}
impl Fault {
    pub const ZERO: Self = Self {
        vector: 0,
        level: 0,
        esr: 0,
        pc: 0,
        far: 0,
        spsr: 0,
        sp: 0,
        action: 0,
    };
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Fixup {
    pub pc: u64,
    pub resume: u64,
}

/// Only same-EL synchronous AArch64 exceptions at exact registered sites may
/// resume. IRQ/FIQ/SError, unknown instructions and ordinary Rust loads halt.
pub fn recovery(f: &Fault, sites: &[Fixup], test: Fixup, test_armed: bool) -> Option<(u64, u64)> {
    if !matches!(f.vector, 0 | 4) || !matches!(f.level, 1 | 2) || f.esr & (1 << 25) == 0 {
        return None;
    }
    match f.esr >> 26 {
        0x3c if test_armed && f.pc == test.pc && f.esr & 0xffff == 0xa8 => Some((test.resume, 1)),
        0x25 => sites.iter().find(|s| s.pc == f.pc).map(|s| (s.resume, 2)),
        _ => None,
    }
}

#[derive(Clone, Copy, Default)]
pub struct Event {
    pub sequence: u64,
    pub stage: u64,
    pub result: u64,
    pub detail: u64,
}

pub struct Journal {
    pub events: [Event; EVENTS],
    pub total: u64,
}
impl Journal {
    pub const fn new() -> Self {
        Self {
            events: [Event {
                sequence: 0,
                stage: 0,
                result: 0,
                detail: 0,
            }; EVENTS],
            total: 0,
        }
    }
    pub fn record(&mut self, stage: u64, result: u64, detail: u64) {
        // Saturate at u64::MAX rather than falsely reusing old sequence numbers.
        if self.total == u64::MAX {
            return;
        }
        let slot = (self.total % EVENTS as u64) as usize;
        self.total += 1;
        self.events[slot] = Event {
            sequence: self.total,
            stage,
            result,
            detail,
        };
    }
}

pub struct Snapshot<'a> {
    pub revision: &'a str,
    pub dirty: bool,
    pub passed: u32,
    pub failed: u32,
    pub board_crc: u32,
    pub fault_count: u64,
    pub fault: Fault,
    pub marine_hash: u64,
    pub marine_samples: u64,
    pub journal: &'a Journal,
}

pub fn encode(s: Snapshot<'_>) -> [u8; BYTES] {
    let mut out = [0; BYTES];
    fn word(out: &mut [u8], at: usize, value: u32) {
        out[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    fn wide(out: &mut [u8], at: usize, value: u64) {
        out[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    out[..4].copy_from_slice(b"ADRP");
    word(&mut out, 4, 1);
    word(&mut out, 8, BYTES as u32);
    word(&mut out, 12, 1 | (u32::from(s.dirty) << 1)); // frozen volatile snapshot, dirty build
    word(&mut out, 16, 3); // bit0 diagnostics, bit1 Marine fixture; no live PCM/Phoenix bit
    word(&mut out, 20, s.passed);
    word(&mut out, 24, s.failed);
    word(&mut out, 28, s.journal.total.min(EVENTS as u64) as u32);
    wide(&mut out, 32, s.journal.total.saturating_sub(EVENTS as u64));
    word(&mut out, 40, s.board_crc);
    // Revision is validated by build.rs, not inferred from a UI version string.
    if s.revision.len() == 40 {
        out[64..104].copy_from_slice(s.revision.as_bytes());
    }
    wide(&mut out, 112, s.fault_count);
    let f = s.fault;
    for (i, value) in [
        f.vector, f.level, f.esr, f.pc, f.far, f.spsr, f.sp, f.action,
    ]
    .into_iter()
    .enumerate()
    {
        wide(&mut out, 128 + i * 8, value);
    }
    wide(&mut out, 192, s.marine_hash);
    wide(&mut out, 200, s.marine_samples);
    let first = s.journal.total.saturating_sub(EVENTS as u64);
    for (i, seq) in (first..s.journal.total).enumerate() {
        let e = s.journal.events[(seq % EVENTS as u64) as usize];
        for (j, value) in [e.sequence, e.stage, e.result, e.detail]
            .into_iter()
            .enumerate()
        {
            wide(&mut out, 256 + i * 32 + j * 8, value);
        }
    }
    out
}
