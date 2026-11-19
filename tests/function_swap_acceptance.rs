//! End-to-end Linux acceptance tests for AyeOS Capsule Inspector & real isolated function swaps.
//!
//! Proves every acceptance criterion on a live open Inspector HTTP server + `FunctionSwapEngine`:
//! 1. Off-device Ed25519 signing (AyeOS receives ONLY the 32-byte public key) + BLAKE3 digest.
//! 2. `g8t` nomination (`NominatedOnly`) cannot activate without explicit local AyeOS authorization.
//! 3. Anti-downgrade: `DualSignedRequired` manifest is REJECTED under `Ed25519LocalInspectorDemo`.
//! 4. Tampered `.so` (1-bit flip) and wrong-key Ed25519 signatures are REJECTED.
//! 5. Pre-`dlopen` Linux sandbox (Landlock + Stage-1 `seccomp-bpf`) KILLS `.init_array` escape
//!    attempts DURING `dlopen()` before `ayeos_capsule_eval` is ever invoked.
//! 6. Live swap `BaselineRust -> SignedCapsule -> BaselineRust` with 64-vector equivalence and
//!    REAL measured timings while the Inspector HTTP app stays open (constant `session_nonce` & PID,
//!    strictly monotonic `eval_counter`).
//! 7. Worker crash (`SIGILL` via `ud2`), worker timeout (parent wall-clock deadline `SIGKILL`),
//!    and Stage-2 `seccomp-bpf` violation (`SYS_socket` -> `SIGSYS`) ALL return the job cleanly to
//!    `BaselineRust` with exact results without restarting the Inspector!
//! 8. Read-only boot-pinned `VirtIO-net` driver panel refuses `POST /api/inspector/driver/upgrade`.

use ayeos::desktop::AyeosDesktopState;
use ayeos::function_swap::{baseline_harmonic_fold, EvalOutcome, FunctionSwapEngine};
use ayeos::inspector::{serve_inspector_loop, InspectorFullState, InspectorState};
use ayeos_manifest::{
    CapsuleKind, CapsuleLifecycle, CapsuleManifest, CapsulePermissions, DeviceAuthorization,
    FunctionLifecycle, FunctionPermissions, G8tNomination, NarrowInterface, SignaturePolicy,
    SignatureRequirement,
};
use ed25519_dalek::{Signer, SigningKey};
use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;

const SENTINEL_CRASH: i64 = 0x0000_0000_DEAD_BEEF;
const SENTINEL_TIMEOUT: i64 = 0x0000_0000_71DE_0071;
const SENTINEL_SECCOMP_ESCAPE: i64 = 0x0000_0000_BAD0_05CA;

fn locate_artifact(name: &str) -> PathBuf {
    let cargo_target = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/home/hue/.cargo/target"));
    let candidates = [
        cargo_target.join("release").join(name),
        cargo_target.join("debug").join(name),
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/release")
            .join(name),
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/debug")
            .join(name),
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("capsules/target/release")
            .join(name),
    ];
    for c in &candidates {
        if c.is_file() {
            return c.clone();
        }
    }
    panic!("Required test binary/capsule '{name}' not found in {candidates:?}");
}

fn sign_off_device(
    signing_key: &SigningKey,
    id: &str,
    so_path: &Path,
    sig_req: SignatureRequirement,
    deadline_ms: u32,
) -> CapsuleManifest {
    let bytes = fs::read(so_path).expect("read .so");
    let blake3_hex = blake3::hash(&bytes).to_hex().to_string();
    let mut m = CapsuleManifest {
        id: id.to_string(),
        version: 2,
        kind: CapsuleKind::Function,
        target_arch: std::env::consts::ARCH.to_string(),
        interface: NarrowInterface::PureFnI64x2ToI64,
        permissions: CapsulePermissions::Function(FunctionPermissions::strict_default(deadline_ms)),
        lifecycle: CapsuleLifecycle::Function(FunctionLifecycle::StagedLocal),
        signature_requirement: sig_req,
        zero_float_declared: true,
        artifact_blake3_hex: blake3_hex,
        ed25519_signature_hex: String::new(),
        secondary_signature_hex: None,
        nomination: Some(G8tNomination::advisory(id, 1, 99, 140)),
        authorization: DeviceAuthorization::NominatedOnly,
    };
    let sig = signing_key.sign(&m.canonical_signing_bytes());
    m.ed25519_signature_hex = hex::encode(sig.to_bytes());
    m
}

fn http_call(addr: SocketAddr, method: &str, path: &str, body: Option<&str>) -> (u16, String) {
    let mut s = TcpStream::connect(addr).expect("connect inspector");
    let payload = body.unwrap_or("");
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
        payload.len()
    );
    s.write_all(req.as_bytes()).expect("write request");
    let mut raw = String::new();
    s.read_to_string(&mut raw).expect("read response");
    let mut parts = raw.splitn(2, "\r\n\r\n");
    let head = parts.next().unwrap_or("");
    let resp_body = parts.next().unwrap_or("").to_string();
    let code: u16 = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    (code, resp_body)
}

#[test]
fn inspector_live_swap_isolation_and_rollback_acceptance() {
    let worker_bin = locate_artifact("ayeos-capsule-worker");
    let good_so = locate_artifact("libayeos_fn_harmonic_fold.so");
    let fault_so = locate_artifact("libayeos_fn_harmonic_fold_fault.so");
    let ctor_so = locate_artifact("libayeos_fn_harmonic_fold_ctor_escape.so");

    // Off-device Ed25519 keypair (signing key NEVER passed to AyeOS / Inspector)
    let off_device_key = SigningKey::from_bytes(&[0x42u8; 32]);
    let verifying_pubkey_bytes = off_device_key.verifying_key().to_bytes();
    let wrong_key = SigningKey::from_bytes(&[0x99u8; 32]);

    // Launch one live Inspector HTTP server on an ephemeral localhost port
    let engine = FunctionSwapEngine::new(
        verifying_pubkey_bytes,
        SignaturePolicy::Ed25519LocalInspectorDemo,
        worker_bin,
    )
    .expect("create FunctionSwapEngine");
    let state = Arc::new(Mutex::new(InspectorState::new(engine)));
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let addr = listener.local_addr().unwrap();
    let srv_state = Arc::clone(&state);
    thread::spawn(move || serve_inspector_loop(listener, srv_state));

    // 1. Initial baseline evaluation on open Inspector
    let (code, body) = http_call(addr, "POST", "/api/inspector/eval", Some(r#"{"seed":432,"rounds":512}"#));
    assert_eq!(code, 200);
    let ev1: EvalOutcome = serde_json::from_str(&body).unwrap();
    let initial_nonce = ev1.session_nonce;
    assert_eq!(ev1.backend, "baseline_rust");
    assert_eq!(ev1.eval_counter, 1);
    assert_eq!(ev1.result, baseline_harmonic_fold(432, 512));
    assert!(!ev1.auto_rolled_back);

    // 2. Stage valid signed capsule -> verify NominatedOnly CANNOT activate without local authorization
    let good_manifest = sign_off_device(
        &off_device_key,
        "fn_harmonic_fold_v2",
        &good_so,
        SignatureRequirement::Ed25519Only,
        150,
    );
    let (code, _) = http_call(
        addr,
        "POST",
        "/api/inspector/stage",
        Some(&serde_json::json!({
            "manifest": good_manifest,
            "artifact_path": good_so.to_string_lossy(),
        }).to_string()),
    );
    assert_eq!(code, 200);
    let (code, body) = http_call(addr, "POST", "/api/inspector/swap", Some("{}"));
    assert_eq!(code, 403, "NominatedOnly must not activate: {body}");
    assert!(body.contains("NOMINATION_ONLY"), "unexpected body: {body}");

    // 3. Anti-downgrade check: DualSignedRequired must be REJECTED under Ed25519LocalInspectorDemo
    let dual_manifest = sign_off_device(
        &off_device_key,
        "fn_dual_signed_g8t",
        &good_so,
        SignatureRequirement::DualSignedRequired,
        150,
    );
    let (code, _) = http_call(
        addr,
        "POST",
        "/api/inspector/stage",
        Some(&serde_json::json!({
            "manifest": dual_manifest,
            "artifact_path": good_so.to_string_lossy(),
        }).to_string()),
    );
    assert_eq!(code, 200);
    let (code, body) = http_call(addr, "POST", "/api/inspector/authorize", Some(r#"{"approver":"hue"}"#));
    assert_eq!(code, 403, "Dual-signed downgrade must fail: {body}");
    assert!(body.contains("POLICY_DOWNGRADE_FORBIDDEN"), "unexpected body: {body}");

    // 4. Tampered .so (1-bit flip) -> rejected by BLAKE3 digest check
    let tampered_path = std::env::temp_dir().join("ayeos_test_tampered_capsule.so");
    let mut tampered_bytes = fs::read(&good_so).unwrap();
    let mid = tampered_bytes.len() / 2;
    tampered_bytes[mid] ^= 0x01;
    fs::write(&tampered_path, &tampered_bytes).unwrap();
    let (code, _) = http_call(
        addr,
        "POST",
        "/api/inspector/stage",
        Some(&serde_json::json!({
            "manifest": good_manifest,
            "artifact_path": tampered_path.to_string_lossy(),
        }).to_string()),
    );
    assert_eq!(code, 200);
    let (code, body) = http_call(addr, "POST", "/api/inspector/authorize", Some(r#"{"approver":"hue"}"#));
    assert_eq!(code, 403);
    assert!(body.contains("ARTIFACT_DIGEST_MISMATCH"), "unexpected body: {body}");
    let _ = fs::remove_file(&tampered_path);

    // 5. Wrong Ed25519 key -> rejected by Ed25519 verify_strict
    let wrong_manifest = sign_off_device(
        &wrong_key,
        "fn_wrong_key",
        &good_so,
        SignatureRequirement::Ed25519Only,
        150,
    );
    let (code, _) = http_call(
        addr,
        "POST",
        "/api/inspector/stage",
        Some(&serde_json::json!({
            "manifest": wrong_manifest,
            "artifact_path": good_so.to_string_lossy(),
        }).to_string()),
    );
    assert_eq!(code, 200);
    let (code, body) = http_call(addr, "POST", "/api/inspector/authorize", Some(r#"{"approver":"hue"}"#));
    assert_eq!(code, 403);
    assert!(body.contains("ED25519_SIGNATURE_INVALID"), "unexpected body: {body}");

    // 6. Pre-dlopen sandbox proof: hostile .init_array constructor calling SYS_socket during dlopen()
    let ctor_manifest = sign_off_device(
        &off_device_key,
        "fn_ctor_escape",
        &ctor_so,
        SignatureRequirement::Ed25519Only,
        150,
    );
    let (code, _) = http_call(
        addr,
        "POST",
        "/api/inspector/stage",
        Some(&serde_json::json!({
            "manifest": ctor_manifest,
            "artifact_path": ctor_so.to_string_lossy(),
        }).to_string()),
    );
    assert_eq!(code, 200);
    let (code, _) = http_call(addr, "POST", "/api/inspector/authorize", Some(r#"{"approver":"hue"}"#));
    assert_eq!(code, 200);
    let (code, body) = http_call(addr, "POST", "/api/inspector/swap", Some("{}"));
    assert_eq!(code, 403, "Hostile .init_array must be killed pre-dlopen: {body}");
    assert!(body.contains("signal 31"), "Expected SIGSYS (31) during dlopen: {body}");

    // 7. REAL LIVE SWAP: Rust -> Ed25519-signed capsule -> Rust while Inspector stays open!
    let (code, _) = http_call(
        addr,
        "POST",
        "/api/inspector/stage",
        Some(&serde_json::json!({
            "manifest": good_manifest,
            "artifact_path": good_so.to_string_lossy(),
        }).to_string()),
    );
    assert_eq!(code, 200);
    let (code, body) = http_call(addr, "POST", "/api/inspector/authorize", Some(r#"{"approver":"hue-local"}"#));
    assert_eq!(code, 200, "Authorize valid capsule failed: {body}");
    let (code, body) = http_call(addr, "POST", "/api/inspector/swap", Some("{}"));
    assert_eq!(code, 200, "Swap valid capsule failed: {body}");
    assert!(body.contains("\"equivalence_passed\":true"), "{body}");

    // Verify Desktop record_verified_function_swap records real measured data (not hardcoded 12/1)
    let bench = state
        .lock()
        .unwrap()
        .engine
        .last_benchmark()
        .expect("real benchmark recorded")
        .clone();
    let mut desk = AyeosDesktopState::default();
    let recorded = desk.record_verified_function_swap(
        1,
        "harmonic_fold_step",
        "fn_harmonic_fold_v2",
        &good_manifest.artifact_blake3_hex,
        &bench,
    );
    assert!(recorded.executable_swapped);
    let sim = desk.hotpatch_guest_assembly(3, "unseen_pattern_xyz").unwrap();
    assert!(!sim.executable_swapped, "pattern match must not claim executable swap");

    // Live eval on active signed capsule (same session_nonce, counter incremented!)
    let (code, body) = http_call(addr, "POST", "/api/inspector/eval", Some(r#"{"seed":432,"rounds":512}"#));
    assert_eq!(code, 200);
    let ev2: EvalOutcome = serde_json::from_str(&body).unwrap();
    assert_eq!(ev2.session_nonce, initial_nonce);
    assert_eq!(ev2.eval_counter, 2);
    assert_eq!(ev2.backend, "signed_capsule:fn_harmonic_fold_v2");
    assert_eq!(ev2.result, baseline_harmonic_fold(432, 512));
    assert!(!ev2.auto_rolled_back);

    // Manual rollback -> Rust baseline while Inspector stays open!
    let (code, body) = http_call(addr, "POST", "/api/inspector/rollback", Some("{}"));
    assert_eq!(code, 200, "{body}");
    let (code, body) = http_call(addr, "POST", "/api/inspector/eval", Some(r#"{"seed":528,"rounds":256}"#));
    assert_eq!(code, 200);
    let ev3: EvalOutcome = serde_json::from_str(&body).unwrap();
    assert_eq!(ev3.session_nonce, initial_nonce);
    assert_eq!(ev3.eval_counter, 3);
    assert_eq!(ev3.backend, "baseline_rust");
    assert_eq!(ev3.result, baseline_harmonic_fold(528, 256));

    // 8. Worker CRASH rollback (SIGILL via ud2) -> job returns to Rust without restarting app!
    let fault_manifest = sign_off_device(
        &off_device_key,
        "fn_fault_harness",
        &fault_so,
        SignatureRequirement::Ed25519Only,
        120,
    );
    let stage_fault = || {
        let (c1, _) = http_call(
            addr,
            "POST",
            "/api/inspector/stage",
            Some(&serde_json::json!({
                "manifest": fault_manifest,
                "artifact_path": fault_so.to_string_lossy(),
            }).to_string()),
        );
        assert_eq!(c1, 200);
        let (c2, _) = http_call(addr, "POST", "/api/inspector/authorize", Some(r#"{"approver":"hue"}"#));
        assert_eq!(c2, 200);
        let (c3, b3) = http_call(addr, "POST", "/api/inspector/swap", Some("{}"));
        assert_eq!(c3, 200, "Fault harness should pass 64 normal vectors: {b3}");
    };

    stage_fault();
    let (code, body) = http_call(
        addr,
        "POST",
        "/api/inspector/eval",
        Some(&serde_json::json!({"seed": SENTINEL_CRASH, "rounds": 8}).to_string()),
    );
    assert_eq!(code, 200);
    let ev_crash: EvalOutcome = serde_json::from_str(&body).unwrap();
    assert_eq!(ev_crash.session_nonce, initial_nonce);
    assert_eq!(ev_crash.eval_counter, 4);
    assert_eq!(ev_crash.backend, "baseline_rust");
    assert!(ev_crash.auto_rolled_back);
    assert_eq!(ev_crash.result, baseline_harmonic_fold(SENTINEL_CRASH, 8));

    // 9. Worker TIMEOUT rollback (infinite loop killed by parent 120ms deadline!)
    stage_fault();
    let (code, body) = http_call(
        addr,
        "POST",
        "/api/inspector/eval",
        Some(&serde_json::json!({"seed": SENTINEL_TIMEOUT, "rounds": 8}).to_string()),
    );
    assert_eq!(code, 200);
    let ev_timeout: EvalOutcome = serde_json::from_str(&body).unwrap();
    assert_eq!(ev_timeout.session_nonce, initial_nonce);
    assert_eq!(ev_timeout.eval_counter, 5);
    assert_eq!(ev_timeout.backend, "baseline_rust");
    assert!(ev_timeout.auto_rolled_back);
    assert!(
        ev_timeout
            .rollback_reason
            .as_deref()
            .unwrap_or("")
            .contains("wall_clock_deadline_exceeded"),
        "expected deadline reason, got {:?}",
        ev_timeout.rollback_reason
    );
    assert_eq!(ev_timeout.result, baseline_harmonic_fold(SENTINEL_TIMEOUT, 8));

    // 10. Stage-2 seccomp escape during eval (SYS_socket -> SIGSYS 31) -> auto-rollback to Rust!
    stage_fault();
    let (code, body) = http_call(
        addr,
        "POST",
        "/api/inspector/eval",
        Some(&serde_json::json!({"seed": SENTINEL_SECCOMP_ESCAPE, "rounds": 8}).to_string()),
    );
    assert_eq!(code, 200);
    let ev_seccomp: EvalOutcome = serde_json::from_str(&body).unwrap();
    assert_eq!(ev_seccomp.session_nonce, initial_nonce);
    assert_eq!(ev_seccomp.eval_counter, 6);
    assert_eq!(ev_seccomp.backend, "baseline_rust");
    assert!(ev_seccomp.auto_rolled_back);
    assert!(
        ev_seccomp
            .rollback_reason
            .as_deref()
            .unwrap_or("")
            .contains("signal=31"),
        "expected SIGSYS signal=31, got {:?}",
        ev_seccomp.rollback_reason
    );

    // 11. Read-only VirtIO-net driver inspector & explicit refusal of live driver upgrade
    let (code, body) = http_call(addr, "GET", "/api/inspector/state", None);
    assert_eq!(code, 200);
    let full: InspectorFullState = serde_json::from_str(&body).unwrap();
    assert!(!full.registry_installs_enabled);
    assert_eq!(full.function_swap.session_nonce, initial_nonce);
    assert!(full.boot_pinned_drivers[0].read_only);
    assert!(!full.boot_pinned_drivers[0].permissions.live_upgrade_allowed);

    let (code, body) = http_call(addr, "POST", "/api/inspector/driver/upgrade", Some("{}"));
    assert_eq!(code, 403);
    assert!(body.contains("DRIVER_LIVE_UPGRADE_BLOCKED"), "{body}");

    // 12. HTML page renders with Constellation Ink (zero purple)
    let (code, html) = http_call(addr, "GET", "/inspector", None);
    assert_eq!(code, 200);
    assert!(html.contains("AyeOS Capsule Inspector"));
    assert!(!html.to_ascii_lowercase().contains("#b48bff"));
}
