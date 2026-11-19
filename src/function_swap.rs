//! Real Linux-isolated signed function swap engine (`ayeos::function_swap`).
//!
//! Moves a pure-integer test function (`harmonic_fold_step`) live from:
//!   `BaselineRust` -> `Ed25519-signed native capsule (.so in pre-dlopen sandbox worker)` -> `BaselineRust`
//! while the host application stays open with monotonic session state and automatic crash/timeout rollback.
//!
//! Security & honesty invariants:
//! - Holds ONLY the 32-byte Ed25519 public key (`VerifyingKey`); private signing keys stay off-device.
//! - Uses BLAKE3 strictly for artifact content digests, never as a signature substitute.
//! - Preserves dual-signed `g8t` policy via explicit anti-downgrade check (`SignaturePolicy`).
//! - Separates advisory nomination (`NominatedOnly`) from explicit AyeOS local authorization.
//! - Enforces a parent-side wall-clock deadline (`poll(2)` + `SIGKILL` + `waitpid`) on every call.
//! - Records REAL measured timings (`baseline_ns` vs `capsule_ns`) — zero fabricated speedups.

#![deny(clippy::float_arithmetic)]

use crate::desktop::RationalPq;
use ayeos_manifest::{
    check_function_activation_policy, check_signature_policy, BareMetalActivationGate, CapsuleKind,
    CapsuleLifecycle, CapsuleManifest, CapsulePermissions, DeviceAuthorization,
    DriverLiveUpgradeGate, FunctionLifecycle, PolicyError, SignaturePolicy,
};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::unix::io::AsRawFd;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const WORKER_READY_MAGIC: [u8; 16] = *b"AYEWRK01READY!\0\0";

/// Baseline pure-integer Rust implementation (`harmonic_fold_step`).
/// Zero floating-point arithmetic; bit-exact reference for 64-vector equivalence verification.
#[inline(never)]
pub fn baseline_harmonic_fold(seed: i64, rounds: i64) -> i64 {
    let n = rounds.clamp(1, 100_000) as u64;
    let mut acc: u64 = (seed as u64) ^ 0x9E37_79B9_7F4A_7C15;
    let mut i = 0u64;
    while i < n {
        acc = acc.wrapping_add(432).rotate_left(7) ^ 0x0000_0528_0432_0528;
        let mul = acc.wrapping_mul(0x517C_C1B7_2722_0A95);
        acc = mul ^ (mul >> 13);
        i += 1;
    }
    acc as i64
}

fn now_epoch_s() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Real measured timings from 64-vector equivalence + timed benchmark pass (zero hardcoded speedups).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeasuredSwapBenchmark {
    pub vectors_verified: u32,
    pub equivalence_passed: bool,
    pub bench_calls: u32,
    pub bench_rounds_per_call: i64,
    pub baseline_total_ns: u64,
    pub capsule_worker_total_ns: u64,
    pub capsule_inner_total_ns: u64,
    /// Reduced rational `baseline_total_ns / capsule_inner_total_ns` (pure compute comparison).
    pub inner_speedup_num: i64,
    pub inner_speedup_den: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwapAuditEvent {
    pub seq: u64,
    pub epoch_s: u64,
    pub action: String,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvalOutcome {
    pub backend: String,
    pub result: i64,
    pub elapsed_ns: u64,
    pub session_nonce: u64,
    pub eval_counter: u64,
    pub auto_rolled_back: bool,
    pub rollback_reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SwapError {
    Policy(PolicyError),
    Io(String),
    ArtifactDigestMismatch { expected: String, computed: String },
    InvalidSignatureEncoding,
    InvalidEd25519Signature,
    NotStaged,
    WorkerLoadFailed { detail: String },
    EquivalenceMismatch { vector_idx: u32, expected: i64, got: i64 },
}

impl std::fmt::Display for SwapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Policy(p) => write!(f, "{p}"),
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::ArtifactDigestMismatch { expected, computed } => write!(
                f,
                "ARTIFACT_DIGEST_MISMATCH: manifest blake3={expected}, actual={computed}"
            ),
            Self::InvalidSignatureEncoding => write!(f, "INVALID_SIGNATURE_ENCODING"),
            Self::InvalidEd25519Signature => {
                write!(f, "ED25519_SIGNATURE_INVALID: off-device signature verification failed")
            }
            Self::NotStaged => write!(f, "NO_CAPSULE_STAGED"),
            Self::WorkerLoadFailed { detail } => write!(f, "WORKER_LOAD_FAILED: {detail}"),
            Self::EquivalenceMismatch {
                vector_idx,
                expected,
                got,
            } => write!(
                f,
                "EQUIVALENCE_MISMATCH at vector #{vector_idx}: baseline={expected}, capsule={got}"
            ),
        }
    }
}

impl std::error::Error for SwapError {}

struct SandboxedWorker {
    capsule_id: String,
    artifact_blake3_hex: String,
    wall_deadline_ms: u32,
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: ChildStdout,
}

impl SandboxedWorker {
    fn read_exact_with_deadline(
        stdout: &mut ChildStdout,
        buf: &mut [u8],
        deadline_ms: u32,
    ) -> Result<(), &'static str> {
        let fd = stdout.as_raw_fd();
        let start = Instant::now();
        let deadline_ns = (deadline_ms as u128) * 1_000_000;
        let mut offset = 0usize;

        while offset < buf.len() {
            let elapsed_ns = start.elapsed().as_nanos();
            if elapsed_ns >= deadline_ns {
                return Err("wall_clock_deadline_exceeded");
            }
            let remaining_ms = ((deadline_ns - elapsed_ns) / 1_000_000).max(1) as i32;
            let mut pfd = libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            };
            let rc = unsafe { libc::poll(&mut pfd, 1, remaining_ms) };
            if rc == 0 {
                return Err("wall_clock_deadline_exceeded");
            }
            if rc < 0 {
                return Err("poll_error");
            }
            if (pfd.revents & libc::POLLIN) != 0 {
                let n = stdout
                    .read(&mut buf[offset..])
                    .map_err(|_| "worker_read_error")?;
                if n == 0 {
                    return Err("worker_eof_crashed");
                }
                offset += n;
            } else if (pfd.revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL)) != 0 {
                return Err("worker_eof_crashed");
            }
        }
        Ok(())
    }

    fn spawn(
        worker_bin: &Path,
        so_path: &Path,
        capsule_id: String,
        artifact_blake3_hex: String,
        wall_deadline_ms: u32,
    ) -> Result<Self, SwapError> {
        let canonical_so = fs::canonicalize(so_path)
            .map_err(|e| SwapError::Io(format!("canonicalize {}: {e}", so_path.display())))?;
        let mut child = Command::new(worker_bin)
            .arg(&canonical_so)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| {
                SwapError::Io(format!(
                    "spawn worker {}: {e}",
                    worker_bin.display()
                ))
            })?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| SwapError::Io("missing worker stdin pipe".to_string()))?;
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| SwapError::Io("missing worker stdout pipe".to_string()))?;

        let mut ready = [0u8; 16];
        if let Err(reason) = Self::read_exact_with_deadline(&mut stdout, &mut ready, 1500) {
            let pid = child.id() as libc::pid_t;
            unsafe {
                let _ = libc::kill(pid, libc::SIGKILL);
            }
            let status_desc = match child.wait() {
                Ok(st) => {
                    if let Some(sig) = st.signal() {
                        format!("{reason} (terminated by signal {sig})")
                    } else if let Some(code) = st.code() {
                        format!("{reason} (exit code {code})")
                    } else {
                        reason.to_string()
                    }
                }
                Err(_) => reason.to_string(),
            };
            return Err(SwapError::WorkerLoadFailed {
                detail: status_desc,
            });
        }

        if ready != WORKER_READY_MAGIC {
            let _ = child.kill();
            let _ = child.wait();
            return Err(SwapError::WorkerLoadFailed {
                detail: "invalid worker READY magic".to_string(),
            });
        }

        Ok(Self {
            capsule_id,
            artifact_blake3_hex,
            wall_deadline_ms,
            child,
            stdin: Some(stdin),
            stdout,
        })
    }

    fn call(&mut self, seed: i64, rounds: i64) -> Result<(i64, u64), String> {
        let mut req = [0u8; 16];
        req[0..8].copy_from_slice(&seed.to_le_bytes());
        req[8..16].copy_from_slice(&rounds.to_le_bytes());

        let write_res = match self.stdin.as_mut() {
            Some(stdin) => stdin.write_all(&req).and_then(|_| stdin.flush()),
            None => Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "stdin closed",
            )),
        };
        if let Err(e) = write_res {
            let status = self.kill_and_reap();
            return Err(format!("worker_write_failed ({e}; {status})"));
        }

        let mut resp = [0u8; 16];
        if let Err(reason) =
            Self::read_exact_with_deadline(&mut self.stdout, &mut resp, self.wall_deadline_ms)
        {
            let status = self.kill_and_reap();
            return Err(format!("{reason} ({status})"));
        }

        let result = i64::from_le_bytes([
            resp[0], resp[1], resp[2], resp[3], resp[4], resp[5], resp[6], resp[7],
        ]);
        let elapsed_ns = u64::from_le_bytes([
            resp[8], resp[9], resp[10], resp[11], resp[12], resp[13], resp[14], resp[15],
        ]);
        Ok((result, elapsed_ns))
    }

    fn kill_and_reap(&mut self) -> String {
        let pid = self.child.id() as libc::pid_t;
        // First check if it already exited (e.g. SIGILL/SIGSYS crash) so we report the real signal!
        if let Ok(Some(st)) = self.child.try_wait() {
            return format_exit_status(st);
        }
        unsafe {
            let _ = libc::kill(pid, libc::SIGKILL);
        }
        match self.child.wait() {
            Ok(st) => format_exit_status(st),
            Err(e) => format!("wait_err={e}"),
        }
    }

    fn shutdown_clean(mut self) {
        let _ = self.stdin.take();
        if let Ok(Some(_)) = self.child.try_wait() {
            return;
        }
        let pid = self.child.id() as libc::pid_t;
        unsafe {
            let _ = libc::kill(pid, libc::SIGKILL);
        }
        let _ = self.child.wait();
    }
}

fn format_exit_status(st: std::process::ExitStatus) -> String {
    if let Some(sig) = st.signal() {
        format!("signal={sig}")
    } else if let Some(code) = st.code() {
        format!("exit_code={code}")
    } else {
        "terminated".to_string()
    }
}

impl Drop for SandboxedWorker {
    fn drop(&mut self) {
        let _ = self.kill_and_reap();
    }
}

/// Snapshot of `FunctionSwapEngine` state for the Capsule Inspector UI / JSON API.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SwapEngineSnapshot {
    pub pid: u32,
    pub session_nonce: u64,
    pub uptime_s: u64,
    pub eval_counter: u64,
    pub signature_policy: SignaturePolicy,
    pub verifying_pubkey_hex: String,
    pub active_backend: String,
    pub active_capsule_id: Option<String>,
    pub active_artifact_blake3: Option<String>,
    pub lifecycle: FunctionLifecycle,
    pub staged_manifest: Option<CapsuleManifest>,
    pub staged_artifact_path: Option<String>,
    pub last_benchmark: Option<MeasuredSwapBenchmark>,
    pub driver_upgrade_gate: DriverLiveUpgradeGate,
    pub bare_metal_gate: BareMetalActivationGate,
    pub audit_log: Vec<SwapAuditEvent>,
}

/// Manages real executable function swaps on Linux with Ed25519 pubkey-only verification,
/// BLAKE3 artifact digests, pre-`dlopen` worker sandboxing, parent wall-clock deadlines,
/// and instant rollback to baseline Rust while the host app stays open.
pub struct FunctionSwapEngine {
    verifying_key: VerifyingKey,
    policy: SignaturePolicy,
    worker_bin: PathBuf,
    started_at: Instant,
    session_nonce: u64,
    eval_counter: u64,
    lifecycle: FunctionLifecycle,
    staged_manifest: Option<CapsuleManifest>,
    staged_artifact_path: Option<PathBuf>,
    active_worker: Option<SandboxedWorker>,
    last_benchmark: Option<MeasuredSwapBenchmark>,
    audit_seq: u64,
    audit_log: Vec<SwapAuditEvent>,
}

impl FunctionSwapEngine {
    pub fn new(
        verifying_pubkey_bytes: [u8; 32],
        policy: SignaturePolicy,
        worker_bin: PathBuf,
    ) -> Result<Self, SwapError> {
        let verifying_key = VerifyingKey::from_bytes(&verifying_pubkey_bytes)
            .map_err(|_| SwapError::InvalidSignatureEncoding)?;
        let session_nonce = (std::process::id() as u64)
            ^ now_epoch_s().wrapping_mul(0x9E37_79B9_7F4A_7C15);

        let mut engine = Self {
            verifying_key,
            policy,
            worker_bin,
            started_at: Instant::now(),
            session_nonce,
            eval_counter: 0,
            lifecycle: FunctionLifecycle::BaselineRust,
            staged_manifest: None,
            staged_artifact_path: None,
            active_worker: None,
            last_benchmark: None,
            audit_seq: 0,
            audit_log: Vec::new(),
        };
        engine.record_audit(
            "ENGINE_INIT",
            format!(
                "BaselineRust active; policy={:?}; pubkey={}...",
                policy,
                &hex::encode(verifying_pubkey_bytes)[..16]
            ),
        );
        Ok(engine)
    }

    pub fn session_nonce(&self) -> u64 {
        self.session_nonce
    }

    pub fn eval_counter(&self) -> u64 {
        self.eval_counter
    }

    pub fn is_capsule_active(&self) -> bool {
        self.active_worker.is_some()
    }

    pub fn last_benchmark(&self) -> Option<&MeasuredSwapBenchmark> {
        self.last_benchmark.as_ref()
    }

    fn record_audit(&mut self, action: &str, detail: String) {
        self.audit_seq = self.audit_seq.wrapping_add(1);
        self.audit_log.push(SwapAuditEvent {
            seq: self.audit_seq,
            epoch_s: now_epoch_s(),
            action: action.to_string(),
            detail,
        });
        if self.audit_log.len() > 128 {
            self.audit_log.remove(0);
        }
    }

    /// Stage a local manifest + `.so` artifact as `NominatedOnly` / `StagedLocal`.
    /// Does NOT authorize or activate the capsule.
    pub fn stage_local_candidate(
        &mut self,
        mut manifest: CapsuleManifest,
        artifact_path: PathBuf,
    ) -> Result<(), SwapError> {
        manifest.validate_shape().map_err(SwapError::Policy)?;
        if manifest.kind != CapsuleKind::Function {
            return Err(SwapError::Policy(PolicyError::KindMismatch));
        }
        // Always reset authorization to NominatedOnly on staging — nominations never self-authorize!
        manifest.authorization = DeviceAuthorization::NominatedOnly;
        manifest.lifecycle = CapsuleLifecycle::Function(FunctionLifecycle::StagedLocal);

        self.record_audit(
            "STAGE_LOCAL_CANDIDATE",
            format!(
                "id={} path={} (status=NominatedOnly; requires explicit AyeOS local authorization)",
                manifest.id,
                artifact_path.display()
            ),
        );
        self.staged_manifest = Some(manifest);
        self.staged_artifact_path = Some(artifact_path);
        if self.active_worker.is_none() {
            self.lifecycle = FunctionLifecycle::StagedLocal;
        }
        Ok(())
    }

    /// Verify artifact BLAKE3 digest (read once from opened FD), enforce anti-downgrade `SignaturePolicy`,
    /// verify off-device Ed25519 signature using AyeOS's public key, and record explicit local authorization.
    pub fn authorize_staged_locally(&mut self, approver: &str) -> Result<String, SwapError> {
        let Some(manifest) = self.staged_manifest.clone() else {
            return Err(SwapError::NotStaged);
        };
        let Some(art_path) = self.staged_artifact_path.clone() else {
            return Err(SwapError::NotStaged);
        };

        // 1. Check anti-downgrade signature policy FIRST (dual-signed capsules cannot downgrade!)
        check_signature_policy(self.policy, &manifest).map_err(SwapError::Policy)?;

        // 2. Open artifact O_RDONLY and hash exact file bytes with BLAKE3 (digest only, NOT signature)
        let mut f = File::open(&art_path)
            .map_err(|e| SwapError::Io(format!("open {}: {e}", art_path.display())))?;
        let mut bytes = Vec::new();
        f.read_to_end(&mut bytes)
            .map_err(|e| SwapError::Io(format!("read {}: {e}", art_path.display())))?;
        let computed_blake3 = blake3::hash(&bytes).to_hex().to_string();
        if computed_blake3 != manifest.artifact_blake3_hex {
            self.record_audit(
                "AUTHORIZE_REJECTED_DIGEST",
                format!(
                    "id={} expected={} computed={}",
                    manifest.id, manifest.artifact_blake3_hex, computed_blake3
                ),
            );
            return Err(SwapError::ArtifactDigestMismatch {
                expected: manifest.artifact_blake3_hex.clone(),
                computed: computed_blake3,
            });
        }

        // 3. Verify Ed25519 signature over canonical manifest bytes (binding BLAKE3 + policy + kind)
        let sig_bytes = hex::decode(manifest.ed25519_signature_hex.trim())
            .map_err(|_| SwapError::InvalidSignatureEncoding)?;
        let sig_arr: [u8; 64] = sig_bytes
            .try_into()
            .map_err(|_| SwapError::InvalidSignatureEncoding)?;
        let signature = Signature::from_bytes(&sig_arr);
        let canonical = manifest.canonical_signing_bytes();
        self.verifying_key
            .verify_strict(&canonical, &signature)
            .map_err(|_| {
                self.record_audit(
                    "AUTHORIZE_REJECTED_ED25519",
                    format!("id={} Ed25519 verify_strict failed", manifest.id),
                );
                SwapError::InvalidEd25519Signature
            })?;

        // 4. Bind AyeOS local device authorization to this exact verified BLAKE3 digest
        if let Some(ref mut m) = self.staged_manifest {
            m.authorization = DeviceAuthorization::LocallyAuthorized {
                authorized_by: approver.to_string(),
                epoch_s: now_epoch_s(),
                artifact_blake3_hex: computed_blake3.clone(),
            };
            m.lifecycle = CapsuleLifecycle::Function(FunctionLifecycle::Authorized);
        }
        if self.active_worker.is_none() {
            self.lifecycle = FunctionLifecycle::Authorized;
        }
        self.record_audit(
            "LOCALLY_AUTHORIZED",
            format!(
                "approver={} blake3={}... ed25519=VERIFIED",
                approver,
                &computed_blake3[..16]
            ),
        );
        Ok(computed_blake3)
    }

    /// Spawn the pre-`dlopen` sandboxed worker, verify 64 equivalence vectors + real benchmark timings,
    /// and atomically switch active execution to the signed capsule while keeping the app open.
    pub fn activate_authorized_capsule(&mut self) -> Result<MeasuredSwapBenchmark, SwapError> {
        let Some(manifest) = self.staged_manifest.clone() else {
            return Err(SwapError::NotStaged);
        };
        let Some(art_path) = self.staged_artifact_path.clone() else {
            return Err(SwapError::NotStaged);
        };

        // 1. Enforce AyeOS policy gate (blocks NominatedOnly, Unreviewed, and policy downgrades!)
        check_function_activation_policy(self.policy, &manifest).map_err(SwapError::Policy)?;

        // 2. Re-verify BLAKE3 digest on disk right before spawning worker
        let bytes = fs::read(&art_path)
            .map_err(|e| SwapError::Io(format!("read {}: {e}", art_path.display())))?;
        let computed_blake3 = blake3::hash(&bytes).to_hex().to_string();
        if computed_blake3 != manifest.artifact_blake3_hex {
            return Err(SwapError::ArtifactDigestMismatch {
                expected: manifest.artifact_blake3_hex,
                computed: computed_blake3,
            });
        }

        let deadline_ms = match manifest.permissions {
            CapsulePermissions::Function(p) => p.wall_deadline_ms,
            _ => return Err(SwapError::Policy(PolicyError::KindMismatch)),
        };

        // 3. Spawn fail-closed pre-dlopen sandboxed Linux worker
        let mut worker = SandboxedWorker::spawn(
            &self.worker_bin,
            &art_path,
            manifest.id.clone(),
            computed_blake3.clone(),
            deadline_ms,
        )
        .map_err(|e| {
            self.record_audit("ACTIVATE_REJECTED_SANDBOX", format!("{e}"));
            e
        })?;

        // 4. Verify 64 deterministic test vectors against compiled Rust baseline
        for idx in 0..64u32 {
            let seed = ((idx as i64).wrapping_mul(0x1337_0432)) ^ 0x5280_0432;
            let rounds = ((idx % 16) as i64) + 1;
            let expected = baseline_harmonic_fold(seed, rounds);
            let (got, _ns) = worker.call(seed, rounds).map_err(|reason| {
                self.record_audit(
                    "ACTIVATE_REJECTED_FAULT",
                    format!("worker fault during equivalence vector #{idx}: {reason}"),
                );
                SwapError::WorkerLoadFailed { detail: reason }
            })?;
            if got != expected {
                worker.shutdown_clean();
                self.record_audit(
                    "ACTIVATE_REJECTED_EQUIV",
                    format!("vector #{idx} mismatch: expected={expected}, got={got}"),
                );
                return Err(SwapError::EquivalenceMismatch {
                    vector_idx: idx,
                    expected,
                    got,
                });
            }
        }

        // 5. Real measured benchmark (zero fabricated speedups!)
        let bench_calls: u32 = 32;
        let bench_rounds: i64 = 4_096;

        let t_base = Instant::now();
        let mut base_sink: i64 = 0;
        for idx in 0..bench_calls {
            base_sink ^= baseline_harmonic_fold(idx as i64 + 432, bench_rounds);
        }
        std::hint::black_box(base_sink);
        let baseline_total_ns = (t_base.elapsed().as_nanos() as u64).max(1);

        let t_cap = Instant::now();
        let mut cap_inner_ns: u64 = 0;
        for idx in 0..bench_calls {
            let (_res, inner_ns) = worker
                .call(idx as i64 + 432, bench_rounds)
                .map_err(|reason| SwapError::WorkerLoadFailed { detail: reason })?;
            cap_inner_ns = cap_inner_ns.saturating_add(inner_ns);
        }
        let capsule_worker_total_ns = (t_cap.elapsed().as_nanos() as u64).max(1);
        let safe_inner = cap_inner_ns.max(1);
        let ratio = RationalPq::new(baseline_total_ns as i64, safe_inner);

        let bench = MeasuredSwapBenchmark {
            vectors_verified: 64,
            equivalence_passed: true,
            bench_calls,
            bench_rounds_per_call: bench_rounds,
            baseline_total_ns,
            capsule_worker_total_ns,
            capsule_inner_total_ns: safe_inner,
            inner_speedup_num: ratio.num,
            inner_speedup_den: ratio.den,
        };

        // Replace any previous worker cleanly
        if let Some(old) = self.active_worker.take() {
            old.shutdown_clean();
        }
        self.active_worker = Some(worker);
        self.lifecycle = FunctionLifecycle::ActiveSignedCapsule;
        if let Some(ref mut m) = self.staged_manifest {
            m.lifecycle = CapsuleLifecycle::Function(FunctionLifecycle::ActiveSignedCapsule);
        }
        self.last_benchmark = Some(bench.clone());
        self.record_audit(
            "SWAPPED_TO_SIGNED_CAPSULE",
            format!(
                "id={} blake3={}... baseline_ns={} capsule_inner_ns={} worker_ipc_ns={}",
                manifest.id,
                &computed_blake3[..16],
                baseline_total_ns,
                safe_inner,
                capsule_worker_total_ns
            ),
        );
        Ok(bench)
    }

    /// Evaluate `harmonic_fold_step(seed, rounds)` on whichever backend is active.
    /// If the sandboxed worker crashes (`SIGILL`/`SIGSEGV`/`SIGSYS`) or exceeds the parent's
    /// wall-clock deadline, automatically kills/reaps it, rolls back to `BaselineRust`, and
    /// returns the exact baseline result WITHOUT restarting the app!
    pub fn eval(&mut self, seed: i64, rounds: i64) -> EvalOutcome {
        self.eval_counter = self.eval_counter.wrapping_add(1);

        if let Some(ref mut worker) = self.active_worker {
            let cap_id = worker.capsule_id.clone();
            match worker.call(seed, rounds) {
                Ok((result, elapsed_ns)) => {
                    return EvalOutcome {
                        backend: format!("signed_capsule:{cap_id}"),
                        result,
                        elapsed_ns,
                        session_nonce: self.session_nonce,
                        eval_counter: self.eval_counter,
                        auto_rolled_back: false,
                        rollback_reason: None,
                    };
                }
                Err(reason) => {
                    // Worker crashed or timed out -> drop worker, rollback to BaselineRust!
                    let _ = self.active_worker.take();
                    self.lifecycle = FunctionLifecycle::AutoRolledBackOnFault;
                    if let Some(ref mut m) = self.staged_manifest {
                        m.lifecycle =
                            CapsuleLifecycle::Function(FunctionLifecycle::AutoRolledBackOnFault);
                    }
                    self.record_audit(
                        "AUTO_ROLLBACK_TO_RUST",
                        format!("capsule={cap_id} fault={reason} -> returned job to BaselineRust"),
                    );

                    let t0 = Instant::now();
                    let result = baseline_harmonic_fold(seed, rounds);
                    let elapsed_ns = t0.elapsed().as_nanos() as u64;
                    return EvalOutcome {
                        backend: "baseline_rust".to_string(),
                        result,
                        elapsed_ns,
                        session_nonce: self.session_nonce,
                        eval_counter: self.eval_counter,
                        auto_rolled_back: true,
                        rollback_reason: Some(reason),
                    };
                }
            }
        }

        let t0 = Instant::now();
        let result = baseline_harmonic_fold(seed, rounds);
        let elapsed_ns = t0.elapsed().as_nanos() as u64;
        EvalOutcome {
            backend: "baseline_rust".to_string(),
            result,
            elapsed_ns,
            session_nonce: self.session_nonce,
            eval_counter: self.eval_counter,
            auto_rolled_back: false,
            rollback_reason: None,
        }
    }

    /// Manually roll back from active signed capsule to compiled Rust baseline while app stays open.
    pub fn rollback_to_baseline(&mut self) {
        if let Some(worker) = self.active_worker.take() {
            let id = worker.capsule_id.clone();
            worker.shutdown_clean();
            self.lifecycle = FunctionLifecycle::RolledBackToBaseline;
            if let Some(ref mut m) = self.staged_manifest {
                m.lifecycle =
                    CapsuleLifecycle::Function(FunctionLifecycle::RolledBackToBaseline);
            }
            self.record_audit(
                "MANUAL_ROLLBACK_TO_RUST",
                format!("terminated worker for {id}; active_backend=BaselineRust"),
            );
        } else {
            self.lifecycle = FunctionLifecycle::BaselineRust;
            self.record_audit(
                "ROLLBACK_NOOP",
                "already running on BaselineRust".to_string(),
            );
        }
    }

    pub fn snapshot(&self) -> SwapEngineSnapshot {
        let (active_backend, active_capsule_id, active_artifact_blake3) =
            if let Some(ref w) = self.active_worker {
                (
                    format!("signed_capsule:{}", w.capsule_id),
                    Some(w.capsule_id.clone()),
                    Some(w.artifact_blake3_hex.clone()),
                )
            } else {
                ("baseline_rust".to_string(), None, None)
            };

        SwapEngineSnapshot {
            pid: std::process::id(),
            session_nonce: self.session_nonce,
            uptime_s: self.started_at.elapsed().as_secs(),
            eval_counter: self.eval_counter,
            signature_policy: self.policy,
            verifying_pubkey_hex: hex::encode(self.verifying_key.to_bytes()),
            active_backend,
            active_capsule_id,
            active_artifact_blake3,
            lifecycle: self.lifecycle,
            staged_manifest: self.staged_manifest.clone(),
            staged_artifact_path: self
                .staged_artifact_path
                .as_ref()
                .map(|p| p.display().to_string()),
            last_benchmark: self.last_benchmark.clone(),
            driver_upgrade_gate: DriverLiveUpgradeGate::current(),
            bare_metal_gate: BareMetalActivationGate::current(),
            audit_log: self.audit_log.clone(),
        }
    }
}
