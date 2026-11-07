//! Constellation Studio: integer-only 720x1640 framebuffer composition.
//! No allocation, hardware probing, network calls or inferred success states.
//! RGB channels are 8-bit integers; geometric divisions round toward zero.

pub const WIDTH: usize = 720;
pub const HEIGHT: usize = 1640;
pub(crate) const INK: u32 = 0xFF05_060A;
pub(crate) const PANEL: u32 = 0xFF10_151D;
pub(crate) const LINE: u32 = 0xFF26_303C;
pub(crate) const WHITE: u32 = 0xFFEF_F5F4;
pub(crate) const MUTED: u32 = 0xFFA3_B2C2;
pub(crate) const MINT: u32 = 0xFF62_E6C9;
pub(crate) const GOLD: u32 = 0xFFE8_B05C;
const VIOLET: u32 = 0xFFB4_8BFF;

pub trait Canvas {
    fn pixel(&mut self, x: usize, y: usize, color: u32);
    fn fill_rect(&mut self, x: usize, y: usize, w: usize, h: usize, color: u32) {
        for py in y..y.saturating_add(h).min(HEIGHT) {
            for px in x..x.saturating_add(w).min(WIDTH) {
                self.pixel(px, py, color);
            }
        }
    }
}

pub(crate) type Glyph = fn(u8) -> [u8; 16];

#[inline(always)]
pub(crate) const fn blend_half(c1: u32, c2: u32) -> u32 {
    0xFF00_0000 | (((c1 & 0x00FE_FEFE) >> 1) + ((c2 & 0x00FE_FEFE) >> 1))
}

#[inline(always)]
pub(crate) fn rect(c: &mut impl Canvas, x: usize, y: usize, w: usize, h: usize, color: u32) {
    c.fill_rect(x, y, w, h, color);
}

pub(crate) fn panel(c: &mut impl Canvas, y: usize, h: usize) {
    if h <= 32 {
        c.fill_rect(36, y, 648, h, PANEL);
        return;
    }
    // Fast scanline span fill for 16px rounded corners + single bulk fill for interior
    const CORNER_INSET: [usize; 16] = [16, 11, 8, 6, 5, 4, 3, 2, 2, 1, 1, 1, 0, 0, 0, 0];
    for ry in 0..16usize {
        let inset = CORNER_INSET[ry];
        c.fill_rect(36 + inset, y + ry, 648 - 2 * inset, 1, PANEL);
        c.fill_rect(36 + inset, y + h - 1 - ry, 648 - 2 * inset, 1, PANEL);
    }
    c.fill_rect(36, y + 16, 648, h - 32, PANEL);
    // Crisp 1px top/bottom HD highlight border
    c.fill_rect(52, y, 616, 1, LINE);
    c.fill_rect(52, y + h - 1, 616, 1, LINE);
}

pub(crate) fn text(
    c: &mut impl Canvas,
    font: Glyph,
    x: usize,
    y: usize,
    scale: usize,
    s: &str,
    color: u32,
) {
    let aa_col = blend_half(color, PANEL);
    for (i, ch) in s.bytes().enumerate() {
        let rows = font(ch);
        let gx = x + i * 9 * scale;
        if gx >= WIDTH {
            break;
        }
        for (row, &bits) in rows.iter().enumerate() {
            let up = if row > 0 { rows[row - 1] } else { 0 };
            let dn = if row < 15 { rows[row + 1] } else { 0 };
            if bits == 0 && up == 0 && dn == 0 {
                continue;
            }
            for col in 0..8usize {
                let mask = 0x80u8 >> col;
                if bits & mask != 0 {
                    c.fill_rect(gx + col * scale, y + row * scale, scale, scale, color);
                } else if scale >= 2 {
                    let l_on = col > 0 && (bits & (mask << 1)) != 0;
                    let r_on = col < 7 && (bits & (mask >> 1)) != 0;
                    let u_on = (up & mask) != 0;
                    let d_on = (dn & mask) != 0;
                    if l_on && u_on {
                        c.pixel(gx + col * scale, y + row * scale, aa_col);
                    }
                    if r_on && u_on {
                        c.pixel(gx + col * scale + scale - 1, y + row * scale, aa_col);
                    }
                    if l_on && d_on {
                        c.pixel(gx + col * scale, y + row * scale + scale - 1, aa_col);
                    }
                    if r_on && d_on {
                        c.pixel(
                            gx + col * scale + scale - 1,
                            y + row * scale + scale - 1,
                            aa_col,
                        );
                    }
                }
            }
        }
    }
}

/// High-resolution Pinch-Zoomable text renderer with sub-pixel corner anti-aliasing:
/// - `zoom == 0` (100%): 8x16 crisp HD glyphs (9px advance, 16px tall)
/// - `zoom == 1` (150%): 12x24 3:2 sub-pixel interpolated HD glyphs (13px advance, 24px tall)
/// - `zoom >= 2` (200%): 16x32 2x2 sub-pixel corner-smoothed HD glyphs (18px advance, 32px tall)
pub(crate) fn text_zoom(
    c: &mut impl Canvas,
    font: Glyph,
    x: usize,
    y: usize,
    zoom: usize,
    s: &str,
    color: u32,
) {
    if zoom == 0 {
        text(c, font, x, y, 1, s, color);
        return;
    }
    if zoom >= 2 {
        text(c, font, x, y, 2, s, color);
        return;
    }
    // zoom == 1 (150% HD: 12x24 sub-pixel interpolated from 8x16)
    let aa_col = blend_half(color, PANEL);
    // Horizontal 8 -> 12 mapping: columns 0, 2, 4, 6 are 1px; columns 1, 3, 5, 7 are 2px (total 12px + 2px spacing = 14px)
    const COL_X: [usize; 8] = [0, 1, 3, 4, 6, 7, 9, 10];
    const COL_W: [usize; 8] = [1, 2, 1, 2, 1, 2, 1, 2];
    const ROW_Y: [usize; 16] = [0, 1, 3, 4, 6, 7, 9, 10, 12, 13, 15, 16, 18, 19, 21, 22];
    const ROW_H: [usize; 16] = [1, 2, 1, 2, 1, 2, 1, 2, 1, 2, 1, 2, 1, 2, 1, 2];
    for (i, ch) in s.bytes().enumerate() {
        let gx = x + i * 14;
        if gx + 12 >= WIDTH {
            break;
        }
        let rows = font(ch);
        for (row, &bits) in rows.iter().enumerate() {
            let up = if row > 0 { rows[row - 1] } else { 0 };
            let dn = if row < 15 { rows[row + 1] } else { 0 };
            if bits == 0 && up == 0 && dn == 0 {
                continue;
            }
            let py = y + ROW_Y[row];
            let ph = ROW_H[row];
            for col in 0..8usize {
                let mask = 0x80u8 >> col;
                let px = gx + COL_X[col];
                let pw = COL_W[col];
                if bits & mask != 0 {
                    c.fill_rect(px, py, pw, ph, color);
                } else {
                    let l_on = col > 0 && (bits & (mask << 1)) != 0;
                    let r_on = col < 7 && (bits & (mask >> 1)) != 0;
                    let u_on = (up & mask) != 0;
                    let d_on = (dn & mask) != 0;
                    if (l_on || r_on) && (u_on || d_on) {
                        c.pixel(px, py, aa_col);
                    }
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
