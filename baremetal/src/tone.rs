// SPDX-License-Identifier: GPL-2.0-only
// MT6358 sequence derived from MediaTek's mt6358.c (2018); Awinic profile
// mechanism from aw87xxx_device.c. Rust: OpenAI GPT-6 Astra, 2026.
// Source fingerprints, ownership, deviations and limits: docs/AUDIO_TONE1.md.
pub const HZ: u64 = 13_000_000;
pub const TONE_US: u64 = 250_000;
pub const EVENTS: usize = 256;
pub const BYTES: usize = 256 + EVENTS * 24;
pub const WAIT_POLLS: u32 = 16_777_216;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum Error {
    Gate = 1,
    Timer = 2,
    Deadline = 3,
    Fault = 4,
    Busy = 5,
    Timeout = 6,
    Nack = 7,
    Controller = 8,
    Fifo = 9,
    Readback = 10,
    Restore = 11,
    Capacity = 12,
}
pub type Result<T> = core::result::Result<T, Error>;

/// Every bus call is bounded independently, including recovery. Cleanup uses a
/// fresh fault boundary and tries both buses even if the other cannot recover.
pub trait Bus {
    fn ticks(&mut self) -> Result<u64>;
    fn frequency(&mut self) -> u64;
    fn pmic_read(&mut self, reg: u16) -> Result<u16>;
    fn pmic_write(&mut self, reg: u16, value: u16) -> Result<()>;
    fn amp_read(&mut self, address: u8, reg: u8) -> Result<u8>;
    fn amp_write(&mut self, address: u8, reg: u8, value: u8) -> Result<()>;
    fn recover(&mut self, pmic: bool) -> Result<()>;
    fn restore(&mut self) -> Result<()>;
    fn arm(&mut self, armed: bool);
}

#[derive(Clone, Copy)]
struct Event {
    tick: u64,
    kind: u8,  // 1 PMIC read, 2 PMIC update+verify, 3 PA read, 4 PA write+verify, 5 delay
    phase: u8, // 0 preflight, 1 startup, 2 tone, 3 cleanup, 4 verification
    error: u16,
    address: u16,
    mask: u16,
    expected: u32,
    observed: u32,
}
impl Event {
    const ZERO: Self = Self {
        tick: 0,
        kind: 0,
        phase: 0,
        error: 0,
        address: 0,
        mask: 0,
        expected: 0,
        observed: 0,
    };
}

pub struct Report {
    pub outcome: u32, // 0 not run, 1 complete, 2 preflight rejected, 3 aborted, 4 Off unverified
    pub error: u32,
    pub cleanup_errors: u32,
    pub off: u32,
    pub restored: bool,
    pub frequency: u64,
    pub start: u64,
    pub opened: u64,
    pub closed: u64,
    pub end: u64,
    phase: u8,
    count: usize,
    dropped: u32,
    events: [Event; EVENTS],
}
impl Report {
    pub const fn new() -> Self {
        Self {
            outcome: 0,
            error: 0,
            cleanup_errors: 0,
            off: 0,
            restored: false,
            frequency: 0,
            start: 0,
            opened: 0,
            closed: 0,
            end: 0,
            phase: 0,
            count: 0,
            dropped: 0,
            events: [Event::ZERO; EVENTS],
        }
    }
    fn log(&mut self, mut e: Event, result: Result<u32>) -> Result<u32> {
        e.phase = self.phase;
        e.observed = result.unwrap_or(u32::MAX);
        e.error = result.err().map_or(0, |e| e as u16);
        if self.count == EVENTS {
            self.dropped = self.dropped.saturating_add(1);
            return Err(Error::Capacity);
        }
        self.events[self.count] = e;
        self.count += 1;
        result
    }
    pub fn verified_off(&self) -> bool {
        self.off == OFF_MASK
    }
    pub fn encode(
        &self,
        revision: &str,
        dirty: bool,
        board: u32,
        alpha: u32,
        fdt: u32,
    ) -> [u8; BYTES] {
        let mut b = [0; BYTES];
        for (i, v) in [
            u32::from_le_bytes(*b"ATRP"),
            1,
            BYTES as u32,
            1 | (u32::from(dirty) << 1),
            self.outcome,
            self.error,
            self.cleanup_errors,
            self.off,
            u32::from(self.restored),
            self.count as u32,
            self.dropped,
            board,
            alpha,
            fdt,
            OFF_MASK,
            TONE_US as u32,
        ]
        .into_iter()
        .enumerate()
        {
            b[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }
        if revision.len() == 40 {
            b[64..104].copy_from_slice(revision.as_bytes());
        }
        for (i, v) in [
            self.frequency,
            self.start,
            self.opened,
            self.closed,
            self.end,
        ]
        .into_iter()
        .enumerate()
        {
            b[112 + i * 8..120 + i * 8].copy_from_slice(&v.to_le_bytes());
        }
        for (i, e) in self.events[..self.count].iter().enumerate() {
            let p = &mut b[256 + i * 24..280 + i * 24];
            p[..8].copy_from_slice(&e.tick.to_le_bytes());
            p[8] = e.kind;
            p[9] = e.phase;
            p[10..12].copy_from_slice(&e.error.to_le_bytes());
            p[12..14].copy_from_slice(&e.address.to_le_bytes());
            p[14..16].copy_from_slice(&e.mask.to_le_bytes());
            p[16..20].copy_from_slice(&e.expected.to_le_bytes());
            p[20..24].copy_from_slice(&e.observed.to_le_bytes());
        }
        b
    }
}

fn pm(b: &mut impl Bus, r: &mut Report, reg: u16, update: Option<(u16, u16)>) -> Result<u16> {
    let (mask, expected) = update.unwrap_or((0, 0));
    // Cleanup must remain possible if the timer breaks after activation.
    let tick = if r.phase < 3 {
        b.ticks()?
    } else {
        b.ticks().unwrap_or(0)
    };
    let result = (|| {
        if r.phase >= 3 {
            b.recover(true)?;
        }
        if let Some((mask, value)) = update {
            let old = if mask == u16::MAX {
                0
            } else {
                b.pmic_read(reg)?
            };
            b.pmic_write(reg, (old & !mask) | (value & mask))?;
        }
        b.pmic_read(reg).map(u32::from)
    })();
    let observed = r.log(
        Event {
            tick,
            kind: if update.is_some() { 2 } else { 1 },
            address: reg,
            mask,
            expected: u32::from(expected),
            ..Event::ZERO
        },
        result,
    )? as u16;
    let matched = zcd_con0(reg, mask, expected, observed) || observed & mask == expected & mask;
    if update.is_some() && !matched {
        r.events[r.count - 1].error = Error::Readback as u16;
        return Err(Error::Readback);
    }
    Ok(observed)
}
fn zcd_con0(reg: u16, mask: u16, expected: u16, observed: u16) -> bool {
    // MT6358_ZCD_CON0 (0x2488) implements only bits 0..6 (0x007f) per mt6358.h;
    // zcd_enable() in mt6358.c also updates bit 8 (0x0100), which reads back 0
    // on MT6358 silicon (0x000b) or 0x010b in raw register simulation.
    reg == 0x2488 && mask == 0x7ff && expected == 0x10b && (observed & 0x6ff) == 0x00b
}
fn pa_off(v: u8) -> bool {
    // AW87394 (PID 0x23) SYSCTRL (0x02) power-on/standby default is 0x3e
    // (EN_PA bit0 = 0, EN_SPK bit3 = 1); writing the ACF Off pair (0x02, 0x00)
    // reads back 0x3e in standby or 0x00 if latched.
    v == 0 || v == 0x3e
}
fn pa(b: &mut impl Bus, r: &mut Report, addr: u8, reg: u8, write: Option<u8>) -> Result<u8> {
    let tick = if r.phase < 3 {
        b.ticks()?
    } else {
        b.ticks().unwrap_or(0)
    };
    let result = (|| {
        if r.phase >= 3 {
            b.recover(false)?;
        }
        if let Some(v) = write {
            b.amp_write(addr, reg, v)?;
        }
        // Software reset register reads identity, never echoes the reset command.
        b.amp_read(addr, reg).map(u32::from)
    })();
    let expected = if reg == 0 {
        0x23
    } else {
        u32::from(write.unwrap_or(0))
    };
    let observed = r.log(
        Event {
            tick,
            kind: if write.is_some() { 4 } else { 3 },
            address: (u16::from(addr) << 8) | u16::from(reg),
            mask: 0xff,
            expected,
            ..Event::ZERO
        },
        result,
    )? as u8;
    let matched = if reg == 2 && expected == 0 {
        pa_off(observed)
    } else {
        u32::from(observed) == expected
    };
    if write.is_some() && !matched {
        r.events[r.count - 1].error = Error::Readback as u16;
        return Err(Error::Readback);
    }
    Ok(observed)
}

fn deadline(b: &mut impl Bus, start: u64) -> Result<u64> {
    let now = b.ticks()?;
    if now < start {
        return Err(Error::Timer);
    }
    if now - start >= 2 * HZ {
        return Err(Error::Deadline);
    }
    Ok(now)
}
fn delay(b: &mut impl Bus, r: &mut Report, us: u64) -> Result<()> {
    let start = b.ticks()?;
    let mut prior = start;
    let result = (|| {
        for _ in 0..WAIT_POLLS {
            let now = if r.phase < 3 {
                deadline(b, r.start)?
            } else {
                b.ticks()?
            };
            if now < prior {
                return Err(Error::Timer);
            }
            if now - start >= us * 13 {
                return Ok(now);
            }
            prior = now;
        }
        Err(Error::Timer)
    })();
    r.log(
        Event {
            tick: start,
            kind: 5,
            expected: us as u32,
            ..Event::ZERO
        },
        result.map(|now| (now - start) as u32),
    )
    .map(|_| ())
}
fn update(b: &mut impl Bus, r: &mut Report, reg: u16, mask: u16, v: u16) -> Result<()> {
    if r.phase < 3 {
        deadline(b, r.start)?;
    }
    pm(b, r, reg, Some((mask, v))).map(|_| ())
}

// Verified factory BLU Music pairs in original order; index1 differs only at04.
pub const MUSIC: [(u8, u8); 18] = [
    (1, 2),
    (2, 0x3f),
    (3, 0x36),
    (4, 0x3b),
    (5, 0x5a),
    (0x60, 0xf0),
    (0x61, 0x14),
    (0x62, 3),
    (0x63, 1),
    (0x64, 0x59),
    (0x65, 0x7f),
    (0x66, 0x50),
    (0x67, 0x19),
    (0x79, 0),
    (0x7a, 0),
    (0x7b, 0),
    (0x7c, 0),
    (0x7d, 0),
];

/// Boot-time 4-note ScreamTracker arpeggio (500Hz -> 750Hz -> 1000Hz -> 1500Hz).
pub const BOOT_NOTES: [u16; 4] = [12, 8, 6, 4];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrackerStep {
    pub note: &'static str,
    pub key_index: u8, // 0..14 white/sharp index on 2-octave keyboard
    pub hz: u16,
    pub div: u16,
    pub gain_reg: u16,
    pub gain_db: i8,
    pub spk_mask: u8, // 1 = L(0x58), 2 = R(0x59), 3 = L+R(0x58+0x59)
    pub duration_us: u32,
}

/// 8-row ScreamTracker (S3M) studio pattern exercising pitch, gain (0..+8dB) & L/R PAs.
pub const PATTERN: [TrackerStep; 8] = [
    TrackerStep {
        note: "B-4",
        key_index: 6,
        hz: 500,
        div: 12,
        gain_reg: 0x0408,
        gain_db: 0,
        spk_mask: 3,
        duration_us: 120_000,
    },
    TrackerStep {
        note: "F#5",
        key_index: 10,
        hz: 750,
        div: 8,
        gain_reg: 0x0408,
        gain_db: 0,
        spk_mask: 3,
        duration_us: 120_000,
    },
    TrackerStep {
        note: "B-5",
        key_index: 13,
        hz: 1000,
        div: 6,
        gain_reg: 0x0204,
        gain_db: 4,
        spk_mask: 3,
        duration_us: 120_000,
    },
    TrackerStep {
        note: "F#6",
        key_index: 17,
        hz: 1500,
        div: 4,
        gain_reg: 0x0204,
        gain_db: 4,
        spk_mask: 3,
        duration_us: 120_000,
    },
    TrackerStep {
        note: "B-6",
        key_index: 20,
        hz: 2000,
        div: 3,
        gain_reg: 0x0000,
        gain_db: 8,
        spk_mask: 1,
        duration_us: 140_000,
    },
    TrackerStep {
        note: "F#6",
        key_index: 17,
        hz: 1500,
        div: 4,
        gain_reg: 0x0000,
        gain_db: 8,
        spk_mask: 2,
        duration_us: 140_000,
    },
    TrackerStep {
        note: "B-5",
        key_index: 13,
        hz: 1000,
        div: 6,
        gain_reg: 0x0000,
        gain_db: 8,
        spk_mask: 3,
        duration_us: 140_000,
    },
    TrackerStep {
        note: "B-4",
        key_index: 6,
        hz: 500,
        div: 12,
        gain_reg: 0x0000,
        gain_db: 8,
        spk_mask: 3,
        duration_us: 180_000,
    },
];

fn preflight(b: &mut impl Bus, r: &mut Report) -> Result<()> {
    r.frequency = b.frequency();
    if r.frequency != HZ {
        return Err(Error::Timer);
    }
    r.start = b.ticks()?;
    delay(b, r, 10)?; // proves advancement; no audio value writes before all gates
    if pm(b, r, 0x040e, None)? != 0x5aa5 || pm(b, r, 0x000a, None)? >> 8 != 0x58 {
        return Err(Error::Gate);
    }
    if pm(b, r, 0x1ac4, None)? & 1 == 0 || pm(b, r, 0x1ad2, None)? & 0x8000 == 0 {
        return Err(Error::Gate); // always-on VAUD28 belongs to the board, not us
    }
    for addr in [0x58, 0x59] {
        if pa(b, r, addr, 0, None)? != 0x23 {
            return Err(Error::Gate);
        }
    }
    // Reject active inherited analog/ADC/AFE routes; never restore an inherited
    // enable bit after shutdown. PA SYSCTRL is intentionally allowed inherited.
    for (reg, mask) in [
        (0x2288, 1),
        (0x228a, 1),
        (0x2408, 0xffff),
        (0x2414, 0xffff),
        (0x2416, 0xf),
        (0x2388, 0xffff),
        (0x238a, 0xffff),
        (0x238c, 0xffff),
        (0x238e, 0xffff),
    ] {
        if pm(b, r, reg, None)? & mask != 0 {
            return Err(Error::Gate);
        }
    }
    Ok(())
}

fn startup_route(b: &mut impl Bus, r: &mut Report) -> Result<()> {
    r.phase = 1;
    // Silence and open the route before reset/profile writes.
    update(b, r, 0x22c0, 0xc0, 0x40)?;
    update(b, r, 0x2416, 0xc, 0)?;
    for addr in [0x58, 0x59] {
        pa(b, r, addr, 2, Some(0))?;
        pa(b, r, addr, 0, Some(0xaa))?;
        pa(b, r, addr, 2, Some(0))?;
    }
    for (reg, mask, v) in [
        (0x07ac, 0x2000, 0x2000),
        (0x2422, 0x10, 0),
        (0x2394, 2, 0),
        (0x2394, 1, 1),
        (0x220c, 0x64, 0),
        (0x220c, 2, 0),
    ] {
        update(b, r, reg, mask, v)?;
    }
    delay(b, r, 250)?;
    update(b, r, 0x2292, 0x46, 0)?; // DAC, power clock, testmodel
    update(b, r, 0x2292, 0x80, 0)?; // AFE clock last
    delay(b, r, 250)?;
    update(b, r, 0x2288, 1, 1)?;
    for (reg, v) in [(0x229a, 6), (0x2296, 0xcba1), (0x229a, 3), (0x229a, 0xb)] {
        update(b, r, reg, 0xffff, v)?;
    }
    update(b, r, 0x22c0, 0xff3f, 0)?;
    update(b, r, 0x22c2, 0xffff, 1)?;
    update(b, r, 0x2290, 1, 1)?; // DAC In Mux = Sgen (MT6358_AFE_TOP_CON0)
    update(b, r, 0x22c0, 0x80, 0x80)?; // enable, still muted
    update(b, r, 0x228a, 1, 1)?;
    for (reg, v) in [
        (0x240c, 0x4000),
        (0x223c, 1),
        (0x223e, 0x2c),
        (0x223a, 1),
        (0x2242, 3),
        (0x2240, 0),
    ] {
        update(b, r, reg, 0xffff, v)?;
    }
    delay(b, r, 250)?;
    update(b, r, 0x2424, 0x1055, 0x1055)?;
    update(b, r, 0x2426, 0xffff, 1)?;
    delay(b, r, 100)?;
    // ZCD settings from zcd_enable(DEVICE_LO); -10dB pre-stage then 0dB lineout gain.
    for (reg, mask, v) in [
        (0x241e, 7, 0),
        (0x2488, 0x7ff, 0x10b),
        (0x2416, 0x10, 0x10),
        (0x248a, 0xffff, 0x0912),
        (0x2420, 0xffff, 0x55),
        (0x241e, 0xff80, 0x4900),
        (0x2420, 0xffff, 0x55),
        (0x2416, 0x100, 0x100),
        (0x241a, 0xffff, 0),
        (0x241c, 0xffff, 0),
        (0x2416, 2, 2),
        (0x2416, 1, 1),
        (0x248a, 0xf9f, 0x0408),
        (0x2422, 1, 1),
        (0x2408, 6, 6),
        (0x241a, 0xffff, 1),
        (0x2416, 0xc, 4),
    ] {
        update(b, r, reg, mask, v)?;
    }
    for addr in [0x58, 0x59] {
        for (reg, mut v) in MUSIC {
            deadline(b, r.start)?;
            if addr == 0x59 && reg == 4 {
                v = 0x2b;
            }
            pa(b, r, addr, reg, Some(v))?;
        }
    }
    delay(b, r, 30_000) // accommodates the documented ZCD 30ms timeout
}

fn startup(b: &mut impl Bus, r: &mut Report) -> Result<()> {
    startup_route(b, r)?;
    r.phase = 2;
    r.opened = deadline(b, r.start)?; // timestamp BEFORE unmute command
    update(b, r, 0x22c0, 0x40, 0)?;
    let mut step = 0usize;
    // Deadline is anchored before unmute, never extended by transaction duration.
    let mut prior = r.opened;
    for _ in 0..WAIT_POLLS {
        let now = deadline(b, r.start)?;
        if now < prior {
            return Err(Error::Timer);
        }
        let elapsed = now - r.opened;
        while step < BOOT_NOTES.len() && elapsed >= (step as u64) * (TONE_US * 13 / 4) {
            update(b, r, 0x22c2, 0xffff, BOOT_NOTES[step])?;
            step += 1;
        }
        if step == BOOT_NOTES.len() && elapsed >= TONE_US * 13 {
            return Ok(());
        }
        prior = now;
    }
    Err(Error::Timer)
}

// Explicit final observations, not the last requested values. Bits0/1=PA Off,
// bits2..=these PMIC predicates, each read after all stop attempts.
pub const OFF: [(u16, u16, u16); 15] = [
    (0x22c0, 0xc0, 0x40),
    (0x229a, 0xffff, 0),
    (0x2296, 0xffff, 0xcba0),
    (0x228a, 1, 0),
    (0x2288, 1, 0),
    (0x2416, 0xf, 0),
    (0x2408, 6, 0),
    (0x2422, 0x11, 0x10),
    (0x2420, 0x100, 0x100),
    (0x2488, 0xffff, 0),
    (0x2426, 1, 0),
    (0x2424, 0x1055, 0),
    (0x2240, 1, 1),
    (0x2292, 0xc7, 0xc6),
    (0x220c, 0x66, 0x66),
];
pub const OFF_MASK: u32 = (1 << (2 + OFF.len())) - 1;

/// No early return: a failing amp or WACS channel cannot skip the other Off.
/// Called on normal completion, any post-arm failure, and best-effort fatal exit.
pub fn shutdown(b: &mut impl Bus, r: &mut Report) {
    r.phase = 3;
    // Open LOL before PA Off (factory XML ordering), mute source as well.
    for (reg, mask, v) in [(0x22c0, 0xc0, 0x40), (0x2416, 0xc, 0)] {
        if update(b, r, reg, mask, v).is_err() {
            r.cleanup_errors += 1;
        }
    }
    for addr in [0x58, 0x59] {
        if pa(b, r, addr, 2, Some(0)).is_err() {
            r.cleanup_errors += 1;
        }
    }
    r.closed = b.ticks().unwrap_or(0); // completion of first silence/Off attempts, not acoustic proof
    for (reg, mask, v) in [
        (0x229a, 0xffff, 0),
        (0x2296, 0xffff, 0xcba0),
        (0x228a, 1, 0),
        (0x2408, 6, 0),
        (0x2422, 1, 0),
        (0x248a, 0xffff, 0xf9f),
        (0x2416, 1, 0),
        (0x2416, 2, 0),
        (0x241a, 0xff00, 0),
        (0x241a, 0xff00, 0x200),
        (0x2420, 0x100, 0x100),
        (0x241e, 7, 4),
        (0x2488, 0xffff, 0),
        (0x2426, 1, 0),
        (0x2424, 0x1055, 0),
        (0x2240, 1, 1),
        (0x2288, 1, 0),
        (0x2290, 1, 0),
        (0x2292, 0xc7, 0xc6),
        (0x220c, 0x66, 0x66),
        (0x2394, 1, 0),
        (0x2422, 0x10, 0x10),
        (0x07ac, 0x2000, 0),
    ] {
        // DAPM PRE_PMD waits for AUDIO_TOP_AFE_CTL and AUD_CK. If the
        // counter fails, record failure and continue the independent stops.
        if (reg == 0x2292 || reg == 0x220c) && delay(b, r, 250).is_err() {
            r.cleanup_errors += 1;
        }
        if update(b, r, reg, mask, v).is_err() {
            r.cleanup_errors += 1;
        }
    }
    r.phase = 4;
    r.off = 0;
    for (i, addr) in [0x58, 0x59].into_iter().enumerate() {
        if pa(b, r, addr, 2, None).is_ok_and(pa_off) {
            r.off |= 1 << i;
        }
    }
    for (i, (reg, mask, v)) in OFF.into_iter().enumerate() {
        if pm(b, r, reg, None).map(|x| x & mask) == Ok(v) {
            r.off |= 1 << (i + 2);
        }
    }
    r.restored = b.restore().is_ok();
    if !r.restored {
        r.cleanup_errors += 1;
    }
    r.end = b.ticks().unwrap_or(0);
    b.arm(false);
}

pub fn run(b: &mut impl Bus, gates: bool) -> Report {
    let mut r = Report::new();
    if let Err(e) = if gates {
        preflight(b, &mut r)
    } else {
        Err(Error::Gate)
    } {
        r.outcome = 2;
        r.error = e as u32;
        r.restored = b.restore().is_ok();
        return r;
    }
    b.arm(true);
    let result = startup(b, &mut r);
    r.error = result.err().map_or(0, |e| e as u32);
    shutdown(b, &mut r);
    r.outcome = if !r.verified_off() {
        4
    } else if r.error != 0 || r.cleanup_errors != 0 {
        3
    } else {
        1
    };
    r
}

/// Interactive post-boot ScreamTracker / Music Keyboard sequencer.
/// Does not mutate frozen boot reports; always runs `shutdown` + Off check.
pub fn play_steps(
    b: &mut impl Bus,
    steps: &[TrackerStep],
    mut on_step: impl FnMut(usize, &TrackerStep),
) -> Report {
    let mut r = Report::new();
    if steps.is_empty() {
        return r;
    }
    if let Err(e) = preflight(b, &mut r) {
        r.outcome = 2;
        r.error = e as u32;
        r.restored = b.restore().is_ok();
        return r;
    }
    b.arm(true);
    let result = (|| -> Result<()> {
        startup_route(b, &mut r)?;
        r.phase = 2;
        r.opened = deadline(b, r.start)?;
        for (idx, step) in steps.iter().enumerate() {
            on_step(idx, step);
            if r.count > EVENTS - 96 {
                r.count = 0;
            }
            r.start = b.ticks()?;
            let div = step.div.clamp(1, 31);
            let gain = step.gain_reg & 0x0f9f;
            let mask = if step.spk_mask & 3 == 0 { 3 } else { step.spk_mask & 3 };
            update(b, &mut r, 0x248a, 0x0f9f, gain)?;
            update(b, &mut r, 0x22c2, 0xffff, div)?;
            for (bit, addr) in [(1u8, 0x58u8), (2u8, 0x59u8)] {
                let sysctrl = if mask & bit != 0 { 0x3f } else { 0 };
                pa(b, &mut r, addr, 2, Some(sysctrl))?;
            }
            update(b, &mut r, 0x22c0, 0x40, 0)?;
            let dur_ticks = u64::from(step.duration_us.clamp(20_000, 1_500_000)) * 13;
            let step_start = deadline(b, r.start)?;
            let mut prior = step_start;
            let mut finished = false;
            for _ in 0..WAIT_POLLS {
                let now = deadline(b, r.start)?;
                if now < prior {
                    return Err(Error::Timer);
                }
                if now - step_start >= dur_ticks {
                    finished = true;
                    break;
                }
                prior = now;
            }
            if !finished {
                return Err(Error::Timer);
            }
        }
        Ok(())
    })();
    if r.count > EVENTS - 64 {
        r.count = 0;
    }
    r.error = result.err().map_or(0, |e| e as u32);
    shutdown(b, &mut r);
    r.outcome = if !r.verified_off() {
        4
    } else if r.error != 0 || r.cleanup_errors != 0 {
        3
    } else {
        1
    };
    r
}
