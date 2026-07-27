use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;

use ayeos::{
    genesis, genesis_node, MemnetCapsule, MemnetNode, TernaryMatrix,
    seed_hash_hex, LINOSV_SEED,
};

fn main() {
    let hash = seed_hash_hex();
    let short_hash = &hash[..16];

    println!("ayeOS daemon — ternary matrix inference node");
    println!("seed: LINOSV");
    println!("hash: {}...", short_hash);
    println!();

    let matrix = genesis(256, 64);
    println!(
        "genesis matrix: {}×{}, {} groups, {} fp32 → {} packed + {} scales = {:.2}x",
        matrix.dim,
        matrix.dim,
        matrix.dim * matrix.dim / matrix.group_size,
        matrix.weights.len() * 4,
        matrix.codes.len(),
        matrix.scales.len() * 4,
        (matrix.weights.len() * 4) as f64 / ((matrix.codes.len() + matrix.scales.len() * 4) as f64),
    );

    println!("sparsity: {:.1}%", sparsity(&matrix));
    println!();

    let node = Arc::new(genesis_node("0.0.0.0", 9876));
    println!("MEMNET node: {}:{} ({})", node.host, node.port, node.address.role);
    println!();

    let listener = TcpListener::bind(format!("{}:{}", node.host, node.port))
        .expect("Failed to bind MEMNET port");
    println!("MEMNET listening on :{}", node.port);
    println!("commands: matrix, capsule, stats, seed, help, quit");
    println!();

    let matrix_arc = Arc::new(matrix);
    let node_arc = Arc::clone(&node);

    let matrix_clone = Arc::clone(&matrix_arc);
    let node_clone = Arc::clone(&node_arc);
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let m = Arc::clone(&matrix_clone);
            let n = Arc::clone(&node_clone);
            thread::spawn(move || handle_memnet(stream, &m, &n));
        }
    });

    let stdin = BufReader::new(std::io::stdin());
    for line in stdin.lines().flatten() {
        match line.trim() {
            "matrix" => print_matrix_stats(&matrix_arc),
            "capsule" => print_capsule(&matrix_arc, &node_arc),
            "stats" => print_stats(&matrix_arc),
            "seed" => println!("{}", LINOSV_SEED),
            "help" => print_help(),
            "quit" | "exit" => break,
            "" => {}
            cmd => println!("unknown: {}", cmd),
        }
    }
}

fn sparsity(m: &TernaryMatrix) -> f64 {
    let zeros = m.codes.iter().filter(|&&c| (c & 0x03) == 1).count();
    zeros as f64 / (m.dim * m.dim) as f64 * 100.0
}

fn print_matrix_stats(m: &TernaryMatrix) {
    println!("dim: {}×{}", m.dim, m.dim);
    println!("group_size: {}", m.group_size);
    println!("weights: {} fp32 ({} bytes)", m.weights.len(), m.weights.len() * 4);
    println!("codes:   {} packed bytes ({} uint32 words)", m.codes.len(), m.codes.len() / 4);
    println!("scales:  {} f32 ({} bytes)", m.scales.len(), m.scales.len() * 4);
    println!("ratio:   {:.2}x", (m.weights.len() * 4) as f64 / ((m.codes.len() + m.scales.len() * 4) as f64));
    println!("sparsity: {:.1}%", sparsity(m));
    println!("seed: {}", m.seed_hash);
}

fn print_capsule(m: &TernaryMatrix, node: &MemnetNode) {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    let capsule = MemnetCapsule {
        capsule_id: format!("LINOSV-{}", m.seed_hash),
        address: node.address.clone(),
        payload_type: "ternary_matrix".into(),
        payload_b64: STANDARD.encode(&m.codes),
        relevance_score: 1.0,
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    };
    println!("{}", serde_json::to_string_pretty(&capsule).unwrap());
}

fn print_stats(m: &TernaryMatrix) {
    println!("ayeOS v0.1.0");
    print_matrix_stats(m);
    println!();
    println!("architecture:");
    println!("  CPU (hearth):  kernel8 — Rust x86_64 kernel, cooperative async executor");
    println!("  GPU (brain):   MLX-QUANT — ternary Metal kernels, 12.80x compression");
    println!("  COORD:         vaked — capability-graph language, flake-native");
    println!("  MESH:          MEMNET — contextual routing, intent-based resolution");
}

fn print_help() {
    println!("ayeOS daemon commands:");
    println!("  matrix   — show genesis matrix stats");
    println!("  capsule  — show MEMNET capsule JSON");
    println!("  stats    — show full system stats");
    println!("  seed     — print the LINOSV seed text");
    println!("  help     — this message");
    println!("  quit     — shutdown");
}

fn handle_memnet(mut stream: TcpStream, matrix: &TernaryMatrix, node: &MemnetNode) {
    let mut buf = [0u8; 1024];
    if let Ok(n) = stream.read(&mut buf) {
        let request = String::from_utf8_lossy(&buf[..n]);
        let response = match request.trim() {
            "capsule" | "get matrix" => {
                use base64::{Engine as _, engine::general_purpose::STANDARD};
                let capsule = MemnetCapsule {
                    capsule_id: format!("LINOSV-{}", matrix.seed_hash),
                    address: node.address.clone(),
                    payload_type: "ternary_matrix".into(),
                    payload_b64: STANDARD.encode(&matrix.codes),
                    relevance_score: 1.0,
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                };
                serde_json::to_string(&capsule).unwrap()
            }
            "ping" => "pong".into(),
            "stats" => serde_json::to_string(&node.address).unwrap(),
            _ => format!("unknown command: {}", request.trim()),
        };
        let _ = stream.write_all(response.as_bytes());
    }
}
