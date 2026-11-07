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

/// On-device PhoeniX tuning parameters in `Q10` (`1024 == 1/1`),
/// augmented with Goonj Ternary Dark Velvet Noise (`{-1, 0, +1}`), Svara Formant, and Naad SVF Sub.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MobilePhoenixPreset {
    pub sparkle_q10: i64,
    pub warmth_q10: i64,
    pub drive_q10: i64,
    pub punch_q10: i64,
    pub air_q10: i64,
    pub phi_432_q10: i64,
    pub goonj_velvet_q10: i64,
    pub svara_formant_q10: i64,
    pub naad_sub_q10: i64,
    pub stereo_width_q10: i64,
}

impl Default for MobilePhoenixPreset {
    fn default() -> Self {
        Self::velvet_sanctuary()
    }
}

impl MobilePhoenixPreset {
    pub const fn velvet_sanctuary() -> Self {
        Self {
            sparkle_q10: 440,
            warmth_q10: 380,
            drive_q10: 240,
            punch_q10: 500,
            air_q10: 360,
            phi_432_q10: 300,
            goonj_velvet_q10: 180,
            svara_formant_q10: 160,
            naad_sub_q10: 150,
            stereo_width_q10: 1340,
        }
    }

    pub const fn liquid_gold() -> Self {
        Self {
            sparkle_q10: 520,
            warmth_q10: 480,
            drive_q10: 310,
            punch_q10: 540,
            air_q10: 420,
            phi_432_q10: 360,
            goonj_velvet_q10: 210,
            svara_formant_q10: 220,
            naad_sub_q10: 210,
            stereo_width_q10: 1420,
        }
    }

    pub const fn riva_cathedral() -> Self {
        Self {
            sparkle_q10: 610,
            warmth_q10: 350,
            drive_q10: 190,
            punch_q10: 460,
            air_q10: 540,
            phi_432_q10: 420,
            goonj_velvet_q10: 340,
            svara_formant_q10: 190,
            naad_sub_q10: 180,
            stereo_width_q10: 1560,
        }
    }

    pub const fn midnight_silk() -> Self {
        Self {
            sparkle_q10: 310,
            warmth_q10: 520,
            drive_q10: 160,
            punch_q10: 380,
            air_q10: 260,
            phi_432_q10: 380,
            goonj_velvet_q10: 260,
            svara_formant_q10: 140,
            naad_sub_q10: 240,
            stereo_width_q10: 1280,
        }
    }

    pub fn from_name(name: &str) -> Self {
        match name.trim().to_ascii_lowercase().as_str() {
            "liquid_gold" | "gold" => Self::liquid_gold(),
            "riva_cathedral" | "cathedral" | "riva" => Self::riva_cathedral(),
            "midnight_silk" | "silk" | "midnight" => Self::midnight_silk(),
            _ => Self::velvet_sanctuary(),
        }
    }
}

/// Babbage 3-Addition Cubic Forward-Difference Stepper for 8× (`48 kHz -> 384 kHz`)
/// 24-bit (`-8388607..=8388607`) hardware output.
///
/// Evaluates the exact Catmull-Rom cubic polynomial `P(u)` at `u = k / 8` (`k = 0..8`)
/// using 4 setup shifts/adds per input frame and **3 integer additions per 384 kHz sample**
/// (`y += d1; d1 += d2; d2 += d3;`) — zero multiplications and zero divisions inside the
/// inner high-rate loop.
#[inline]
pub fn step_babbage_cubic_8x_i32(p0: i32, p1: i32, p2: i32, p3: i32) -> [i32; 8] {
    let s0 = p0 as i64;
    let s1 = p1 as i64;
    let s2 = p2 as i64;
    let s3 = p3 as i64;

    // 2 * P(t) = 2*p1 + (-p0 + p2)*t + (2*p0 - 5*p1 + 4*p2 - p3)*t^2 + (-p0 + 3*p1 - 3*p2 + p3)*t^3
    // At t = u / 8 (scaled by 2 * 8^3 = 1024):
    let two_a = -s0 + 3 * s1 - 3 * s2 + s3;
    let two_b = 2 * s0 - 5 * s1 + 4 * s2 - s3;
    let two_c = -s0 + s2;

    let mut y = s1 << 10;
    let mut d1 = (two_c << 6) + (two_b << 3) + two_a;
    let mut d2 = (two_b << 4) + 6 * two_a;
    let d3 = 6 * two_a;

    let mut out = [0i32; 8];
    let mut k = 0;
    while k < 8 {
        out[k] = ((y + 512) >> 10).clamp(-8_388_607, 8_388_607) as i32;
        y += d1;
        d1 += d2;
        d2 += d3;
        k += 1;
    }
    out
}

/// Upsample interleaved stereo 16-bit PhoeniX PCM (`48 kHz`) by `factor` (`1`, `2`, `4`, or `8`)
/// into packed 24-bit little-endian (`S24_3LE`, 3 bytes per sample) using the Babbage 3-Addition
/// Cubic Forward-Difference Stepper.
pub fn upsample_stereo_babbage_cubic_s24le(stereo_i16: &[i16], factor: usize) -> Vec<u8> {
    let ratio = match factor {
        8 => 8usize,
        4 => 4usize,
        2 => 2usize,
        _ => 1usize,
    };
    let frames = stereo_i16.len() / 2;
    let mut s24_bytes = Vec::with_capacity(frames * ratio * 6);
    let step_stride = 8 / ratio;

    for i in 0..frames {
        let i0 = i.saturating_sub(1);
        let i1 = i;
        let i2 = (i + 1).min(frames.saturating_sub(1));
        let i3 = (i + 2).min(frames.saturating_sub(1));

        // Promote 16-bit PhoeniX output into 24-bit dynamic range (`<< 8`)
        let l8 = step_babbage_cubic_8x_i32(
            (stereo_i16[i0 * 2] as i32) << 8,
            (stereo_i16[i1 * 2] as i32) << 8,
            (stereo_i16[i2 * 2] as i32) << 8,
            (stereo_i16[i3 * 2] as i32) << 8,
        );
        let r8 = step_babbage_cubic_8x_i32(
            (stereo_i16[i0 * 2 + 1] as i32) << 8,
            (stereo_i16[i1 * 2 + 1] as i32) << 8,
            (stereo_i16[i2 * 2 + 1] as i32) << 8,
            (stereo_i16[i3 * 2 + 1] as i32) << 8,
        );

        let mut k = 0;
        while k < 8 {
            let lb = l8[k].to_le_bytes();
            let rb = r8[k].to_le_bytes();
            s24_bytes.extend_from_slice(&[lb[0], lb[1], lb[2], rb[0], rb[1], rb[2]]);
            k += step_stride;
        }
    }
    s24_bytes
}

/// Encode packed 24-bit (`S24_3LE`) PCM bytes into a RIFF WAVE container (`bits_per_sample = 24`).
pub fn encode_wav_s24le(s24le_pcm: &[u8], sample_rate_hz: u32, channels: u16) -> Vec<u8> {
    let ch = channels.clamp(1, 2);
    let data_bytes = s24le_pcm.len() as u32;
    let riff_size = 36u32.saturating_add(data_bytes);
    let block_align = ch * 3;
    let byte_rate = sample_rate_hz * (block_align as u32);

    let mut wav = Vec::with_capacity(44 + s24le_pcm.len());
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
    wav.extend_from_slice(&24u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_bytes.to_le_bytes());
    wav.extend_from_slice(s24le_pcm);
    wav
}

/// 8-tap mobile Goonj Balanced-Ternary `{-1, +1}` Dark Velvet Noise reflection table.
pub const MOBILE_GOONJ_DVN_TAPS: [(usize, i64, i64); 8] = [
    (173, 1, -1),
    (463, -1, 1),
    (859, 1, 1),
    (1361, -1, -1),
    (1879, 1, -1),
    (2411, -1, 1),
    (3229, 1, 1),
    (3989, -1, 1),
];

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
    let mut svf_low_l: i64 = 0;
    let mut svf_band_l: i64 = 0;
    let mut svf_low_r: i64 = 0;
    let mut svf_band_r: i64 = 0;
    let mut dvn_buf_l = vec![0i16; 4096];
    let mut dvn_buf_r = vec![0i16; 4096];
    let mut dvn_idx: usize = 0;
    let mut dvn_dark_l: i64 = 0;
    let mut dvn_dark_r: i64 = 0;
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

        // Naad Chamberlin SVF for Svara Formant & Sub-Harmonic Body
        let svf_high_l = s_l - svf_low_l - ((svf_band_l * 640) >> 10);
        svf_band_l = (svf_band_l + ((svf_high_l * 180) >> 10)).clamp(-32767, 32767);
        svf_low_l = (svf_low_l + ((svf_band_l * 180) >> 10)).clamp(-32767, 32767);

        let svf_high_r = s_r - svf_low_r - ((svf_band_r * 640) >> 10);
        svf_band_r = (svf_band_r + ((svf_high_r * 180) >> 10)).clamp(-32767, 32767);
        svf_low_r = (svf_low_r + ((svf_band_r * 180) >> 10)).clamp(-32767, 32767);

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

        let svara_l = (svf_band_l * preset.svara_formant_q10) >> 13;
        let svara_r = (svf_band_r * preset.svara_formant_q10) >> 13;
        let naad_l = (svf_low_l * preset.naad_sub_q10) >> 13;
        let naad_r = (svf_low_r * preset.naad_sub_q10) >> 13;

        dvn_buf_l[dvn_idx] = l1;
        dvn_buf_r[dvn_idx] = r1;
        let mut dvn_acc_l: i64 = 0;
        let mut dvn_acc_r: i64 = 0;
        for &(tap_off, sign_l, sign_r) in &MOBILE_GOONJ_DVN_TAPS {
            let r_idx = (dvn_idx + 4096 - tap_off) & 4095;
            let tl = dvn_buf_l[r_idx] as i64;
            let tr = dvn_buf_r[r_idx] as i64;
            dvn_acc_l += if sign_l > 0 { tl } else { -tl };
            dvn_acc_r += if sign_r > 0 { tr } else { -tr };
        }
        dvn_idx = (dvn_idx + 1) & 4095;
        dvn_dark_l = (dvn_dark_l * 3 + (dvn_acc_l >> 2)) >> 2;
        dvn_dark_r = (dvn_dark_r * 3 + (dvn_acc_r >> 2)) >> 2;
        let goonj_l = (dvn_dark_l * preset.goonj_velvet_q10) >> 14;
        let goonj_r = (dvn_dark_r * preset.goonj_velvet_q10) >> 14;

        let raw_l = s_l + warmth_l + sparkle_l + drive_l + punch_l + air_l + bloom_l + svara_l + naad_l + goonj_l;
        let raw_r = s_r + warmth_r + sparkle_r + drive_r + punch_r + air_r + bloom_r + svara_r + naad_r + goonj_r;

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
