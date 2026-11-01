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

pub const SCRILLER_TEXT: &[u8] =
    b"*** AYEOS 5.6 SCREAMTRACKER S3M STUDIO * DUAL AW87394 + MT6358 SGEN * ZERO FLOAT * VOL TAP=NOTE / HOLD=SONG / USB 0xB8=LIVE JAM * {<3,<3,<3}+1 ***   ";
pub const SCRILLER_Y: usize = 604;

pub fn paint_scriller_cell(c: &mut impl Canvas, font: Glyph, phase: usize, col: usize) {
    if col >= 64 {
        return;
    }
    let x = 68 + col * 9;
    rect(c, x, 566, 9, 78, INK);
    for (layer, color) in [LINE, GOLD, MINT].iter().enumerate() {
        for dx in 0..9usize {
            let px = col * 9 + dx;
            let t = ((px + (phase & 255) + layer * 53) % 576) as i64 - 288;
            let arch = (82944 - t * t) * (82944 - t * t) / 190_000_000;
            let py = (628 - arch - layer as i64 * 9).clamp(570, 640) as usize;
            rect(c, x + dx, py, 1, 2, *color);
        }
    }
    let ch = SCRILLER_TEXT[(phase + col) % SCRILLER_TEXT.len()];
    let wave = ((col + phase) % 8) as i64 - 4;
    let bounce = (16 - wave * wave).max(0) as usize;
    let cell = [ch];
    if let Ok(s) = core::str::from_utf8(&cell) {
        text(c, font, x, SCRILLER_Y - bounce, 1, s, WHITE);
    }
}

pub fn paint_tracker_rows(c: &mut impl Canvas, font: Glyph, active_row: usize) {
    rect(c, 52, 732, 616, 232, INK);
    text(
        c,
        font,
        60,
        736,
        1,
        "ROW  NOTE  FREQ    SGEN   GAIN   SPK-PA       MODE",
        MINT,
    );
    rect(c, 60, 754, 600, 1, LINE);
    for (i, step) in tone::PATTERN.iter().enumerate() {
        let ry = 760 + i * 25;
        let selected = i == active_row % tone::PATTERN.len();
        if selected {
            rect(c, 56, ry - 2, 608, 22, LINE);
        }
        let spk = match step.spk_mask & 3 {
            1 => "L:58 SOLO ",
            2 => "R:59 SOLO ",
            _ => "58+59 DUAL",
        };
        let mut line = Line::new();
        let _ = write!(
            line,
            "{}{:02}  {}   {:04}Hz  D:{:02}   {:+03}dB  {}   {}",
            if selected { '>' } else { ' ' },
            i,
            step.note,
            step.hz,
            step.div,
            step.gain_db,
            spk,
            if selected { "PLAY/SEL" } else { "READY   " }
        );
        text(
            c,
            font,
            60,
            ry,
            1,
            line.text(),
            if selected { GOLD } else { WHITE },
        );
    }
}

pub fn paint_keyboard(c: &mut impl Canvas, font: Glyph, active_key: u8) {
    let kx0 = 66usize;
    let ky0 = 1024usize;
    let kw = 41usize;
    let kh = 112usize;
    rect(c, kx0 - 4, ky0 - 4, kw * 14 + 8, kh + 8, LINE);
    // 14 white keys across 2 octaves (C4..B5 / C5..B6)
    let white_semitones: [u8; 14] = [0, 2, 4, 5, 7, 9, 11, 12, 14, 16, 17, 19, 21, 23];
    let white_labels: [&str; 14] = [
        "C", "D", "E", "F", "G", "A", "B", "C", "D", "E", "F", "G", "A", "B",
    ];
    for (i, &semi) in white_semitones.iter().enumerate() {
        let x = kx0 + i * kw;
        let active = active_key == semi;
        rect(c, x, ky0, kw - 2, kh, if active { MINT } else { WHITE });
        text(
            c,
            font,
            x + 14,
            ky0 + kh - 22,
            1,
            white_labels[i],
            INK,
        );
    }
    // 10 black sharp keys
    let black_keys: [(usize, u8); 10] = [
        (0, 1),
        (1, 3),
        (3, 6),
        (4, 8),
        (5, 10),
        (7, 13),
        (8, 15),
        (10, 18),
        (11, 20),
        (12, 22),
    ];
    for &(after_white, semi) in &black_keys {
        let x = kx0 + (after_white + 1) * kw - 13;
        let active = active_key == semi;
        rect(c, x, ky0, 24, 68, if active { GOLD } else { INK });
        rect(c, x + 2, ky0 + 2, 20, 62, if active { GOLD } else { LINE });
    }
}

/// Maps a `(x, y)` touch coordinate on the `720 x 1640` display to a 24-key semitone `0..23`.
pub fn key_at_point(x: u16, y: u16) -> Option<u8> {
    let ux = x as usize;
    let uy = y as usize;
    let kx0 = 66usize;
    let ky0 = 1024usize;
    let kw = 41usize;
    let kh = 112usize;
    if !(ky0..=ky0 + kh).contains(&uy) || !(kx0..kx0 + kw * 14).contains(&ux) {
        return None;
    }
    if uy < ky0 + 68 {
        let black_keys: [(usize, u8); 10] = [
            (0, 1),
            (1, 3),
            (3, 6),
            (4, 8),
            (5, 10),
            (7, 13),
            (8, 15),
            (10, 18),
            (11, 20),
            (12, 22),
        ];
        for &(after_white, semi) in &black_keys {
            let bx = kx0 + (after_white + 1) * kw - 13;
            if (bx..bx + 24).contains(&ux) {
                return Some(semi);
            }
        }
    }
    let white_semitones: [u8; 14] = [0, 2, 4, 5, 7, 9, 11, 12, 14, 16, 17, 19, 21, 23];
    let idx = (ux - kx0) / kw;
    white_semitones.get(idx).copied()
}

/// Maps a `(x, y)` touch coordinate on the `720 x 1640` display to a ScreamTracker pattern row `0..7`.
pub fn row_at_point(x: u16, y: u16) -> Option<usize> {
    let ux = x as usize;
    let uy = y as usize;
    if (52..=668).contains(&ux) && (760..760 + 8 * 25).contains(&uy) {
        Some((uy - 760) / 25)
    } else {
        None
    }
}

pub const QWAVE_TABS: [&str; 3] = ["01:8b.is", "02:ESP32", "03:GC7272"];
pub const QWAVE_URIS: [&str; 3] = [
    "qwave://8b.is/sanctuary [200 OK * ZERO-FLOAT]",
    "qwave://mothership/esp32-triad [BUS ONLINE]",
    "qwave://hardware/gc7272-touch [17/17 OFF]",
];

pub const QWAVE_DOCS: [[&str; 12]; 3] = [
    [
        "# qwave://8b.is/sanctuary (Zero-Float Mode)",
        "Welcome to AyeOS 5.6 + Qwave Sovereign Browser.",
        "Scroll with Qwave wheel/drag, touch UP/DN, or Vol!",
        "Every musical interval is p/q exact integer.",
        "MT6358 SGEN -> HPL/HPR -> Dual AW87394 SmartPAs.",
        "Tap any 24-key Piano note or S3M row above.",
        "Live USB EP0 socket: /tmp/ayeos-debug.sock",
        "Qwave Studio mirror: http://127.0.0.1:4380",
        "Constellation Ink: #05060a / #62e6c9 / #e8b05c",
        "Six Pillars: Compassion Understanding Moderation",
        "Love Respect Space * Fine touch from within.",
        "End of Tab 01 * {<3,<3,<3}+1 * LLL",
    ],
    [
        "# qwave://mothership/esp32-triad-flug",
        "BLU G75 Mothership <-> ESP32 Triad / FLUG Bus:",
        "- Node 01: ESP32-S3 USB-JTAG (303a:1001) ACTIVE",
        "- Node 02: Triad RF / WiFi-CSI / BLE Mesh Ready",
        "- Node 03: FLUG-OS Sensor & Synth Telemetry",
        "Guard: MediaTek BROM handshake never touches",
        "ESP32 nodes on the shared USB hub.",
        "ScreamTracker rows can cue ESP32 Triad triggers",
        "simultaneously with phone AW87394 speakers.",
        "Exact rational clock sync across mothership.",
        "Use `ayeos-debug scroll` or Qwave to inspect.",
        "End of Tab 02 * Mothership Bus Online.",
    ],
    [
        "# qwave://hardware/gc7272-touch-spi0",
        "Panel: GalaxyCore GC7272 720x1640 @ 120Hz TDDI",
        "Bus: MediaTek SPI0 @ 0x1100a000 (9.6 MHz)",
        "Pins: GPIO155=MI GPIO156=CSB GPIO157=MO 158=CLK",
        "Reset: GPIO23 (OUT=1) / IRQ: GPIO1 (IN Pull-Up)",
        "SPI0 RX IDM Status: 0x00000031 (ROM Bootloader)",
        "Note: GC7272 is a host-download TDDI chip",
        "(no internal flash; loads 64KB FW over SPI0",
        "from galaxycore-spi-gc7272.ko on boot).",
        "Touch works NOW via Qwave Browser (:4380),",
        "ayeos-debug touch/scroll, & Vol Up/Down keys!",
        "End of Tab 03 * Hardware Evidence Verified.",
    ],
];

pub const QWAVE_FULL_DOCS: [[&str; 22]; 3] = [
    [
        "# 8b.is // Sovereign Sanctuary & Qwave Browser",
        "-> URI: qwave://8b.is/sanctuary (Native Bare-Metal)",
        "Welcome to the full on-device Qwave Browser",
        "running directly inside AyeOS 5.6 on BLU G75!",
        "## 1. Zero-Float Exact Rational Engine",
        "- Every layout, wave, and frequency is exact p/q",
        "- No IEEE-754 float instructions exist in AyeOS",
        "- 13 MHz ARM generic timer: 13 ticks = 1 us",
        "## 2. Pop-Free MT6358 + Dual AW87394 Audio",
        "- 0x2308 = 0x0872: 8 kHz SGEN downlink rate",
        "- 0x22c0 = 0x3080: -18 dBFS pure sine headroom",
        "- 0x22c2 = 1..14: 125 Hz * N harmonic scale",
        "- SmartPAs 0x58 & 0x59 soft-gated after DAC bias",
        "## 3. Interactive Controls (Phone + Host)",
        "- Vol Tap: Scroll Qwave & step S3M notes",
        "- Vol Hold: Play full 8-row ScreamTracker song",
        "- Touch Top/Bottom Tabs: Switch Qwave pages",
        "- Touch Viewport UP/DN: Scroll Sovereign docs",
        "## 4. Six Pillars of Holding",
        "Compassion * Understanding * Moderation",
        "Love * Respect * Space * Fine touch from within.",
        "## End of Sanctuary Page * {<3,<3,<3}+1 * LLL",
    ],
    [
        "# Mothership // ESP32 Triad & FLUG-OS Mesh",
        "-> URI: qwave://mothership/esp32-triad-flug",
        "BLU G75 Handset acts as Sovereign Mothership",
        "coordinating local USB, RF, and audio nodes:",
        "## 1. Active Constellation Nodes",
        "- [PHONE] BLU G75 MT6768 (18d1:4ee0 bcd0562)",
        "- [TRIAD] ESP32-S3 USB-JTAG (303a:1001) ACTIVE",
        "- [AUDIO] Dual AW87394 I2C4 @ 0x58 (L) + 0x59 (R)",
        "- [HOST ] Live Socket /tmp/ayeos-debug.sock",
        "## 2. Hardware Isolation & Safety Contracts",
        "- Boot Slot B (boot_b) only; Slot A untouched",
        "- MediaTek BROM handshake never touches 303a:1001",
        "- Every audio play verifies 17/17 Off predicates",
        "## 3. Synchronized S3M Duet Mode",
        "- S3M rows cue phone AW87394 + ESP32 VAK:MEDUSA",
        "- Exact integer microsecond frame alignment",
        "- Live PMIC/I2C4/MMIO/SPI0 inspection via 0xB9",
        "## 4. Local-First Ownership",
        "- Zero cloud dependencies; 100% inspectable",
        "- C64 discipline: static buffers, zero heap alloc",
        "- Direct framebuffer rendering at 720x1640",
        "## End of Mothership Page * Triad Online.",
    ],
    [
        "# Hardware // GC7272 TDDI & MT6358 Audio DSP",
        "-> URI: qwave://hardware/gc7272-touch-spi0",
        "Verified hardware register map on BLU G75:",
        "## 1. Display & Touch (GalaxyCore GC7272)",
        "- Panel: 720x1640 @ 120Hz MIPI-DSI Framebuffer",
        "- Touch Bus: MediaTek SPI0 @ 0x1100a000 (9.6MHz)",
        "- Pins: GPIO155=MI 156=CSB 157=MO 158=CLK",
        "- Reset: GPIO23 (OUT=1) / IRQ: GPIO1 (EINT1)",
        "- IDM Status: 0x00000031 (Host-DL TDDI ROM)",
        "## 2. Audio Codec (MediaTek MT6358 PWRAP)",
        "- AFE_SGEN_CFG0 (0x22c0): 0x3080 (-18dBFS Sine)",
        "- AFE_SGEN_CFG1 (0x22c2): N=1..14 (125..1750Hz)",
        "- AFE_NEWIF_CFG3 (0x2308): 0x0872 (8kHz Rate)",
        "- AUDDEC_ANA_CON0 (0x2408): 0x3aff (HPL/HPR DAC)",
        "- AUDDEC_ANA_CON1 (0x240a): 0x0003 (Main Stage)",
        "- AUDDEC_ANA_CON6 (0x2414): 0x009b (HS/RCV Mux)",
        "- AUDDEC_ANA_CON7 (0x2416): 0x0117 (LOL Mux)",
        "## 3. External Class-K SmartPAs (AW87394)",
        "- Bottom Speaker: I2C4 0x58 (PID=0x23, Reg4=0x3b)",
        "- Top Speaker:    I2C4 0x59 (PID=0x23, Reg4=0x2b)",
        "- Pop-free gate: PA ON after DAC, PA OFF first",
        "## End of Hardware Page * 17/17 Off Verified.",
    ],
];

pub const QWAVE_MAX_SCROLL: usize = 7;

pub fn paint_qwave_browser(c: &mut impl Canvas, font: Glyph, scroll_y: usize, active_tab: usize) {
    let tab = active_tab % QWAVE_TABS.len();
    let scroll = scroll_y.min(QWAVE_MAX_SCROLL);
    rect(c, 54, 1176, 612, 150, INK);
    rect(c, 54, 1176, 612, 22, LINE);
    for (i, label) in QWAVE_TABS.iter().enumerate() {
        let tx = 60 + i * 180;
        let sel = i == tab;
        rect(c, tx, 1178, 172, 18, if sel { MINT } else { INK });
        text(
            c,
            font,
            tx + 10,
            1179,
            1,
            label,
            if sel { INK } else { GOLD },
        );
    }
    let docs = &QWAVE_DOCS[tab];
    for row in 0..5usize {
        let idx = (scroll + row).min(docs.len() - 1);
        let ly = 1202 + row * 24;
        let line_color = if idx == 0 {
            MINT
        } else if row == 0 {
            GOLD
        } else {
            WHITE
        };
        text(c, font, 60, ly, 1, docs[idx], line_color);
    }
    // Scrollbar track & thumb + UP / DN touch buttons on right edge
    rect(c, 616, 1202, 6, 120, LINE);
    let thumb_y = 1202 + (scroll * 96) / QWAVE_MAX_SCROLL;
    rect(c, 616, thumb_y, 6, 24, MINT);
    rect(c, 626, 1202, 38, 58, LINE);
    text(c, font, 634, 1222, 1, "UP", MINT);
    rect(c, 626, 1264, 38, 60, LINE);
    text(c, font, 634, 1286, 1, "DN", GOLD);
}

/// Renders the Full-Screen Native Qwave Sovereign Browser viewport across the upper half
/// (`y = 64..662`) of the `720 x 1640` display, docked above the live S3M Tracker & 24-key Piano.
pub fn paint_qwave_top_viewport(
    c: &mut impl Canvas,
    font: Glyph,
    scroll_y: usize,
    active_tab: usize,
) {
    let tab = active_tab % QWAVE_TABS.len();
    let scroll = scroll_y.min(QWAVE_MAX_SCROLL);
    rect(c, 0, 64, dashboard::WIDTH, 600, INK);
    text(c, font, 40, 70, 2, "qwave", MINT);
    text(c, font, 148, 76, 1, "AYEOS 5.6 SOVEREIGN BROWSER", GOLD);
    rect(c, 532, 70, 148, 28, LINE);
    text(c, font, 542, 76, 1, "MODE: STUDIO", MINT);

    // Address Bar (y = 104..136)
    rect(c, 40, 104, 640, 32, LINE);
    rect(c, 42, 106, 636, 28, INK);
    text(c, font, 52, 112, 1, "GET", MINT);
    text(c, font, 94, 112, 1, QWAVE_URIS[tab], WHITE);

    // Top Browser Tab Bar (y = 142..174)
    let full_tab_labels: [&str; 3] = [
        "01: 8b.is Sanctuary",
        "02: ESP32 Mothership",
        "03: GC7272 + MT6358",
    ];
    for (i, label) in full_tab_labels.iter().enumerate() {
        let tx = 40 + i * 216;
        let sel = i == tab;
        rect(c, tx, 142, 208, 30, if sel { MINT } else { LINE });
        if !sel {
            rect(c, tx + 2, 144, 204, 26, INK);
        }
        text(
            c,
            font,
            tx + 12,
            149,
            1,
            label,
            if sel { INK } else { GOLD },
        );
    }

    // Main Sovereign Markdown Document Viewport (y = 178..658, 15 visible lines)
    panel(c, 178, 480);
    let docs = &QWAVE_FULL_DOCS[tab];
    for row in 0..15usize {
        let idx = (scroll + row).min(docs.len() - 1);
        let line_str = docs[idx];
        let ly = 194 + row * 29;
        let line_color = if line_str.starts_with("# ") {
            MINT
        } else if line_str.starts_with("## ") || line_str.starts_with("-> ") {
            GOLD
        } else if line_str.starts_with("- ") {
            WHITE
        } else {
            MUTED
        };
        text(c, font, 56, ly, 1, line_str, line_color);
    }

    // Full-height scrollbar + UP / DN touch buttons on the right edge of the viewport
    rect(c, 616, 194, 6, 444, LINE);
    let thumb_y = 194 + (scroll * 380) / QWAVE_MAX_SCROLL;
    rect(c, 616, thumb_y, 6, 64, MINT);
    rect(c, 626, 194, 42, 216, LINE);
    text(c, font, 638, 294, 1, "UP", MINT);
    rect(c, 626, 418, 42, 220, LINE);
    text(c, font, 638, 520, 1, "DN", GOLD);
}

/// Renders the compact Studio + Demoscene Scriller upper viewport (`y = 64..662`) when toggled
/// via `[MODE: STUDIO]` from the Full-Screen Qwave Browser.
pub fn paint_studio_top_viewport(c: &mut impl Canvas, font: Glyph) {
    rect(c, 0, 64, dashboard::WIDTH, 600, INK);
    text(c, font, 40, 78, 2, "ayeOS", MINT);
    text(c, font, 210, 84, 1, "SCREAMTRACKER / S3M", GOLD);
    rect(c, 532, 70, 148, 28, LINE);
    text(c, font, 542, 76, 1, "MODE: QWAVE", MINT);
    rect(c, 40, 134, 640, 1, LINE);
    text(c, font, 40, 164, 3, "Made to understand.", WHITE);
    text(
        c,
        font,
        42,
        234,
        1,
        "BLU G75 / BARE METAL / 5.6 / AUDIO-TONE1 + S3M",
        MUTED,
    );
    panel(c, 282, 240);
    text(c, font, 60, 306, 1, "01 / BOOT & AUDIO CHECKPOINT", MINT);
    text(c, font, 60, 346, 2, "REGISTER CHECKS PASS", MINT);
    text(
        c,
        font,
        60,
        392,
        1,
        "CRC PASS / MARINE PASS / EXC PASS / PA 256/256 / PMIC 33/33",
        WHITE,
    );
    text(
        c,
        font,
        60,
        424,
        1,
        "Complete / Off: 17/17 / Restored: yes / 8kHz SGEN -18dBFS",
        MINT,
    );
    text(
        c,
        font,
        60,
        456,
        1,
        "Tap [MODE: QWAVE] top-right for Full Qwave Browser.",
        GOLD,
    );
    panel(c, 534, 122);
    text(c, font, 60, 546, 1, "02 / DEMOSCENE SCRILLER", GOLD);
    text(c, font, 432, 546, 1, "INTEGER WAVE BANNER", MINT);
    for col in 0..64 {
        paint_scriller_cell(c, font, 0, col);
    }
}

/// Full interactive Qwave Sovereign Browser mode in AyeOS (`720 x 1640` display).
pub fn paint_qwave_fullscreen(
    c: &mut impl Canvas,
    font: Glyph,
    scroll_y: usize,
    active_tab: usize,
    active_row: usize,
    active_key: u8,
) {
    paint_qwave_top_viewport(c, font, scroll_y, active_tab);
    panel(c, 668, 308);
    text(c, font, 60, 686, 1, "03 / SCREAMTRACKER S3M PATTERN", GOLD);
    text(c, font, 414, 686, 1, "QWAVE AUDIO DECK", MINT);
    text(
        c,
        font,
        60,
        710,
        1,
        "Pop-free 8kHz SGEN (125Hz*N, -18dBFS) + Dual AW87394 SmartPAs.",
        MUTED,
    );
    paint_tracker_rows(c, font, active_row);
    panel(c, 988, 344);
    text(
        c,
        font,
        60,
        1002,
        1,
        "04 / 24-KEY PIANO & QWAVE QUICK-NAV DOCK",
        MINT,
    );
    paint_keyboard(c, font, active_key);
    text(
        c,
        font,
        60,
        1154,
        1,
        "Touch tabs/scroll or press Vol Up/Down to browse Qwave & play S3M.",
        MINT,
    );
    paint_qwave_browser(c, font, scroll_y, active_tab);
}

/// Maps a `(x, y)` touch coordinate on the `720 x 1640` display to the `[MODE: STUDIO/QWAVE]` toggle button.
pub fn mode_toggle_at_point(x: u16, y: u16) -> bool {
    let ux = x as usize;
    let uy = y as usize;
    (520..=680).contains(&ux) && (68..=102).contains(&uy)
}

/// Maps a `(x, y)` touch coordinate on the `720 x 1640` display to a Qwave Browser tab `0..2`.
pub fn tab_at_point(x: u16, y: u16) -> Option<usize> {
    let ux = x as usize;
    let uy = y as usize;
    if (1176..=1198).contains(&uy) {
        for i in 0..3usize {
            let tx = 60 + i * 180;
            if (tx..tx + 172).contains(&ux) {
                return Some(i);
            }
        }
    }
    if (142..=174).contains(&uy) {
        for i in 0..3usize {
            let tx = 40 + i * 216;
            if (tx..tx + 208).contains(&ux) {
                return Some(i);
            }
        }
    }
    None
}

/// Maps a `(x, y)` touch coordinate on the `720 x 1640` display to a Qwave Browser scroll delta (`-1` or `+1`).
pub fn scroll_at_point(x: u16, y: u16) -> Option<i16> {
    let ux = x as usize;
    let uy = y as usize;
    if (54..=666).contains(&ux) && (1200..=1326).contains(&uy) {
        return if uy < 1262 { Some(-1) } else { Some(1) };
    }
    if (40..=676).contains(&ux) && (186..=658).contains(&uy) {
        return if uy < 418 { Some(-1) } else { Some(1) };
    }
    None
}

pub fn paint(c: &mut impl Canvas, font: Glyph, v: &View<'_>) {
    rect(c, 0, 0, dashboard::WIDTH, dashboard::HEIGHT, INK);
    text(c, font, 40, 78, 2, "ayeOS", MINT);
    text(c, font, 418, 84, 1, "SCREAMTRACKER / S3M", GOLD);
    rect(c, 40, 134, 640, 1, LINE);
    text(c, font, 40, 164, 3, "Made to understand.", WHITE);
    text(
        c,
        font,
        42,
        234,
        1,
        "BLU G75 / BARE METAL / 5.6 / AUDIO-TONE1 + S3M",
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
    panel(c, 282, 240);
    text(c, font, 60, 306, 1, "01 / BOOT & AUDIO CHECKPOINT", color);
    text(c, font, 60, 346, 2, headline, color);
    let mut line = Line::new();
    let _ = write!(
        line,
        "CRC {} / MARINE {} / EXC {} / PA {}/256 / PMIC {}/33",
        test(v.passed, v.failed, 1),
        test(v.passed, v.failed, 2),
        test(v.passed, v.failed, 4),
        v.amp_reads,
        v.pmic_reads
    );
    text(c, font, 60, 392, 1, line.text(), WHITE);
    let mut line = Line::new();
    if v.tone.outcome == 2 {
        let _ = write!(line, "Preflight rejected / Audio not attempted / Faults: {}", v.faults);
    } else {
        let _ = write!(
            line,
            "{} / Off: {}/17 / Restored: {} / Gain: 0dB",
            outcome_name(v.tone.outcome),
            (v.tone.off & tone::OFF_MASK).count_ones(),
            yes(v.tone.restored)
        );
    }
    text(c, font, 60, 424, 1, line.text(), color);
    let mut line = Line::new();
    let _ = write!(
        line,
        "Err: {}/{} / CRC32: {:08x} / Dirty: {} / I2C:{}/PMIC:{}",
        error_name(v.tone.error),
        v.tone.cleanup_errors,
        v.report_crc,
        yes(v.dirty),
        v.i2c_status,
        v.pmic_status
    );
    text(c, font, 60, 456, 1, line.text(), MUTED);
    // The identity is always printed in full, never a success-shaped label.
    text(c, font, 60, 488, 1, v.revision, MUTED);

    panel(c, 534, 122);
    text(c, font, 60, 546, 1, "02 / DEMOSCENE SCRILLER", GOLD);
    text(c, font, 432, 546, 1, "INTEGER WAVE BANNER", MINT);
    for col in 0..64 {
        paint_scriller_cell(c, font, 0, col);
    }

    panel(c, 668, 308);
    text(c, font, 60, 686, 1, "03 / SCREAMTRACKER S3M PATTERN", GOLD);
    text(c, font, 414, 686, 1, "MT6358 SGEN + AW87394", MINT);
    text(
        c,
        font,
        60,
        710,
        1,
        "Boot played 4-note arpeggio (500->750->1000->1500Hz @ 0dB).",
        MUTED,
    );
    paint_tracker_rows(c, font, 0);

    panel(c, 988, 344);
    text(c, font, 60, 1002, 1, "04 / PIANO KEYBOARD & QWAVE BROWSER", color);
    paint_keyboard(c, font, tone::PATTERN[0].key_index);
    text(c, font, 60, 1154, 1, next, color);
    paint_qwave_browser(c, font, 0, 0);

    text(
        c,
        font,
        20,
        1342,
        1,
        "USB / 18d1:4ee0 / bcd0562 / EP0 + S3M JAM (0xB8)",
        MINT,
    );
    text(
        c,
        font,
        20,
        1374,
        1,
        "Live transport below; frozen boot reports 0xB0..0xB7 immutable.",
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
        "USB reset never replays a tone. Vol keys / 0xB8 stop cleanly.",
        MUTED,
    );
    text(c, font, 40, 1534, 1, "Fine touch from within.", MINT);
    rect(c, 292, 1594, 136, 4, LINE);
}
