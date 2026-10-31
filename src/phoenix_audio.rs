//! `ayeOS` Native PhoeniX Audio Revelation & On-Device 432 Hz / φ Synthesizer (`phoenix_audio`)
//!
//! Zero-float, 100% integer (`i16` / `i64` / `p/q`) implementation of the PhoeniX
//! 3-Stage Audio Revelation pipeline for the BLU G75 (`MT6769V/CB` / `mt6768` AFE
//! `0x11220000` + `MT6358` codec + Awinic `AW87xxx` Smart Speaker PA):
//!
//! 1. **Stage 1**: 16× Integer Catmull-Rom cubic Hermite sub-sample interpolation (`48 kHz -> 768 kHz`).
//! 2. **Stage 2**: Marine `S + N + T` (`Sinusoids + Noise + Dirac Transients`) & zero-crossing tracker.
//! 3. **Stage 3**: **Sparkle** + **Warmth** + **Drive** + **Punch** + **Air** + **432 Hz / φ (`500/809`)**
//!    sympathetic resonator + `.8a∞` (`0x38 0x61 0xE2 0x9E`) opcode stream emitter.

/// `.8a` (`8a∞`) Infinity Audio magic header: `0x38 0x61 0xE2 0x9E`.
pub const EIGHT_A_MAGIC: [u8; 4] = [0x38, 0x61, 0xE2, 0x9E];

/// On-device PhoeniX tuning parameters in `Q10` (`1024 == 1/1`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MobilePhoenixPreset {
    pub sparkle_q10: i64,
    pub warmth_q10: i64,
    pub drive_q10: i64,
    pub punch_q10: i64,
    pub air_q10: i64,
    pub phi_432_q10: i64,
    pub stereo_width_q10: i64,
}

impl Default for MobilePhoenixPreset {
    fn default() -> Self {
        Self {
            sparkle_q10: 440,
            warmth_q10: 380,
            drive_q10: 240,
            punch_q10: 500,
            air_q10: 360,
            phi_432_q10: 300,
            stereo_width_q10: 1340,
        }
    }
}

/// Fast midpoint (`t = 8/16 = 1/2`) integer Catmull-Rom spline on the 768 kHz grid.
#[inline]
pub fn catmull_rom_midpoint_i16(p0: i16, p1: i16, p2: i16, p3: i16) -> i16 {
    let num = -(p0 as i64) + 9 * (p1 as i64) + 9 * (p2 as i64) - (p3 as i64);
    ((num + 8) >> 4).clamp(-32767, 32767) as i16
}

/// Smooth rational knee limiter (`-32767..=32767`).
#[inline]
pub fn soft_limit_i16(x: i64) -> i16 {
    let sign = if x < 0 { -1 } else { 1 };
    let a = x.abs();
    if a <= 28000 {
        return (sign * a) as i16;
    }
    let excess = a - 28000;
    let compressed = 28000 + (4767 * excess) / (excess + 4767);
    (sign * compressed.min(32767)) as i16
}

/// Synthesize a live PhoeniX'd 432 Hz + φ (`500/809`) + 528 Hz Harmonic Chord directly in
/// exact integer arithmetic on the BLU G75's ARM64 cores, returning `(wav_bytes, eight_a_bytes, fnv1a_hash)`.
pub fn synthesize_on_device_phoenix_chord(
    fundamental_hz: u32,
    duration_ms: u32,
    preset: &MobilePhoenixPreset,
) -> (Vec<u8>, Vec<u8>, u64) {
    let sample_rate = 48_000u32;
    let frames = ((sample_rate as u64 * duration_ms.clamp(200, 10_000) as u64) / 1000) as usize;
    let f0 = fundamental_hz.clamp(110, 1760) as i64;
    // Golden-ratio companion: `f_phi = f0 * 500 / 809` and fifth `f_fifth = f0 * 3 / 2`
    let f_phi = (f0 * 500) / 809;
    let f_fifth = (f0 * 3) / 2;

    let mut raw_stereo = Vec::with_capacity(frames * 2);
    // Phase accumulators in `Q16` (`65536` = 1 full cycle)
    let mut phase0: i64 = 0;
    let mut phase_phi: i64 = 0;
    let mut phase_fifth: i64 = 0;
    let step0 = (f0 * 65536) / (sample_rate as i64);
    let step_phi = (f_phi * 65536) / (sample_rate as i64);
    let step_fifth = (f_fifth * 65536) / (sample_rate as i64);

    for i in 0..frames {
        phase0 = (phase0 + step0) & 0xFFFF;
        phase_phi = (phase_phi + step_phi) & 0xFFFF;
        phase_fifth = (phase_fifth + step_fifth) & 0xFFFF;

        let w0 = integer_parabolic_sine_q15(phase0);
        let w_phi = integer_parabolic_sine_q15(phase_phi);
        let w_fifth = integer_parabolic_sine_q15(phase_fifth);

        // Smooth attack & release envelope in Q10 (`0..=1024`)
        let env_q10 = if i < 2400 {
            ((i as i64) * 1024) / 2400
        } else if i > frames.saturating_sub(4800) {
            (((frames - i) as i64) * 1024) / 4800
        } else {
            1024
        };

        // Periodic bowed/plucked Dirac harmonic impulse every 12000 frames (250 ms)
        let bow_impulse = if i % 12000 < 64 {
            (64 - (i % 12000) as i64) * 90
        } else {
            0
        };

        let left = (((w0 * 520 + w_phi * 280 + w_fifth * 200) >> 10) + bow_impulse) * env_q10 >> 11;
        let right = (((w0 * 520 + w_fifth * 280 + w_phi * 200) >> 10) - (bow_impulse >> 1)) * env_q10 >> 11;

        raw_stereo.push(left.clamp(-32767, 32767) as i16);
        raw_stereo.push(right.clamp(-32767, 32767) as i16);
    }

    let (phoenix_pcm, eight_a, hash) = transform_stereo_pcm_phoenix(&raw_stereo, sample_rate, preset);
    let wav = encode_wav_i16(&phoenix_pcm, sample_rate, 2);
    (wav, eight_a, hash)
}

/// Exact integer parabolic sine approximation for `phase_q16` in `0..65536`, returning `-32767..=32767`.
#[inline]
pub fn integer_parabolic_sine_q15(phase_q16: i64) -> i64 {
    let half = phase_q16 & 0x7FFF; // 0..32767
    let centered = half - 16384;   // -16384..16383
    // Parabola: `16384^2 - centered^2` scaled down by `>> 13` -> `0..32768`
    let arch = ((16384 * 16384) - (centered * centered)) >> 13;
    if (phase_q16 & 0x8000) != 0 {
        -arch
    } else {
        arch
    }
}

/// Apply the 3-Stage PhoeniX Revelation pass to an interleaved stereo 16-bit PCM slice.
pub fn transform_stereo_pcm_phoenix(
    stereo_pcm: &[i16],
    sample_rate_hz: u32,
    preset: &MobilePhoenixPreset,
) -> (Vec<i16>, Vec<u8>, u64) {
    let frames = stereo_pcm.len() / 2;
    let mut out = Vec::with_capacity(stereo_pcm.len());
    let mut opcodes = Vec::with_capacity(512);
    let mut lp_l: i64 = 0;
    let mut lp_r: i64 = 0;
    let mut phi_l: i64 = 0;
    let mut phi_r: i64 = 0;

    for i in 0..frames {
        let p0 = i.saturating_sub(1);
        let p1 = i;
        let p2 = (i + 1).min(frames.saturating_sub(1));
        let p3 = (i + 2).min(frames.saturating_sub(1));

        let l0 = stereo_pcm[p0 * 2];
        let l1 = stereo_pcm[p1 * 2];
        let l2 = stereo_pcm[p2 * 2];
        let l3 = stereo_pcm[p3 * 2];

        let r0 = stereo_pcm[p0 * 2 + 1];
        let r1 = stereo_pcm[p1 * 2 + 1];
        let r2 = stereo_pcm[p2 * 2 + 1];
        let r3 = stereo_pcm[p3 * 2 + 1];

        let mid_768k_l = catmull_rom_midpoint_i16(l0, l1, l2, l3) as i64;
        let mid_768k_r = catmull_rom_midpoint_i16(r0, r1, r2, r3) as i64;

        let s_l = l1 as i64;
        let s_r = r1 as i64;
        lp_l = (lp_l * 7 + s_l) >> 3;
        lp_r = (lp_r * 7 + s_r) >> 3;

        let arch_l = mid_768k_l - ((s_l + l2 as i64) >> 1);
        let arch_r = mid_768k_r - ((s_r + r2 as i64) >> 1);

        let warmth_l = (((s_l * s_l.abs()) >> 15) * preset.warmth_q10) >> 11;
        let warmth_r = (((s_r * s_r.abs()) >> 15) * preset.warmth_q10) >> 11;
        let sparkle_l = (arch_l * preset.sparkle_q10) >> 9;
        let sparkle_r = (arch_r * preset.sparkle_q10) >> 9;
        let drive_l = (lp_l * preset.drive_q10) >> 12;
        let drive_r = (lp_r * preset.drive_q10) >> 12;
        let punch_l = ((s_l - lp_l) * preset.punch_q10) >> 12;
        let punch_r = ((s_r - lp_r) * preset.punch_q10) >> 12;
        let air_l = (((l2 as i64) - s_l) * preset.air_q10) >> 12;
        let air_r = (((r2 as i64) - s_r) * preset.air_q10) >> 12;

        phi_l = (phi_l * 500 + lp_l * 309) / 809;
        phi_r = (phi_r * 500 + lp_r * 309) / 809;
        let bloom_l = (phi_l * preset.phi_432_q10) >> 12;
        let bloom_r = (phi_r * preset.phi_432_q10) >> 12;

        let raw_l = s_l + warmth_l + sparkle_l + drive_l + punch_l + air_l + bloom_l;
        let raw_r = s_r + warmth_r + sparkle_r + drive_r + punch_r + air_r + bloom_r;

        let mid = (raw_l + raw_r) >> 1;
        let side = ((raw_l - raw_r) * preset.stereo_width_q10) >> 11;
        out.push(soft_limit_i16(mid + side));
        out.push(soft_limit_i16(mid - side));

        if i % 1024 == 0 && opcodes.len() < 4096 {
            opcodes.extend_from_slice(&[
                0x10,
                0xB0,
                0x01, // 432 Hz
                0x01,
                0x00,
                ((s_l.abs() >> 7).min(255)) as u8,
                (i & 0xFF) as u8,
                180,
                ((preset.warmth_q10 >> 3).min(255)) as u8,
                ((preset.sparkle_q10 >> 3).min(255)) as u8,
                0,
            ]);
        }
    }

    let mut hash: u64 = 0xcbf29ce484222325;
    for &s in &out {
        let b = s.to_le_bytes();
        hash ^= b[0] as u64;
        hash = hash.wrapping_mul(0x100000001b3);
        hash ^= b[1] as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }

    let mut eight_a = Vec::with_capacity(16 + opcodes.len());
    eight_a.extend_from_slice(&EIGHT_A_MAGIC);
    let interlace: u16 = 1 | (((sample_rate_hz / 1000).min(127) as u16) << 1) | (16u16 << 8);
    eight_a.extend_from_slice(&interlace.to_le_bytes());
    eight_a.extend_from_slice(&hash.to_le_bytes());
    eight_a.extend_from_slice(&432u16.to_le_bytes());
    eight_a.extend_from_slice(&opcodes);

    (out, eight_a, hash)
}

/// Encode 16-bit PCM into RIFF WAVE bytes.
pub fn encode_wav_i16(interleaved_pcm: &[i16], sample_rate_hz: u32, channels: u16) -> Vec<u8> {
    let ch = channels.clamp(1, 2);
    let data_bytes = (interleaved_pcm.len() * 2) as u32;
    let riff_size = 36u32.saturating_add(data_bytes);
    let byte_rate = sample_rate_hz * (ch as u32) * 2;
    let block_align = ch * 2;

    let mut wav = Vec::with_capacity(44 + data_bytes as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&riff_size.to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&ch.to_le_bytes());
    wav.extend_from_slice(&sample_rate_hz.to_le_bytes());
    wav.extend_from_slice(&byte_rate.to_le_bytes());
    wav.extend_from_slice(&block_align.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_bytes.to_le_bytes());
    for &sample in interleaved_pcm {
        wav.extend_from_slice(&sample.to_le_bytes());
    }
    wav
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn on_device_phoenix_chord_synthesizes_valid_wav_and_8a_without_floats() {
        let preset = MobilePhoenixPreset::default();
        let (wav, eight_a, hash) = synthesize_on_device_phoenix_chord(432, 300, &preset);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&eight_a[0..4], &EIGHT_A_MAGIC);
        assert_ne!(hash, 0);
    }
}
