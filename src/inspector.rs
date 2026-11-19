//! Tiny, manual, local **AyeOS Capsule Inspector** (`ayeos::inspector`).
//!
//! Serves a compact HTTP + JSON + Constellation Ink HTML interface (zero purple, zero floats)
//! that stays open continuously across `BaselineRust -> SignedCapsule -> BaselineRust` swaps
//! and worker crash/timeout rollbacks.
//!
//! - **Manual & local only**: no registry-driven remote installs.
//! - **Drivers tab is strictly READ-ONLY**: shows boot-pinned `VirtIO-net` with live driver
//!   upgrades and bare-metal dynamic activation explicitly gated.

#![deny(clippy::float_arithmetic)]

use crate::function_swap::{FunctionSwapEngine, SwapEngineSnapshot};
use ayeos_manifest::{
    AppLifecycle, AppPermissions, BareMetalActivationGate, CapsuleKind, CapsuleLifecycle,
    CapsuleManifest, CapsulePermissions, DeviceAuthorization, DriverLifecycle, DriverLiveUpgradeGate,
    DriverPermissions, NarrowInterface, SignatureRequirement,
};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReadOnlyDriverRecord {
    pub id: String,
    pub pci_vid_did: String,
    pub bdf: String,
    pub mac: String,
    pub link: String,
    pub lifecycle: DriverLifecycle,
    pub permissions: DriverPermissions,
    pub g8t_core_note: String,
    pub read_only: bool,
    pub upgrade_gate: DriverLiveUpgradeGate,
    pub bare_metal_gate: BareMetalActivationGate,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InspectorAppRecord {
    pub id: String,
    pub interface: NarrowInterface,
    pub permissions: AppPermissions,
    pub lifecycle: AppLifecycle,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InspectorFullState {
    pub mode: String,
    pub registry_installs_enabled: bool,
    pub function_swap: SwapEngineSnapshot,
    pub apps: Vec<InspectorAppRecord>,
    pub boot_pinned_drivers: Vec<ReadOnlyDriverRecord>,
}

pub struct InspectorState {
    pub engine: FunctionSwapEngine,
    pub apps: Vec<InspectorAppRecord>,
    pub boot_pinned_drivers: Vec<ReadOnlyDriverRecord>,
}

impl InspectorState {
    pub fn new(engine: FunctionSwapEngine) -> Self {
        let apps = vec![InspectorAppRecord {
            id: "ayeos-inspector".to_string(),
            interface: NarrowInterface::InspectorSurfaceV1,
            permissions: AppPermissions::inspector_local(),
            lifecycle: AppLifecycle::Running,
        }];
        let boot_pinned_drivers = vec![ReadOnlyDriverRecord {
            id: "drv_virtio_net_boot_pinned".to_string(),
            pci_vid_did: "1af4:1000".to_string(),
            bdf: "00:01.0 (QEMU Q35)".to_string(),
            mac: "52:54:00:8b:15:42".to_string(),
            link: "boot-pinned (read-only)".to_string(),
            lifecycle: DriverLifecycle::BootPinnedReadOnly,
            permissions: DriverPermissions::boot_pinned_virtio_net(),
            g8t_core_note: "g8t driver registry holds deterministic_core_spec only — NOT a loadable device driver".to_string(),
            read_only: true,
            upgrade_gate: DriverLiveUpgradeGate::current(),
            bare_metal_gate: BareMetalActivationGate::current(),
        }];
        Self {
            engine,
            apps,
            boot_pinned_drivers,
        }
    }

    pub fn full_state(&self) -> InspectorFullState {
        InspectorFullState {
            mode: "MANUAL_LOCAL_LINUX_SANDBOX".to_string(),
            registry_installs_enabled: false,
            function_swap: self.engine.snapshot(),
            apps: self.apps.clone(),
            boot_pinned_drivers: self.boot_pinned_drivers.clone(),
        }
    }
}

#[derive(Deserialize)]
struct EvalReq {
    seed: i64,
    rounds: i64,
}

#[derive(Deserialize)]
struct StageReq {
    manifest: CapsuleManifest,
    artifact_path: String,
}

#[derive(Deserialize)]
struct AuthorizeReq {
    approver: String,
}

pub fn render_inspector_html(st: &InspectorFullState) -> String {
    let f = &st.function_swap;
    let bench_line = if let Some(ref b) = f.last_benchmark {
        format!(
            "64/64 vectors OK · baseline={} ns · capsule_inner={} ns · worker_ipc={} ns · inner_ratio={}/{}",
            b.baseline_total_ns,
            b.capsule_inner_total_ns,
            b.capsule_worker_total_ns,
            b.inner_speedup_num,
            b.inner_speedup_den
        )
    } else {
        "No signed capsule benchmarked yet (running BaselineRust)".to_string()
    };

    let drv = &st.boot_pinned_drivers[0];
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>AyeOS Capsule Inspector</title>
<style>
  body {{ margin:0; font-family: ui-monospace, monospace; background:#05060a; color:#e8eef7; padding:18px; }}
  .hdr {{ background:#0a0e1a; border:1px solid #1b2838; border-top:3px solid #62e6c9; padding:14px; margin-bottom:14px; }}
  .title {{ color:#62e6c9; font-size:16px; font-weight:bold; }}
  .grid {{ display:grid; grid-template-columns: 1fr 1fr; gap:14px; }}
  .card {{ background:#0b101e; border:1px solid #1e2d3d; padding:14px; }}
  .card h3 {{ margin:0 0 10px 0; color:#e8b05c; font-size:13px; text-transform:uppercase; }}
  .badge {{ display:inline-block; padding:2px 8px; border:1px solid #00f0ff; color:#00f0ff; font-size:11px; margin-right:6px; }}
  .badge-ok {{ border-color:#2ecc71; color:#2ecc71; }}
  .badge-lock {{ border-color:#e8b05c; color:#e8b05c; }}
  button {{ background:#0a0e1a; color:#62e6c9; border:1px solid #62e6c9; padding:6px 10px; cursor:pointer; font-family:inherit; margin:4px 4px 0 0; }}
  button:disabled {{ opacity:0.4; cursor:not-allowed; border-color:#556; color:#778; }}
  pre {{ background:#05060a; border:1px solid #16202e; padding:10px; overflow:auto; max-height:220px; font-size:11px; color:#b8d4e8; }}
</style>
</head>
<body>
<div class="hdr">
  <div class="title">AyeOS Capsule Inspector · Manual &amp; Local (Zero Registry Installs)</div>
  <div style="margin-top:6px;">
    <span class="badge">PID: {pid}</span>
    <span class="badge">SESSION: {nonce}</span>
    <span class="badge badge-ok" id="evals">EVALS: {evals}</span>
    <span class="badge" id="backend">BACKEND: {backend}</span>
    <span class="badge badge-lock">POLICY: {policy:?}</span>
  </div>
</div>
<div class="grid">
  <div class="card">
    <h3>1. Live Function Capsule Swap (Linux Pre-dlopen Sandbox)</h3>
    <div style="font-size:12px;margin-bottom:8px;">
      Verifying PubKey (Ed25519 only, secret off-device): <code>{pubshort}...</code><br>
      Measured Timing: <span id="bench">{bench}</span>
    </div>
    <div>
      <button onclick="doEval()">Eval harmonic_fold_step(432, 1024)</button>
      <button onclick="doPost('/api/inspector/authorize', {{approver:'hue-local'}})">Local Authorize</button>
      <button onclick="doPost('/api/inspector/swap', {{}})">Swap -&gt; Signed Capsule</button>
      <button onclick="doPost('/api/inspector/rollback', {{}})">Rollback -&gt; Rust Baseline</button>
    </div>
    <pre id="out">Ready (Inspector stays open across swaps, crashes, and timeouts).</pre>
  </div>
  <div class="card">
    <h3>2. Boot-Pinned Drivers (READ-ONLY)</h3>
    <div style="font-size:12px;line-height:1.6;">
      <div><strong>{drv_id}</strong> · PCI <code>{pci}</code> · BDF <code>{bdf}</code> · MAC <code>{mac}</code></div>
      <div>Lifecycle: <span class="badge badge-lock">BOOT_PINNED_READ_ONLY</span></div>
      <div>Live Upgrades: <span class="badge badge-lock">LOCKED</span> (io_drain=false, dma_irq_revocation=false, independent_recovery=false)</div>
      <div>Bare-Metal Activation: <span class="badge badge-lock">GATED</span> (enforced MMU/IOMMU isolation required)</div>
      <div style="color:#9eb4c7;margin-top:6px;">{g8t_note}</div>
      <button disabled title="Blocked by DriverLiveUpgradeGate">Live Upgrade (Locked)</button>
    </div>
  </div>
</div>
<script>
async function refresh() {{
  const r = await fetch('/api/inspector/state');
  const s = await r.json();
  document.getElementById('evals').textContent = 'EVALS: ' + s.function_swap.eval_counter;
  document.getElementById('backend').textContent = 'BACKEND: ' + s.function_swap.active_backend;
}}
async function doEval() {{
  const r = await fetch('/api/inspector/eval', {{
    method:'POST', headers:{{'Content-Type':'application/json'}},
    body: JSON.stringify({{seed: 432, rounds: 1024}})
  }});
  document.getElementById('out').textContent = JSON.stringify(await r.json(), null, 2);
  refresh();
}}
async function doPost(url, payload) {{
  const r = await fetch(url, {{
    method:'POST', headers:{{'Content-Type':'application/json'}},
    body: JSON.stringify(payload)
  }});
  document.getElementById('out').textContent = JSON.stringify(await r.json(), null, 2);
  refresh();
}}
</script>
</body>
</html>"#,
        pid = f.pid,
        nonce = f.session_nonce,
        evals = f.eval_counter,
        backend = f.active_backend,
        policy = f.signature_policy,
        pubshort = &f.verifying_pubkey_hex[..16],
        bench = bench_line,
        drv_id = drv.id,
        pci = drv.pci_vid_did,
        bdf = drv.bdf,
        mac = drv.mac,
        g8t_note = drv.g8t_core_note,
    )
}

fn write_http(stream: &mut TcpStream, status: &str, content_type: &str, body: &str) {
    let resp = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(resp.as_bytes());
}

pub fn handle_inspector_connection(mut stream: TcpStream, state: Arc<Mutex<InspectorState>>) {
    let mut raw = vec![0u8; 65_536];
    let n = match stream.read(&mut raw) {
        Ok(0) | Err(_) => return,
        Ok(n) => n,
    };
    let req_str = String::from_utf8_lossy(&raw[..n]);
    let mut parts = req_str.splitn(2, "\r\n\r\n");
    let head = parts.next().unwrap_or("");
    let body = parts.next().unwrap_or("").trim();
    let first_line = head.lines().next().unwrap_or("");
    let mut tokens = first_line.split_whitespace();
    let method = tokens.next().unwrap_or("GET");
    let path = tokens.next().unwrap_or("/");

    match (method, path) {
        ("GET", "/") | ("GET", "/inspector") => {
            let snap = state.lock().unwrap().full_state();
            let html = render_inspector_html(&snap);
            write_http(&mut stream, "200 OK", "text/html; charset=utf-8", &html);
        }
        ("GET", "/api/inspector/state") => {
            let snap = state.lock().unwrap().full_state();
            let json = serde_json::to_string(&snap).unwrap_or_else(|_| "{}".to_string());
            write_http(&mut stream, "200 OK", "application/json", &json);
        }
        ("POST", "/api/inspector/eval") => {
            let req: EvalReq = match serde_json::from_str(body) {
                Ok(r) => r,
                Err(e) => {
                    write_http(
                        &mut stream,
                        "400 Bad Request",
                        "application/json",
                        &serde_json::json!({"ok": false, "error": e.to_string()}).to_string(),
                    );
                    return;
                }
            };
            let outcome = state.lock().unwrap().engine.eval(req.seed, req.rounds);
            let json = serde_json::to_string(&outcome).unwrap_or_else(|_| "{}".to_string());
            write_http(&mut stream, "200 OK", "application/json", &json);
        }
        ("POST", "/api/inspector/stage") => {
            let req: StageReq = match serde_json::from_str(body) {
                Ok(r) => r,
                Err(e) => {
                    write_http(
                        &mut stream,
                        "400 Bad Request",
                        "application/json",
                        &serde_json::json!({"ok": false, "error": e.to_string()}).to_string(),
                    );
                    return;
                }
            };
            match state
                .lock()
                .unwrap()
                .engine
                .stage_local_candidate(req.manifest, PathBuf::from(req.artifact_path))
            {
                Ok(()) => write_http(
                    &mut stream,
                    "200 OK",
                    "application/json",
                    &serde_json::json!({"ok": true, "authorization": "nominated_only"}).to_string(),
                ),
                Err(e) => write_http(
                    &mut stream,
                    "403 Forbidden",
                    "application/json",
                    &serde_json::json!({"ok": false, "error": e.to_string()}).to_string(),
                ),
            }
        }
        ("POST", "/api/inspector/authorize") => {
            let approver = serde_json::from_str::<AuthorizeReq>(body)
                .map(|r| r.approver)
                .unwrap_or_else(|_| "hue-local".to_string());
            match state
                .lock()
                .unwrap()
                .engine
                .authorize_staged_locally(&approver)
            {
                Ok(digest) => write_http(
                    &mut stream,
                    "200 OK",
                    "application/json",
                    &serde_json::json!({"ok": true, "authorized_blake3": digest}).to_string(),
                ),
                Err(e) => write_http(
                    &mut stream,
                    "403 Forbidden",
                    "application/json",
                    &serde_json::json!({"ok": false, "error": e.to_string()}).to_string(),
                ),
            }
        }
        ("POST", "/api/inspector/swap") => {
            match state.lock().unwrap().engine.activate_authorized_capsule() {
                Ok(bench) => write_http(
                    &mut stream,
                    "200 OK",
                    "application/json",
                    &serde_json::json!({"ok": true, "benchmark": bench}).to_string(),
                ),
                Err(e) => write_http(
                    &mut stream,
                    "403 Forbidden",
                    "application/json",
                    &serde_json::json!({"ok": false, "error": e.to_string()}).to_string(),
                ),
            }
        }
        ("POST", "/api/inspector/rollback") => {
            state.lock().unwrap().engine.rollback_to_baseline();
            let snap = state.lock().unwrap().engine.snapshot();
            write_http(
                &mut stream,
                "200 OK",
                "application/json",
                &serde_json::json!({
                    "ok": true,
                    "active_backend": snap.active_backend,
                    "session_nonce": snap.session_nonce,
                    "eval_counter": snap.eval_counter,
                })
                .to_string(),
            );
        }
        ("POST", "/api/inspector/driver/upgrade") => {
            let err = DriverLiveUpgradeGate::current()
                .check_live_upgrade()
                .unwrap_err();
            write_http(
                &mut stream,
                "403 Forbidden",
                "application/json",
                &serde_json::json!({
                    "ok": false,
                    "read_only": true,
                    "error": err.to_string(),
                })
                .to_string(),
            );
        }
        _ => write_http(
            &mut stream,
            "404 Not Found",
            "application/json",
            r#"{"ok":false,"error":"not_found"}"#,
        ),
    }
}

/// Bind and serve the tiny local AyeOS Capsule Inspector on `127.0.0.1:<port>`.
pub fn serve_inspector_loop(listener: TcpListener, state: Arc<Mutex<InspectorState>>) {
    for stream in listener.incoming().flatten() {
        handle_inspector_connection(stream, Arc::clone(&state));
    }
}

/// Helper to confirm sample manifest types stay linked for tests.
pub fn sample_boot_pinned_driver_manifest() -> CapsuleManifest {
    CapsuleManifest {
        id: "drv_virtio_net_boot_pinned".to_string(),
        version: 1,
        kind: CapsuleKind::Driver,
        target_arch: "x86_64".to_string(),
        interface: NarrowInterface::BootPinnedPciNet {
            vendor_id: 0x1AF4,
            device_id: 0x1000,
        },
        permissions: CapsulePermissions::Driver(DriverPermissions::boot_pinned_virtio_net()),
        lifecycle: CapsuleLifecycle::Driver(DriverLifecycle::BootPinnedReadOnly),
        signature_requirement: SignatureRequirement::Ed25519Only,
        zero_float_declared: true,
        artifact_blake3_hex: "00".repeat(32),
        ed25519_signature_hex: String::new(),
        secondary_signature_hex: None,
        nomination: None,
        authorization: DeviceAuthorization::BootPinned,
    }
}
