//! Read-only presentation of completed Tone1 evidence. No MMIO, allocation,
//! timers, or audio control. Paint once BEFORE USB attachment; the existing
//! one-cell-at-a-time idle updater owns the reserved live status strip.
use crate::{board_screen::Line, dashboard, tone};
use core::fmt::Write;
use dashboard::{panel, rect, text, Canvas, Glyph, GOLD, INK, LINE, MINT, MUTED, WHITE};

pub const USB_STATUS_Y: usize = 1408;
pub const FATAL_BANNER: &str = "AYEOS AUDIO-TONE1 / FATAL - photograph this screen";
const RED: u32 = 0xFFFF_9D9D;

/// Called only after emergency cleanup, when fatal USB service has stopped.
/// Framebuffer faults still depend on the existing nested-exception halt.
pub fn clear_for_fatal(c: &mut impl Canvas) {
    rect(c, 0, 0, dashboard::WIDTH, dashboard::HEIGHT, INK);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Checkpoint {
    RegistersPass,
    Review,
    OffUnverified,
}

/// Values already read by USB initialization, never re-probed by the UI.
#[derive(Default)]
pub struct UsbSnapshot {
    pub power: u8,
    pub devctl: u8,
    pub hwvers: u16,
    pub csr0: u16,
    pub acr6: u32,
    pub dtm0: u32,
    pub dtm1: u32,
    pub infra: u32,
    pub top: u32,
}
impl UsbSnapshot {
    pub fn controller_line(&self) -> Line {
        let mut line = Line::new();
        let _ = write!(
            line,
            "Pre-connect POWER={:02x} DEV={:02x} HW={:04x} CSR0={:04x} CG0={:08x} TOP={:08x}",
            self.power, self.devctl, self.hwvers, self.csr0, self.infra, self.top
        );
        line
    }
    pub fn phy_line(&self) -> Line {
        let mut line = Line::new();
        let _ = write!(
            line,
            "PHY ACR6={:08x} DTM0={:08x} DTM1={:08x}",
            self.acr6, self.dtm0, self.dtm1
        );
        line
    }
}

pub struct View<'a> {
    pub revision: &'a str,
    pub dirty: bool,
    pub passed: u32,
    pub failed: u32,
    pub faults: u64,
    pub i2c_status: u32,
    pub amp_reads: usize,
    pub i2c_restored: bool,
    pub pmic_status: u32,
    pub pmic_reads: usize,
    pub pmic_idle: bool,
    pub report_crc: u32,
    pub usb: &'a UsbSnapshot,
    pub tone: &'a tone::Report,
}

impl View<'_> {
    pub fn checkpoint(&self) -> Checkpoint {
        // Preflight rejection never activated audio and makes no Off claim.
        // Everything else without complete final Off evidence must be prominent.
        if self.tone.outcome != 2 && !self.tone.verified_off() {
            return Checkpoint::OffUnverified;
        }
        if self.passed == 7
            && self.failed == 0
            && self.faults == 0
            && !self.dirty
            && self.revision.len() == 40
            && self.revision.bytes().all(|b| b.is_ascii_hexdigit())
            && self.i2c_status == 1
            && self.amp_reads == 256
            && self.i2c_restored
            && self.pmic_status == 1
            && self.pmic_reads == 33
            && self.pmic_idle
            && self.tone.outcome == 1
            && self.tone.error == 0
            && self.tone.cleanup_errors == 0
            && self.tone.verified_off()
            && self.tone.restored
            && self.tone.frequency == tone::HZ
            && self.tone.start > 0
            && self.tone.opened >= self.tone.start
            && self.tone.opened - self.tone.start < 2 * tone::HZ
            && self.tone.closed > self.tone.opened
            && self.tone.closed - self.tone.opened >= 13 * tone::TONE_US
            && self.tone.end >= self.tone.closed
        {
            Checkpoint::RegistersPass
        } else {
            Checkpoint::Review
        }
    }
}

pub fn error_name(error: u32) -> &'static str {
    match error {
        0 => "none",
        1 => "preflight gate",
        2 => "timer",
        3 => "deadline",
        4 => "MMIO fault",
        5 => "bus busy",
        6 => "timeout",
        7 => "I2C NACK",
        8 => "controller",
        9 => "FIFO",
        10 => "readback",
        11 => "restore",
        12 => "journal capacity",
        _ => "unknown",
    }
}

pub fn outcome_name(outcome: u32) -> &'static str {
    match outcome {
        1 => "Attempt complete",
        2 => "Preflight rejected",
        3 => "Attempt aborted",
        4 => "Off not verified",
        _ => "Not established",
    }
}

fn yes(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "NO"
    }
}
fn test(passed: u32, failed: u32, bit: u32) -> &'static str {
    if passed & bit != 0 && failed & bit == 0 {
        "PASS"
    } else {
        "FAIL"
    }
}

pub fn paint(c: &mut impl Canvas, font: Glyph, v: &View<'_>) {
    rect(c, 0, 0, dashboard::WIDTH, dashboard::HEIGHT, INK);
    text(c, font, 40, 78, 2, "ayeOS", MINT);
    text(c, font, 454, 84, 1, "PHONE / STUDIO", MUTED);
    rect(c, 40, 134, 640, 1, LINE);
    text(c, font, 40, 164, 3, "Made to understand.", WHITE);
    text(
        c,
        font,
        42,
        234,
        1,
        "BLU G75 / BARE METAL / 5.6 / AUDIO-TONE1",
        MUTED,
    );

    let (headline, color, next) = match v.checkpoint() {
        Checkpoint::RegistersPass => (
            "REGISTER CHECKS PASS",
            MINT,
            "Report what you heard BEFORE this screen. Capture now.",
        ),
        Checkpoint::Review => (
            "REVIEW REQUIRED",
            GOLD,
            "Keep this evidence. Do not treat a failure as a pass.",
        ),
        Checkpoint::OffUnverified => (
            "OFF NOT VERIFIED",
            RED,
            "If sound persists, use physical power-off now.",
        ),
    };
    panel(c, 282, 266);
    text(c, font, 60, 306, 1, "01 / BOOT CHECKPOINT", color);
    text(c, font, 60, 346, 2, headline, color);
    text(
        c,
        font,
        60,
        394,
        1,
        "Not proof of audible sound or physical shutdown.",
        MUTED,
    );
    let mut line = Line::new();
    let _ = write!(
        line,
        "CRC {} / MARINE {} / EXCEPTION {}",
        test(v.passed, v.failed, 1),
        test(v.passed, v.failed, 2),
        test(v.passed, v.failed, 4)
    );
    text(c, font, 60, 430, 1, line.text(), WHITE);
    let mut line = Line::new();
    let _ = write!(
        line,
        "Pre-USB faults: {} / dirty build: {}",
        v.faults,
        yes(v.dirty)
    );
    text(c, font, 60, 466, 1, line.text(), MUTED);
    // The identity is always printed in full, never a success-shaped label.
    text(c, font, 60, 502, 1, v.revision, MUTED);

    panel(c, 568, 260);
    text(c, font, 60, 592, 1, "02 / AUDIO AND STOP", color);
    text(c, font, 60, 632, 2, outcome_name(v.tone.outcome), WHITE);
    let mut line = Line::new();
    if v.tone.outcome == 2 {
        let _ = write!(line, "Audio not attempted / no final Off claim");
    } else {
        let _ = write!(
            line,
            "Observed Off: {}/17 / controllers restored: {}",
            (v.tone.off & tone::OFF_MASK).count_ones(),
            yes(v.tone.restored)
        );
    }
    text(c, font, 60, 682, 1, line.text(), color);
    let mut line = Line::new();
    let _ = write!(
        line,
        "Attempt error: {} / cleanup errors: {}",
        error_name(v.tone.error),
        v.tone.cleanup_errors
    );
    text(c, font, 60, 718, 1, line.text(), WHITE);
    text(
        c,
        font,
        60,
        754,
        1,
        "250ms requested / SGEN bypasses live PCM",
        MUTED,
    );
    text(
        c,
        font,
        60,
        790,
        1,
        "Sound, stop, speakers and pops: ask the listener.",
        MUTED,
    );

    panel(c, 848, 222);
    text(c, font, 60, 872, 1, "03 / PRE-TONE HARDWARE EVIDENCE", MINT);
    let mut line = Line::new();
    let _ = write!(
        line,
        "PA reads: {}/256 / I2C restored: {}",
        v.amp_reads,
        yes(v.i2c_restored)
    );
    text(c, font, 60, 912, 1, line.text(), WHITE);
    let mut line = Line::new();
    let _ = write!(
        line,
        "PMIC reads: {}/33 / WACS2 idle: {}",
        v.pmic_reads,
        yes(v.pmic_idle)
    );
    text(c, font, 60, 948, 1, line.text(), WHITE);
    let mut line = Line::new();
    let _ = write!(
        line,
        "I2C status: {} / PMIC status: {} (1 = complete)",
        v.i2c_status, v.pmic_status
    );
    text(c, font, 60, 984, 1, line.text(), MUTED);
    let mut line = Line::new();
    let _ = write!(line, "Frozen tone CRC32: {:08x} / RAM only", v.report_crc);
    text(c, font, 60, 1020, 1, line.text(), MUTED);

    panel(c, 1090, 216);
    text(c, font, 60, 1114, 1, "04 / NEXT STEP", color);
    text(c, font, 60, 1154, 1, next, color);
    text(
        c,
        font,
        60,
        1190,
        1,
        "Capture: capture_tone.py / keep raw data private",
        WHITE,
    );
    text(
        c,
        font,
        60,
        1226,
        1,
        "Marine: synthetic fixture / PCM + Phoenix: not active",
        MUTED,
    );
    text(
        c,
        font,
        60,
        1262,
        1,
        "No autonomous repair, radio or network claim.",
        MUTED,
    );

    text(
        c,
        font,
        20,
        1342,
        1,
        "USB / 18d1:4ee0 / bcd0562 / READ ONLY",
        MINT,
    );
    text(
        c,
        font,
        20,
        1374,
        1,
        "Live transport below; not part of the frozen reports.",
        MUTED,
    );
    text(
        c,
        font,
        20,
        USB_STATUS_Y,
        1,
        "USB initialized; attachment pending",
        MUTED,
    );
    text(c, font, 20, 1440, 1, v.usb.controller_line().text(), MUTED);
    text(c, font, 20, 1460, 1, v.usb.phy_line().text(), MUTED);
    text(
        c,
        font,
        40,
        1480,
        1,
        "USB reset never replays a tone. No touch controls.",
        MUTED,
    );
    text(c, font, 40, 1534, 1, "Fine touch from within.", MINT);
    rect(c, 292, 1594, 136, 4, LINE);
}
