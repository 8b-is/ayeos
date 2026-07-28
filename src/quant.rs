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

impl TernaryMatrix {
    /// Load a capsule from the train_quantal.py JSON format.
    /// Expects a JSON object with a `"matrices"` array, each entry having
    /// `dim`, `in_features`, `group_size`, `codes` (byte array), `scales`.
    /// Returns the first matrix in the capsule (for now).
    pub fn from_capsule_json(json_str: &str) -> Result<Self, String> {
        let v: serde_json::Value =
            serde_json::from_str(json_str).map_err(|e| format!("JSON parse: {e}"))?;
        let matrices = v["matrices"]
            .as_array()
            .ok_or("no matrices array in capsule")?;
        let first = matrices
            .first()
            .ok_or("empty matrices array")?;

        let dim = first["dim"].as_u64().ok_or("missing dim")? as usize;
        let in_features = first["in_features"].as_u64().ok_or("missing in_features")? as usize;
        let group_size = first["group_size"].as_u64().ok_or("missing group_size")? as usize;
        let codes_arr = first["codes"]
            .as_array()
            .ok_or("missing codes array")?;
        let scales_arr = first["scales"]
            .as_array()
            .ok_or("missing scales array")?;

        let codes: Vec<u8> = codes_arr
            .iter()
            .map(|v| v.as_u64().unwrap_or(0) as u8)
            .collect();
        let scales: Vec<f32> = scales_arr
            .iter()
            .map(|v| v.as_f64().unwrap_or(0.0) as f32)
            .collect();

        // Reconstruct full weights via dequantize (for reference)
        let weights = dequantize(&codes, &scales, dim.max(in_features), group_size);

        Ok(TernaryMatrix {
            dim: dim.max(in_features),
            group_size,
            weights,
            codes,
            scales,
            seed_hash: "quantal-trained".into(),
        })
    }
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
/// y = x @ W, where W is ternary-quantized, x is dim×dim.
/// Each 4-byte uint32 word packs 16 ternary codes (2 bits each).
pub fn ternary_matmul(
    x: &[f32],
    codes: &[u8],
    scales: &[f32],
    dim: usize,
    group_size: usize,
) -> Vec<f32> {
    let n = dim * dim;
    let mut y = vec![0.0f32; n];
    let words_per_row = dim.div_ceil(16);

    for i in 0..dim {
        let x_row_base = i * dim;
        for j in 0..dim {
            let mut acc = 0.0f32;
            // Iterate over 16-element word blocks in row j of W
            for w in 0..words_per_row {
                let k = w * 16;
                let word_offset = ((j * dim) / 16 + w) * 4;
                if word_offset + 4 > codes.len() {
                    break;
                }
                let word_bytes = &codes[word_offset..word_offset + 4];
                let word = u32::from_le_bytes([
                    word_bytes[0],
                    word_bytes[1],
                    word_bytes[2],
                    word_bytes[3],
                ]);

                let flat_idx = j * dim + k;
                let g = flat_idx / group_size;
                let scale = scales[g];

                let limit = 16.min(dim - k);
                for t in 0..limit {
                    let code = ((word >> (2 * t)) & 0x03) as i32 - 1;
                    acc += x[x_row_base + k + t] * (code as f32 * scale);
                }
            }
            y[i * dim + j] = acc;
        }
    }
    y
}

/// Matrix-vector product: y = x @ W (row vector × ternary matrix).
/// x is a vector of length dim, W is dim×dim ternary-quantized.
/// Returns a vector of length dim.
pub fn ternary_matvec(
    x: &[f32],
    codes: &[u8],
    scales: &[f32],
    dim: usize,
    group_size: usize,
) -> Vec<f32> {
    let mut y = vec![0.0f32; dim];
    let words_per_row = dim.div_ceil(16);

    for j in 0..dim {
        let mut acc = 0.0f32;
        for w in 0..words_per_row {
            let k = w * 16;
            let word_offset = ((j * dim) / 16 + w) * 4;
            if word_offset + 4 > codes.len() {
                break;
            }
            let word_bytes = &codes[word_offset..word_offset + 4];
            let word = u32::from_le_bytes([
                word_bytes[0],
                word_bytes[1],
                word_bytes[2],
                word_bytes[3],
            ]);

            let flat_idx = j * dim + k;
            let g = flat_idx / group_size;
            let scale = scales[g];

            let limit = 16.min(dim - k);
            for t in 0..limit {
                let code = ((word >> (2 * t)) & 0x03) as i32 - 1;
                acc += x[k + t] * (code as f32 * scale);
            }
        }
        y[j] = acc;
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
    fn ternary_matmul_small_is_deterministic() {
        let m = genesis(16, 4);
        let x = vec![1.0f32; 16 * 16];
        let y1 = ternary_matmul(&x, &m.codes, &m.scales, m.dim, m.group_size);
        let y2 = ternary_matmul(&x, &m.codes, &m.scales, m.dim, m.group_size);
        assert_eq!(y1, y2, "matmul should be deterministic");
        assert_eq!(y1.len(), 16 * 16, "output should be dim×dim");
    }

    #[test]
    fn ternary_matmul_different_inputs_produce_different_results() {
        let m = genesis(16, 4);
        let x1 = vec![1.0f32; 16 * 16];
        let x2 = vec![0.5f32; 16 * 16];
        let y1 = ternary_matmul(&x1, &m.codes, &m.scales, m.dim, m.group_size);
        let y2 = ternary_matmul(&x2, &m.codes, &m.scales, m.dim, m.group_size);
        assert_ne!(y1, y2, "different inputs should produce different outputs");
    }

    #[test]
    fn ternary_matmul_larger_dim() {
        // Test with same dim as genesis default (256)
        let m = genesis(64, 32);
        let x = vec![0.5f32; 64 * 64];
        let y = ternary_matmul(&x, &m.codes, &m.scales, m.dim, m.group_size);
        assert_eq!(y.len(), 64 * 64);
        // Should have non-zero values (ternary × fp32 → non-zero)
        let max_val = y.iter().cloned().fold(0.0f32, f32::max);
        assert!(max_val > 0.0, "output should have non-zero values");
    }

    #[test]
    fn from_capsule_json_loads_codes_and_scales() {
        // 16×16 matrix: 256 elements / 16 codes-per-word × 4 bytes = 64 code bytes
        // group_size=64: 256 / 64 = 4 scale groups
        let mut codes = vec![0u8; 64];
        let json_str = format!(r#"{{
            "matrices": [{{
                "name": "test",
                "dim": 16,
                "in_features": 16,
                "group_size": 64,
                "codes": {codes:?},
                "scales": [0.5, 0.3, 0.7, 0.2]
            }}]
        }}"#);
        let m = TernaryMatrix::from_capsule_json(&json_str).unwrap();
        assert_eq!(m.dim, 16);
        assert_eq!(m.group_size, 64);
        assert_eq!(m.codes.len(), 64);
        assert_eq!(m.scales.len(), 4);
        assert!((m.scales[0] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn from_capsule_json_rejects_bad_json() {
        let r = TernaryMatrix::from_capsule_json("not json");
        assert!(r.is_err());
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

    #[test]
    fn ternary_matvec_matches_matmul_row() {
        let m = genesis(32, 16);
        let x = vec![0.7f32; 32];
        let y_vec = ternary_matvec(&x, &m.codes, &m.scales, m.dim, m.group_size);
        assert_eq!(y_vec.len(), 32);
        let x_mat = x.repeat(32);
        let y_mat = ternary_matmul(&x_mat, &m.codes, &m.scales, m.dim, m.group_size);
        for j in 0..32 {
            assert_eq!(
                y_vec[j], y_mat[j],
                "matvec[{j}] should equal matmul[0][{j}] when x is tiled"
            );
        }
    }
}
