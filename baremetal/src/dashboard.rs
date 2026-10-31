//! Constellation Studio: integer-only 720x1640 framebuffer composition.
//! No allocation, hardware probing, network calls or inferred success states.
//! RGB channels are 8-bit integers; geometric divisions round toward zero.

pub const WIDTH: usize = 720;
pub const HEIGHT: usize = 1640;
const INK: u32 = 0xFF05_060A;
const PANEL: u32 = 0xFF10_151D;
const LINE: u32 = 0xFF26_303C;
const WHITE: u32 = 0xFFEF_F5F4;
const MUTED: u32 = 0xFFA3_B2C2;
const MINT: u32 = 0xFF62_E6C9;
const GOLD: u32 = 0xFFE8_B05C;
const VIOLET: u32 = 0xFFB4_8BFF;

pub trait Canvas {
    fn pixel(&mut self, x: usize, y: usize, color: u32);
}

type Glyph = fn(u8) -> [u8; 16];

fn rect(c: &mut impl Canvas, x: usize, y: usize, w: usize, h: usize, color: u32) {
    for py in y..y.saturating_add(h).min(HEIGHT) {
        for px in x..x.saturating_add(w).min(WIDTH) {
            c.pixel(px, py, color);
        }
    }
}

fn panel(c: &mut impl Canvas, y: usize, h: usize) {
    // 16px rounded corners using integer circle coverage.
    for py in 0..h {
        for px in 0..648usize {
            let dx = 16usize.saturating_sub(px.min(647 - px));
            let dy = 16usize.saturating_sub(py.min(h - 1 - py));
            if dx * dx + dy * dy <= 256 {
                c.pixel(36 + px, y + py, PANEL);
            }
        }
    }
}

fn text(c: &mut impl Canvas, font: Glyph, x: usize, y: usize, scale: usize, s: &str, color: u32) {
    for (i, ch) in s.bytes().enumerate() {
        for (row, bits) in font(ch).iter().enumerate() {
            for col in 0..8 {
                if bits & (0x80 >> col) != 0 {
                    rect(
                        c,
                        x + (i * 9 + col) * scale,
                        y + row * scale,
                        scale,
                        scale,
                        color,
                    );
                }
            }
        }
    }
}

pub fn paint(c: &mut impl Canvas, font: Glyph, moving: bool) {
    rect(c, 0, 0, WIDTH, HEIGHT, INK);
    // The camera cutout occupies the top 60px; all text begins below it.
    text(c, font, 40, 78, 2, "ayeOS", MINT);
    text(c, font, 454, 78, 1, "5.4 / STUDIO PREVIEW", MUTED);
    rect(c, 40, 134, 640, 1, LINE);
    text(c, font, 40, 170, 4, "A little space.", WHITE);
    text(c, font, 42, 268, 2, "Yours to explore.", MUTED);

    panel(c, 352, 422);
    text(c, font, 60, 376, 1, "01 / QUIET FIELD", MINT);
    text(c, font, 492, 376, 1, "GENERATIVE DEMO", MUTED);
    artwork(c, 0);
    text(c, font, 60, 720, 1, "INTEGER ART / NO SENSOR INPUT", MUTED);
    text(
        c,
        font,
        510,
        720,
        1,
        if moving { "MOTION ON" } else { "STILL MODE" },
        MINT,
    );

    panel(c, 802, 218);
    text(c, font, 60, 826, 1, "02 / THIS SESSION", MINT);
    text(c, font, 60, 870, 2, "Bare-metal workspace", WHITE);
    text(c, font, 60, 920, 1, "BLU G75 / AARCH64 / 720 x 1640", MUTED);
    text(c, font, 60, 964, 1, "RENDER LOOPS", MUTED);
    counter(c, font, 0);

    panel(c, 1048, 350);
    text(c, font, 60, 1072, 1, "03 / CAPABILITY NOTES", GOLD);
    text(c, font, 60, 1116, 2, "Room to grow.", WHITE);
    for (y, label, state) in [
        (1178, "FM / RDS", "DEMO ONLY"),
        (1226, "CAMERA / SPEECH", "DEMO ONLY"),
        (1274, "WI-FI / HOTSPOT", "NOT VERIFIED"),
        (1322, "USB UPDATER", "EXPERIMENTAL"),
    ] {
        rect(c, 60, y - 12, 600, 1, LINE);
        text(c, font, 60, y, 1, label, MUTED);
        text(c, font, 450, y, 1, state, GOLD);
    }

    text(c, font, 40, 1440, 1, "VOLUME TAP  /  STILL OR MOTION", MINT);
    text(
        c,
        font,
        40,
        1476,
        1,
        "VOLUME HOLD /  REBOOT FOR USB UPDATER",
        MUTED,
    );
    text(c, font, 40, 1534, 1, "Fine touch from within.", MUTED);
    text(c, font, 490, 1534, 1, "{<3,<3,<3}+1", VIOLET);
    rect(c, 292, 1594, 136, 4, LINE);
}

/// Layered quartic ribbons, not sampled waves. Phase is reduced to 0..255;
/// maximum intermediate product is 8,100,000,000 (i64). No sine accuracy claim.
pub fn artwork(c: &mut impl Canvas, phase: u16) {
    rect(c, 60, 420, 600, 272, PANEL);
    for (layer, color) in [LINE, VIOLET, MINT, GOLD].iter().enumerate() {
        for x in 0..600usize {
            let t = ((x + usize::from(phase & 255) + layer * 67) % 600) as i64 - 300;
            let arch = (90000 - t * t) * (90000 - t * t) / 90000000;
            let y = (632 - arch - layer as i64 * 29) as usize;
            rect(c, 60 + x, y, 1, if layer == 2 { 3 } else { 1 }, *color);
        }
    }
    // Small fixed constellation, deliberately not a signal-strength indicator.
    for (x, y) in [(108, 450), (552, 452), (592, 490), (218, 476)] {
        rect(c, x, y, 3, 3, MUTED);
    }
}

pub fn counter(c: &mut impl Canvas, font: Glyph, frame: u64) {
    rect(c, 384, 960, 270, 24, PANEL);
    let mut bytes = [b'0'; 20];
    let mut n = frame;
    let mut start = 19;
    loop {
        bytes[start] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
        start -= 1;
    }
    // Every byte above is an ASCII digit.
    if let Ok(s) = core::str::from_utf8(&bytes[start..]) {
        text(c, font, 654 - s.len() * 9, 964, 1, s, WHITE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Bounds;
    impl Canvas for Bounds {
        fn pixel(&mut self, x: usize, y: usize, _: u32) {
            assert!(
                x < WIDTH && y < HEIGHT,
                "pixel outside framebuffer: {x},{y}"
            );
        }
    }
    fn solid(_: u8) -> [u8; 16] {
        [255; 16]
    }
    #[test]
    fn all_motion_phases_and_maximum_counter_fit() {
        let mut c = Bounds;
        paint(&mut c, solid, false);
        paint(&mut c, solid, true);
        for phase in 0..=255 {
            artwork(&mut c, phase);
        }
        counter(&mut c, solid, u64::MAX);
    }
}
