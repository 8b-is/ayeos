use serde::{Deserialize, Serialize};

use crate::prng::{Xoshiro128, seed_hash};

/// The ternary matrix — {n+-1-<△>} in the user's notation.
/// n = weight count, -1/0/+1 = ternary states, △ = the quantization delta.
#[derive(Clone, Serialize, Deserialize)]
pub struct TernaryMatrix {
    pub dim: usize,
    pub group_size: usize,
    pub weights: Vec<f32>,
    pub codes: Vec<u8>,  // packed 2-bit codes
    pub scales: Vec<f32>,
    pub seed_hash: String,
}

/// Quantize weights to ternary {-1, 0, +1} per group.
/// Mirrors mlx/backend/cpu/quantized.cpp + metal/kernels/ternary_quantized.h:
/// scale = mean(|w|) per group, code = round(clamp(w/scale, -1, 1)) + 1.
pub fn quantize(weights: &[f32], group_size: usize) -> (Vec<u8>, Vec<f32>) {
    let n = weights.len();
    let groups = n / group_size;
    let packed_words = (n + 15) / 16;
    let mut codes = vec![0u8; packed_words * 4]; // uint32 words, but stored as u8
    let mut scales = vec![0.0f32; groups];
    let eps: f32 = 1e-7;

    for g in 0..groups {
        let base = g * group_size;
        let mut abs_sum: f32 = 0.0;
        for j in 0..group_size {
            abs_sum += weights[base + j].abs();
        }
        let scale = abs_sum.max(eps) / (group_size as f32);
        scales[g] = scale;

        // Pack 16 codes per uint32 word, LSB-first, 2 bits each
        for j in (0..group_size).step_by(16) {
            let word_idx = (base + j) / 16;
            let mut word: u32 = 0;
            for k in 0..16 {
                if j + k >= group_size { break; }
                let idx = base + j + k;
                let shifted = (weights[idx] / scale).clamp(-1.0, 1.0).round() + 1.0;
                let code = shifted as u32; // {0→-1, 1→0, 2→+1}
                word |= code << (2 * k);
            }
            let bytes = word.to_le_bytes();
            let offset = word_idx * 4;
            if offset + 4 <= codes.len() {
                codes[offset..offset + 4].copy_from_slice(&bytes);
            }
        }
    }

    (codes, scales)
}

/// Dequantize from packed codes back to fp32 weights.
pub fn dequantize(codes: &[u8], scales: &[f32], dim: usize, group_size: usize) -> Vec<f32> {
    let n = dim * dim;
    let mut weights = vec![0.0f32; n];
    for i in 0..n {
        let word_idx = i / 16;
        let bit_offset = 2 * (i % 16);
        let word_bytes = &codes[word_idx * 4..word_idx * 4 + 4];
        let word = u32::from_le_bytes([word_bytes[0], word_bytes[1], word_bytes[2], word_bytes[3]]);
        let code = ((word >> bit_offset) & 0x03) as i32 - 1; // {-1, 0, +1}
        let g = i / group_size;
        weights[i] = code as f32 * scales[g];
    }
    weights
}

/// Generate a ternary matrix from the LINOSV seed — the ayeOS genesis matrix.
/// Deterministic: same seed + same dimensions = same matrix every time.
pub fn genesis(dim: usize, group_size: usize) -> TernaryMatrix {
    let hash = seed_hash();
    let hash_hex = hex::encode(&hash[..8]);
    let mut rng = Xoshiro128::new(&hash);
    let n = dim * dim;

    let mut weights = Vec::with_capacity(n);
    for _ in 0..n {
        weights.push(rng.normal(0.0, 1.0) as f32);
    }

    let (codes, scales) = quantize(&weights, group_size);

    TernaryMatrix {
        dim,
        group_size,
        weights,
        codes,
        scales,
        seed_hash: hash_hex,
    }
}

/// Block-sparse matrix multiplication with ternary-quantized weights.
/// y = x @ W, where W is ternary-quantized.
pub fn ternary_matmul(x: &[f32], codes: &[u8], scales: &[f32], dim: usize, group_size: usize) -> Vec<f32> {
    let n = dim * dim;
    let mut y = vec![0.0f32; n];
    for i in 0..dim {
        for j in 0..dim {
            let mut acc = 0.0f32;
            let word_idx = j / 16;
            let words = &codes[word_idx * 4..];
            if words.len() < 4 { continue; }
            let word = u32::from_le_bytes([words[0], words[1], words[2], words[3]]);

            for k in (0..dim).step_by(16) {
                let g = (j * dim + k) / group_size;
                let scale = scales[g];
                let x_idx = i * dim + k;
                for t in 0..16 {
                    if k + t >= dim { break; }
                    let code = ((word >> (2 * t)) & 0x03) as i32 - 1;
                    acc += x[x_idx + t] * (code as f32 * scale);
                }
            }
            y[i * dim + j] = acc;
        }
    }
    y
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn genesis_is_deterministic() {
        let m1 = genesis(128, 64);
        let m2 = genesis(128, 64);
        assert_eq!(m1.weights, m2.weights);
        assert_eq!(m1.codes, m2.codes);
        assert_eq!(m1.scales, m2.scales);
    }

    #[test]
    fn quantize_dequantize_roundtrip() {
        let m = genesis(128, 64);
        let recovered = dequantize(&m.codes, &m.scales, m.dim, m.group_size);
        // Every recovered value should be {-scale, 0, +scale}
        let gs = m.group_size;
        for i in 0..m.weights.len() {
            let g = i / gs;
            let scale = m.scales[g];
            assert!(
                (recovered[i] + scale).abs() < 1e-5 ||
                recovered[i].abs() < 1e-5 ||
                (recovered[i] - scale).abs() < 1e-5,
                "weight[{}] = {} not in {{{}, 0, {}}}", i, recovered[i], -scale, scale
            );
        }
    }
}
