use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, RwLock};
use std::thread;

use ayeos::{
    genesis, genesis_node, MemnetAddress, MemnetCapsule, MemnetNode, TernaryMatrix,
    seed_hash_hex, LINOSV_SEED,
};

/// A named ternary matrix loaded from a capsule.
#[derive(Clone)]
struct NamedMatrix {
    name: String,
    matrix: TernaryMatrix,
}

fn main() {
    let hash = seed_hash_hex();
    let short_hash = &hash[..16];

    println!("ayeOS daemon — ternary matrix inference node");
    println!("seed: LINOSV");
    println!("hash: {}...", short_hash);
    println!();

    let genesis_m = genesis(256, 64);
    println!(
        "genesis matrix: {}×{}, {} groups, {:.2}x compression, {:.1}% sparsity",
        genesis_m.dim,
        genesis_m.dim,
        genesis_m.dim * genesis_m.dim / genesis_m.group_size,
        (genesis_m.weights.len() * 4) as f64
            / ((genesis_m.codes.len() + genesis_m.scales.len() * 4) as f64),
        sparsity(&genesis_m),
    );
    println!();

    // Capsule store — genesis at index 0, trained models loaded at runtime.
    let store: Arc<RwLock<Vec<NamedMatrix>>> = Arc::new(RwLock::new(vec![NamedMatrix {
        name: "genesis".into(),
        matrix: genesis_m,
    }]));

    let node = Arc::new(genesis_node("0.0.0.0", 9876));
    println!("MEMNET node: {}:{} ({})", node.host, node.port, node.address.role);
    println!();

    let listener = TcpListener::bind(format!("{}:{}", node.host, node.port))
        .expect("Failed to bind MEMNET port");
    println!("MEMNET listening on :{}", node.port);
    println!("commands: matrix, capsule [name], list, load <path>, stats, seed, help, quit");
    println!();

    let store_clone = Arc::clone(&store);
    let node_clone = Arc::clone(&node);
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let s = Arc::clone(&store_clone);
            let n = Arc::clone(&node_clone);
            thread::spawn(move || handle_memnet(stream, &s, &n));
        }
    });

    let stdin = BufReader::new(std::io::stdin());
    for line in stdin.lines().flatten() {
        let trimmed = line.trim().to_string();
        let parts: Vec<&str> = trimmed.splitn(2, ' ').collect();
        let cmd = parts[0];
        let arg = parts.get(1).copied().unwrap_or("");

        match cmd {
            "matrix" => {
                let store = store.read().unwrap();
                if let Some(nm) = store.first() {
                    print_matrix_stats(&nm.matrix);
                }
            }
            "capsule" => {
                let store = store.read().unwrap();
                let target = if arg.is_empty() { "genesis" } else { arg };
                if let Some(nm) = store.iter().find(|m| m.name == target) {
                    print_capsule(&nm.matrix, &node, &nm.name);
                } else {
                    println!("no capsule named '{target}' — try 'list'");
                }
            }
            "load" => {
                if arg.is_empty() {
                    println!("usage: load <path> [name]");
                } else {
                    let path = arg;
                    let name = parts.get(2).copied().unwrap_or(
                        std::path::Path::new(path)
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("unnamed"),
                    );
                    match std::fs::read_to_string(path) {
                        Ok(json_str) => {
                            match TernaryMatrix::from_capsule_json(&json_str) {
                                Ok(mat) => {
                                    let mut store = store.write().unwrap();
                                    // Replace if name exists, else append
                                    if let Some(existing) = store.iter_mut().find(|m| m.name == name) {
                                        existing.matrix = mat;
                                        existing.name = name.to_string();
                                        println!("replaced capsule '{name}'");
                                    } else {
                                        store.push(NamedMatrix {
                                            name: name.to_string(),
                                            matrix: mat,
                                        });
                                        println!("loaded capsule '{name}'");
                                    }
                                }
                                Err(e) => println!("load failed: {e}"),
                            }
                        }
                        Err(e) => println!("read error: {e}"),
                    }
                }
            }
            "list" => {
                let store = store.read().unwrap();
                println!("loaded capsules ({}):", store.len());
                for (i, nm) in store.iter().enumerate() {
                    let m = &nm.matrix;
                    println!(
                        "  [{i}] {}: {}×{}, {} groups, {:.2}x, {:.1}% sparsity",
                        nm.name,
                        m.dim,
                        m.dim,
                        m.dim * m.dim / m.group_size,
                        (m.weights.len() * 4) as f64
                            / ((m.codes.len() + m.scales.len() * 4) as f64),
                        sparsity(m),
                    );
                }
            }
            "stats" => {
                let store = store.read().unwrap();
                let nm = store.first().unwrap();
                print_stats(&nm.matrix);
            }
            "seed" => println!("{}", LINOSV_SEED),
            "help" => print_help(),
            "quit" | "exit" => break,
            "" => {}
            cmd => println!("unknown: {cmd}"),
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

fn print_capsule(m: &TernaryMatrix, _node: &MemnetNode, name: &str) {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    let capsule = MemnetCapsule {
        capsule_id: format!("{}-{}", name, m.seed_hash),
        address: MemnetAddress {
            geo: None,
            role: "matrix.ternary".into(),
            tags: vec![name.into(), "ternary".into(), "ayeos".into()],
            vector_hash: None,
        },
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
    println!("ayeOS v0.2.0");
    print_matrix_stats(m);
    println!();
    println!("architecture:");
    println!("  CPU (hearth):  kernel8 — Rust x86_64 kernel, cooperative async executor");
    println!("  GPU (brain):   MLX-QUANT — ternary Metal kernels, 12.80x compression");
    println!("  COORD:         vaked — capability-graph language, flake-native");
    println!("  MESH:          MEMNET — contextual routing, intent-based resolution");
    println!("  CAPSULES:      Hot-load trained ternary models via 'load <path>'");
}

fn print_help() {
    println!("ayeOS daemon commands:");
    println!("  matrix          — show genesis matrix stats");
    println!("  capsule [name]  — show MEMNET capsule JSON for named matrix");
    println!("  list            — list all loaded capsules");
    println!("  load <path> [n] — load a trained quantal capsule JSON");
    println!("  stats           — show full system stats");
    println!("  seed            — print the LINOSV seed text");
    println!("  help            — this message");
    println!("  quit            — shutdown");
}

fn handle_memnet(
    mut stream: TcpStream,
    store: &Arc<RwLock<Vec<NamedMatrix>>>,
    node: &MemnetNode,
) {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    let mut buf = [0u8; 1024];
    if let Ok(n) = stream.read(&mut buf) {
        let request = String::from_utf8_lossy(&buf[..n]);
        let trimmed = request.trim();
        let parts: Vec<&str> = trimmed.splitn(2, ' ').collect();
        let cmd = parts[0];
        let arg = parts.get(1).copied().unwrap_or("");

        let response = match (cmd, arg) {
            ("capsule", name) if !name.is_empty() => {
                let s = store.read().unwrap();
                if let Some(nm) = s.iter().find(|m| m.name == name) {
                    let capsule = MemnetCapsule {
                        capsule_id: format!("{}-{}", nm.name, nm.matrix.seed_hash),
                        address: node.address.clone(),
                        payload_type: "ternary_matrix".into(),
                        payload_b64: STANDARD.encode(&nm.matrix.codes),
                        relevance_score: 1.0,
                        timestamp: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs(),
                    };
                    serde_json::to_string(&capsule).unwrap()
                } else {
                    format!(r#"{{"error":"no capsule '{name}'"}}"#)
                }
            }
            ("capsule", _) | ("get matrix", _) => {
                let s = store.read().unwrap();
                if let Some(nm) = s.first() {
                    let capsule = MemnetCapsule {
                        capsule_id: format!("{}-{}", nm.name, nm.matrix.seed_hash),
                        address: node.address.clone(),
                        payload_type: "ternary_matrix".into(),
                        payload_b64: STANDARD.encode(&nm.matrix.codes),
                        relevance_score: 1.0,
                        timestamp: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs(),
                    };
                    serde_json::to_string(&capsule).unwrap()
                } else {
                    r#"{"error":"no matrices loaded"}"#.into()
                }
            }
            ("list", _) => {
                let s = store.read().unwrap();
                let names: Vec<String> = s.iter().map(|nm| nm.name.clone()).collect();
                serde_json::json!({ "capsules": names }).to_string()
            }
            ("ping", _) => "pong".into(),
            ("stats", _) => serde_json::to_string(&node.address).unwrap(),
            _ => format!(r#"{{"error":"unknown: {cmd}"}}"#),
        };
        let _ = stream.write_all(response.as_bytes());
    }
}
