//! Standalone tiny **AyeOS Capsule Inspector** binary (`ayeos-inspector`).
//!
//! Manual & local only — receives ONLY the 32-byte Ed25519 verifying public key (`--pubkey` or
//! `--pubkey-hex`); private signing keys stay off-device.

use ayeos::function_swap::FunctionSwapEngine;
use ayeos::inspector::{serve_inspector_loop, InspectorState};
use ayeos_manifest::SignaturePolicy;
use std::env;
use std::fs;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process;
use std::sync::{Arc, Mutex};

fn parse_pubkey_hex(s: &str) -> [u8; 32] {
    let raw = hex::decode(s.trim()).expect("valid hex Ed25519 pubkey");
    raw.try_into().expect("Ed25519 pubkey must be 32 bytes")
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut port: u16 = 8438;
    let mut pubkey_hex: Option<String> = None;
    let mut worker_bin: Option<PathBuf> = None;
    let mut policy = SignaturePolicy::Ed25519LocalInspectorDemo;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--port" if i + 1 < args.len() => {
                port = args[i + 1].parse().expect("valid u16 port");
                i += 2;
            }
            "--pubkey" if i + 1 < args.len() => {
                pubkey_hex = Some(fs::read_to_string(&args[i + 1]).expect("read pubkey file"));
                i += 2;
            }
            "--pubkey-hex" if i + 1 < args.len() => {
                pubkey_hex = Some(args[i + 1].clone());
                i += 2;
            }
            "--worker" if i + 1 < args.len() => {
                worker_bin = Some(PathBuf::from(&args[i + 1]));
                i += 2;
            }
            "--policy" if i + 1 < args.len() => {
                policy = match args[i + 1].as_str() {
                    "ed25519-demo" => SignaturePolicy::Ed25519LocalInspectorDemo,
                    "dual-strict" => SignaturePolicy::DualSignedStrict,
                    _ => {
                        eprintln!("Unknown policy (use ed25519-demo or dual-strict)");
                        process::exit(2);
                    }
                };
                i += 2;
            }
            _ => {
                eprintln!(
                    "Usage: ayeos-inspector --pubkey <pub.hex> | --pubkey-hex <64hex> [--port 8438] [--worker <path>] [--policy ed25519-demo|dual-strict]"
                );
                process::exit(2);
            }
        }
    }

    let Some(pub_hex) = pubkey_hex else {
        eprintln!("ERROR: --pubkey or --pubkey-hex is required (AyeOS takes public key only; signing key stays off-device).");
        process::exit(2);
    };
    let pub_bytes = parse_pubkey_hex(&pub_hex);

    let worker = worker_bin.unwrap_or_else(|| {
        env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("ayeos-capsule-worker")))
            .unwrap_or_else(|| PathBuf::from("ayeos-capsule-worker"))
    });

    let engine = FunctionSwapEngine::new(pub_bytes, policy, worker).expect("init FunctionSwapEngine");
    let state = Arc::new(Mutex::new(InspectorState::new(engine)));
    let listener = TcpListener::bind(("127.0.0.1", port)).expect("bind inspector socket");
    println!(
        "AyeOS Capsule Inspector listening on http://127.0.0.1:{port} (policy={policy:?}, manual/local only)"
    );
    serve_inspector_loop(listener, state);
}
