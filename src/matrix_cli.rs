use std::env;

use ayeos::{dequantize, genesis, seed_hash_hex};

fn main() {
    let args: Vec<String> = env::args().collect();
    let dim: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(256);
    let group_size: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(64);

    let hash = seed_hash_hex();
    eprintln!("ayeOS matrix — LINOSV seed (SHA-256: {}...)", &hash[..16]);
    eprintln!(
        "generating {}×{} matrix, group_size={}...",
        dim, dim, group_size
    );

    let m = genesis(dim, group_size);

    let recovered = dequantize(&m.codes, &m.scales, m.dim, m.group_size);

    // Output first 8×8 patch of dequantized values for verification
    println!("first 8×8 patch of dequantized ternary weights {{-1, 0, +1}}:");
    for i in 0..8 {
        for j in 0..8 {
            let idx = i * m.dim + j;
            let g = idx / group_size;
            let scale = m.scales[g];
            let val = recovered[idx] / scale; // normalize by scale
            print!(
                "{:>4} ",
                if val > 0.5 {
                    "1"
                } else if val < -0.5 {
                    "-1"
                } else {
                    "0"
                }
            );
        }
        println!();
    }

    println!();
    println!("stats:");
    println!("  dim:          {}×{}", m.dim, m.dim);
    println!("  group_size:   {}", m.group_size);
    println!(
        "  weights:      {} fp32 ({} bytes)",
        m.weights.len(),
        m.weights.len() * 4
    );
    println!("  packed:       {} bytes", m.codes.len());
    println!(
        "  scales:       {} f32 ({} bytes)",
        m.scales.len(),
        m.scales.len() * 4
    );
    println!(
        "  ratio:        {:.2}x",
        (m.weights.len() * 4) as f64 / ((m.codes.len() + m.scales.len() * 4) as f64)
    );
    println!(
        "  sparsity:     {:.1}%",
        m.codes.iter().filter(|&&c| (c & 0x03) == 1).count() as f64 / (dim * dim) as f64 * 100.0
    );
    println!("  seed:         LINOSV");
    println!("  seed_hash:    {}", &hash[..16]);
}
