//! Frozen inherited-state diagnostic. Fixed-size, no heap, no USB-triggered IO.
use crate::{audio_probe, board_pages::crc32, pmic_probe, usb_control::Io};
pub const BYTES: usize = 6400;
pub const AMP: usize = 64;
pub const PMIC: usize = 1344;
pub const READS: usize = 1376;
pub const END: usize = 1541;
pub struct Report {
    pub words: [u32; BYTES / 4],
}
pub fn run(io: &mut impl Io, blob: &[u8]) -> Report {
    let mut r = Report {
        words: [0; BYTES / 4],
    };
    r.words[..8].copy_from_slice(&[
        u32::from_le_bytes(*b"A6RP"),
        1,
        BYTES as u32,
        crc32(blob),
        256,
        33,
        audio_probe::BUDGET,
        1,
    ]);
    for entry in r.words[AMP..PMIC].chunks_exact_mut(5) {
        entry.fill(u32::MAX);
        entry[0] = 0;
    }
    for entry in r.words[READS..END].chunks_exact_mut(5) {
        entry.fill(u32::MAX);
        entry[0] = 0;
    }
    let amps = audio_probe::run_window(io, blob, &mut r.words[AMP..PMIC]);
    r.words[16..AMP].copy_from_slice(&amps.words);
    let (head, entries) = r.words[PMIC..END].split_at_mut(32);
    pmic_probe::run(io, blob, head, entries);
    r
}
impl Report {
    pub fn encode(&self) -> [u8; BYTES] {
        let mut out = [0; BYTES];
        for (b, w) in out.chunks_exact_mut(4).zip(self.words.iter()) {
            b.copy_from_slice(&w.to_le_bytes());
        }
        out
    }
}
