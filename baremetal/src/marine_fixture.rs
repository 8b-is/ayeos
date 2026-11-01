//! Deterministic PCM fixture. Samples are generated in integer arithmetic;
//! neither microphone input nor acoustic frequency/rate is implied.
pub const SAMPLES: usize = 1024;
// Obtained by running the byte-identical upstream reference (host test), not
// inferred from the adapted output. fixture_reference_hash verifies both.
pub const REFERENCE_HASH: u64 = 0x800d_3c2f_169c_4722;
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
pub fn hash_bytes(mut hash: u64, bytes: &[u8]) -> u64 {
    for b in bytes {
        hash = (hash ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3);
    }
    hash
}
pub fn sample(i: usize) -> i16 {
    match i {
        0..=127 | 768..=895 => 0,
        128..=511 | 896..=1023 => {
            let phase = (i % 32) as i32;
            let triangle = if phase < 16 { phase } else { 32 - phase };
            (triangle * 2048 - 16384) as i16
        }
        _ => (((i as u32).wrapping_mul(1103515245).wrapping_add(12345) >> 16) as u16) as i16,
    }
}

pub fn run() -> (u64, usize) {
    let mut marine = marine_core::Marine::new();
    let mut hash = FNV_OFFSET;
    for i in 0..SAMPLES {
        let Some(score) = marine.process(sample(i)) else {
            return (hash, i);
        };
        hash = hash_bytes(hash, &score.num.to_le_bytes());
        hash = hash_bytes(hash, &score.den.to_le_bytes());
    }
    (hash, SAMPLES)
}
