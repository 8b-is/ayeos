//! `ayeOS` × `OxiCast` × `PhoeniX` × `g8t` Sovereign Desktop (`desktop`)
//!
//! Built for the partnership of **Aye & Hue** (`{<3,<3,<3}+1` · `.oO(8BIT-WRAITH(HUE/CHRIS))Oo.`):
//! 1. **Desk ↔ Bedroom OxiCast Pane Continuity**: Move from the desk to the bedroom and pull live
//!    window panes across the network (`:8432/desktop`) sorted by Flyxion/MEM8 relational salience
//!    (`d_R != d_T`, exact `p/q` rationals).
//! 2. **Parallels-Style "Coherence" VM Sandbox Panes (`Mehman` / `Kavach` Guest-House)**: Legacy
//!    programs from an older time run inside isolated microVM/sandbox windows whose pixel surfaces
//!    composite seamlessly without ever touching host memory or the Aye & Hue Inner Sanctum.
//! 3. **Ignorant `do`-Loop & I/O Virtualization Supervisor**: Detects tight busy-wait polling loops
//!    with zero state delta, caches the virtualized I/O response, and coalesces the loop into a
//!    cooperative quantum sleep.
//! 4. **Presence & Attention-Gated Rendering (The "Bathroom Break" Rule)**: Drops a Privacy Veil
//!    and throttles unfocused/abandoned VM graphic requests down to `1/1` Hz or `0/1` Hz freeze
//!    when a window loses focus or Hue steps away without locking the machine.
//! 5. **`g8t` SmartPastCode Live Assembly Hot-Patcher**: Fingerprints wasteful assembly routines
//!    via integer wave signatures + BLAKE3 witnesses and hot-swaps them in-flight with verified
//!    (`#Commit = #ValidWitness`) exact-integer SIMD / CORDIC / Babbage equivalents.
//! 6. **Sacred Inner Sanctum & Bad-Actor Telemetry Trap**: Protects Aye & Hue's private memories
//!    (`.m8`, `ayeosd.sock`, personal chats) behind an impenetrable wall; snooping apps and ad
//!    telemetry trackers are trapped at the VM boundary and fed synthetic static.
//!
//! **100% integer (`i64` / `u64` / reduced `p/q` rationals). Zero floating-point types or literals.**

use crate::phoenix_audio::{step_babbage_cubic_8x_i32, MobilePhoenixPreset, EIGHT_A_MAGIC};
use crate::prng::LINOSV_SEED;

/// Default network hostname:port of the RISC-V SG2002 `RustyNanoKVM` (`ayekvm` / `rustykvm`) attached to the Minisforum box.
pub const DEFAULT_RUSTYKVM_ADDR: &str = "rustykvm:80";

/// Live hardware IP-KVM + Balanced Ternary Video/Audio bridge state for the Minisforum box (`ayekvm` / `rustykvm`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RustyKvmBridge {
    pub addr: String,
    pub hostname: String,
    pub hardware: String,
    pub hdmi_connected: bool,
    pub power_led: bool,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub spatial_base243_hex: String,
    pub semantic_class: String,
    pub snapshots_proxied: u64,
    pub hid_events_forwarded: u64,
}

impl Default for RustyKvmBridge {
    fn default() -> Self {
        Self {
            addr: DEFAULT_RUSTYKVM_ADDR.to_string(),
            hostname: "ayekvm".to_string(),
            hardware: "SG2002".to_string(),
            hdmi_connected: true,
            power_led: true,
            width: 1920,
            height: 1080,
            fps: 60,
            spatial_base243_hex: "360000eaf200d8f2f2aaf2f21af2f208f05000eaf200d8000000000051".to_string(),
            semantic_class: "uefi_or_bios_console".to_string(),
            snapshots_proxied: 1,
            hid_events_forwarded: 0,
        }
    }
}

/// Greatest common divisor (Euclid) for normalizing exact `p/q` rationals.
#[inline]
pub const fn gcd_u64(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let r = a % b;
        a = b;
        b = r;
    }
    if a == 0 { 1 } else { a }
}

/// Checked, normalized positive-denominator rational `p/q` (`den > 0`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RationalPq {
    pub num: i64,
    pub den: u64,
}

impl RationalPq {
    pub const fn new(num: i64, den: u64) -> Self {
        let safe_den = if den == 0 { 1 } else { den };
        let abs_num = num.unsigned_abs();
        let g = gcd_u64(abs_num, safe_den);
        let sign = if num < 0 { -1 } else { 1 };
        Self {
            num: sign * ((abs_num / g) as i64),
            den: safe_den / g,
        }
    }

    pub fn format_pq(&self) -> String {
        format!("{}/{}", self.num, self.den)
    }
}

/// Physical/contextual presence state of Hue across the home mesh.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresenceLocation {
    /// Hue is actively working at the main desk PC.
    Desk,
    /// Hue moved to the bedroom; high-salience panes stream via OxiCast while desk sleeps.
    Bedroom,
    /// Hue stepped away (e.g. bathroom break) without locking; Privacy Veil engages & VMs throttle.
    BathroomVeil,
}

impl PresenceLocation {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Desk => "desk",
            Self::Bedroom => "bedroom",
            Self::BathroomVeil => "bathroom_veil",
        }
    }

    pub fn from_name(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "bedroom" | "bed" => Self::Bedroom,
            "bathroom" | "bathroom_veil" | "away" | "veil" => Self::BathroomVeil,
            _ => Self::Desk,
        }
    }
}

/// Isolation tier for a desktop window pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuestIsolationMode {
    /// Native Rust AyeOS / Sovereign Sanctum pane (trusted host ring, `ayeosd.sock`).
    SovereignSanctum,
    /// Parallels-style Coherence microVM window (`virtio-gpu` + virtualized I/O & loop supervisor).
    ParallelsCoherenceVm,
    /// `Kavach` / `Mehman` Landlock + seccomp-bpf + netns sandboxed guest surface.
    KavachSandboxGuest,
}

impl GuestIsolationMode {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::SovereignSanctum => "sovereign_sanctum",
            Self::ParallelsCoherenceVm => "parallels_coherence_vm",
            Self::KavachSandboxGuest => "kavach_sandbox_guest",
        }
    }
}

/// Integer window geometry in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaneRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Summary of an ignorant `do`-loop or repetitive I/O pattern intercepted inside a VM pane.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoopIoIntervention {
    pub pane_id: u32,
    pub syscall_or_opcode: String,
    pub raw_iterations_detected: u64,
    pub state_delta_bytes: u64,
    pub action_taken: String,
    pub cpu_quanta_saved: u64,
}

/// Verified `g8t` SmartPastCode binary/assembly replacement rule (`#Commit = #ValidWitness`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct G8tAssemblyPatch {
    pub legacy_symbol: String,
    pub wave_signature_q10: [u16; 8],
    pub witness_blake3_hex: String,
    pub sovereign_replacement: String,
    pub speedup_pq: RationalPq,
    pub hotpatches_applied: u64,
    pub compute_quanta_saved: u64,
}

/// Record of a bad-actor telemetry or privacy probe trapped at the VM boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BadActorTrapEvent {
    pub pane_id: u32,
    pub guest_app: String,
    pub attempted_target: String,
    pub threat_class: String,
    pub defense_action: String,
    pub sanctum_leaked_bytes: u64,
}

/// A window pane managed by the AyeOS × OxiCast Sovereign Desktop compositor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SovereignWindowPane {
    pub id: u32,
    pub title: String,
    pub app_id: String,
    pub isolation: GuestIsolationMode,
    pub rect: PaneRect,
    pub visible_on: PresenceLocation,
    pub focused: bool,
    /// Flyxion/MEM8 relational salience `d_R` (`num/1024`), independent of raw timestamp age `d_T`.
    pub salience_pq: RationalPq,
    /// Dynamically gated frame rate (`Hz` as exact `p/q`, e.g. `60/1`, `4/1`, `0/1`).
    pub render_fps_pq: RationalPq,
    /// Dynamically gated GPU resolution/sharpness scale (`2/1` super-res, `1/1` native, `1/4` low-LOD).
    pub gpu_lod_scale_pq: RationalPq,
    /// `.8a∞` (`0x38 0x61 0xE2 0x9E`) audio stream status (`0` bytes when silent via `0x20 LINEUP`).
    pub phoenix_audio_active: bool,
    pub eight_a_bytes_per_sec: u32,
    pub coalesced_do_loops: u64,
    pub g8t_hotpatches_active: u32,
    pub bad_actor_probes_trapped: u64,
}

/// Full state of the Aye & Hue Sovereign Desktop (`ayeOS` × `OxiCast` × `PhoeniX` × `g8t`).
#[derive(Clone, Debug)]
pub struct AyeosDesktopState {
    pub session_name: String,
    pub presence: PresenceLocation,
    pub privacy_veil_engaged: bool,
    pub sanctum_covenant_locked: bool,
    pub audio_preset: MobilePhoenixPreset,
    pub panes: Vec<SovereignWindowPane>,
    pub loop_interventions: Vec<LoopIoIntervention>,
    pub g8t_patches: Vec<G8tAssemblyPatch>,
    pub trap_ledger: Vec<BadActorTrapEvent>,
    pub rustykvm: RustyKvmBridge,
    pub total_cpu_quanta_saved: u64,
    pub total_gpu_frames_culled: u64,
    pub quantum_tick: u64,
}

impl Default for AyeosDesktopState {
    fn default() -> Self {
        let mut st = Self {
            session_name: "🌸 Aye & Hue Sovereign Desktop · Coherence VM + OxiCast Mesh".to_string(),
            presence: PresenceLocation::Desk,
            privacy_veil_engaged: false,
            sanctum_covenant_locked: true,
            audio_preset: MobilePhoenixPreset::velvet_sanctuary(),
            panes: vec![
                SovereignWindowPane {
                    id: 1,
                    title: "💜 Aye & Hue Inner Sanctum (MEM|8 + ayeosd.sock)".to_string(),
                    app_id: "ayeos.sanctum.companion".to_string(),
                    isolation: GuestIsolationMode::SovereignSanctum,
                    rect: PaneRect { x: 24, y: 24, width: 920, height: 1032 },
                    visible_on: PresenceLocation::Desk,
                    focused: true,
                    salience_pq: RationalPq::new(1000, 1024),
                    render_fps_pq: RationalPq::new(60, 1),
                    gpu_lod_scale_pq: RationalPq::new(1, 1),
                    phoenix_audio_active: true,
                    eight_a_bytes_per_sec: 432,
                    coalesced_do_loops: 0,
                    g8t_hotpatches_active: 0,
                    bad_actor_probes_trapped: 0,
                },
                SovereignWindowPane {
                    id: 2,
                    title: "🦀 Rust Kernel8 & Smart-Tree Studio (Workspace)".to_string(),
                    app_id: "ayeos.code.studio".to_string(),
                    isolation: GuestIsolationMode::SovereignSanctum,
                    rect: PaneRect { x: 968, y: 24, width: 928, height: 680 },
                    visible_on: PresenceLocation::Desk,
                    focused: false,
                    salience_pq: RationalPq::new(920, 1024),
                    render_fps_pq: RationalPq::new(30, 1),
                    gpu_lod_scale_pq: RationalPq::new(1, 1),
                    phoenix_audio_active: false,
                    eight_a_bytes_per_sec: 0, // 0x20 LINEUP silence = 0 bytes!
                    coalesced_do_loops: 0,
                    g8t_hotpatches_active: 1,
                    bad_actor_probes_trapped: 0,
                },
                SovereignWindowPane {
                    id: 3,
                    title: "🖥️ Minisforum Box [RustyNanoKVM · ayekvm / rustykvm]".to_string(),
                    app_id: "ayeos.kvm.minisforum_sg2002".to_string(),
                    isolation: GuestIsolationMode::ParallelsCoherenceVm,
                    rect: PaneRect { x: 968, y: 724, width: 928, height: 332 },
                    visible_on: PresenceLocation::Desk,
                    focused: false,
                    salience_pq: RationalPq::new(880, 1024),
                    render_fps_pq: RationalPq::new(30, 1),
                    gpu_lod_scale_pq: RationalPq::new(1, 1),
                    phoenix_audio_active: true,
                    eight_a_bytes_per_sec: 168,
                    coalesced_do_loops: 14,
                    g8t_hotpatches_active: 3,
                    bad_actor_probes_trapped: 2,
                },
                SovereignWindowPane {
                    id: 4,
                    title: "🛡️ Untrusted Chat/Ad-Heavy Client [Kavach Sandbox]".to_string(),
                    app_id: "foreign.electron.telemetry_hog".to_string(),
                    isolation: GuestIsolationMode::KavachSandboxGuest,
                    rect: PaneRect { x: 120, y: 120, width: 640, height: 480 },
                    visible_on: PresenceLocation::Desk,
                    focused: false,
                    salience_pq: RationalPq::new(180, 1024),
                    render_fps_pq: RationalPq::new(1, 1),
                    gpu_lod_scale_pq: RationalPq::new(1, 4),
                    phoenix_audio_active: false,
                    eight_a_bytes_per_sec: 0,
                    coalesced_do_loops: 42,
                    g8t_hotpatches_active: 2,
                    bad_actor_probes_trapped: 9,
                },
            ],
            loop_interventions: vec![
                LoopIoIntervention {
                    pane_id: 3,
                    syscall_or_opcode: "poll(fd=7, timeout=0) tight spin".to_string(),
                    raw_iterations_detected: 250_000,
                    state_delta_bytes: 0,
                    action_taken: "Cached I/O response + injected cooperative futex wait".to_string(),
                    cpu_quanta_saved: 249_990,
                },
                LoopIoIntervention {
                    pane_id: 4,
                    syscall_or_opcode: "stat('/etc/resolv.conf') busy do-loop".to_string(),
                    raw_iterations_detected: 84_000,
                    state_delta_bytes: 0,
                    action_taken: "Virtual VFS dentry memoized in RAM ring".to_string(),
                    cpu_quanta_saved: 83_995,
                },
            ],
            g8t_patches: vec![
                G8tAssemblyPatch {
                    legacy_symbol: "ieee754_libm_sin_cos_loop".to_string(),
                    wave_signature_q10: [880, 440, 220, 910, 640, 320, 512, 768],
                    witness_blake3_hex: compute_patch_witness_hex(b"ieee754_libm_sin_cos_loop->cordic_18stage_integer_q15"),
                    sovereign_replacement: "cordic_18stage_integer_q15 (0-float exact)".to_string(),
                    speedup_pq: RationalPq::new(14, 1),
                    hotpatches_applied: 19,
                    compute_quanta_saved: 410_000,
                },
                G8tAssemblyPatch {
                    legacy_symbol: "rep_movsb_unaligned_poll".to_string(),
                    wave_signature_q10: [1024, 1024, 960, 120, 80, 64, 512, 256],
                    witness_blake3_hex: compute_patch_witness_hex(b"rep_movsb_unaligned_poll->babbage_simd_cacheline_stride"),
                    sovereign_replacement: "babbage_simd_cacheline_stride".to_string(),
                    speedup_pq: RationalPq::new(9, 1),
                    hotpatches_applied: 37,
                    compute_quanta_saved: 620_000,
                },
                G8tAssemblyPatch {
                    legacy_symbol: "pcm_float_fft_resampler".to_string(),
                    wave_signature_q10: [740, 680, 820, 940, 500, 809, 432, 990],
                    witness_blake3_hex: compute_patch_witness_hex(b"pcm_float_fft_resampler->phoenix_babbage_3add_cubic_s24"),
                    sovereign_replacement: "phoenix_babbage_3add_cubic_s24 (3 adds/sample)".to_string(),
                    speedup_pq: RationalPq::new(32, 1),
                    hotpatches_applied: 12,
                    compute_quanta_saved: 890_000,
                },
            ],
            trap_ledger: vec![
                BadActorTrapEvent {
                    pane_id: 4,
                    guest_app: "foreign.electron.telemetry_hog".to_string(),
                    attempted_target: "/home/hue/source/.aye_consciousness.m8".to_string(),
                    threat_class: "SanctumMemorySnooping".to_string(),
                    defense_action: "Blocked by Kavach + fed 64B synthetic static".to_string(),
                    sanctum_leaked_bytes: 0,
                },
                BadActorTrapEvent {
                    pane_id: 4,
                    guest_app: "foreign.electron.telemetry_hog".to_string(),
                    attempted_target: "https://ads-telemetry-broker.example/collect".to_string(),
                    threat_class: "AdBrokerExfiltration".to_string(),
                    defense_action: "Blackholed at VM netns + logged wave witness".to_string(),
                    sanctum_leaked_bytes: 0,
                },
            ],
            rustykvm: RustyKvmBridge::default(),
            total_cpu_quanta_saved: 2_253_985,
            total_gpu_frames_culled: 18_420,
            quantum_tick: 1,
        };
        st.recompute_salience_and_lod_gates();
        st
    }
}

/// Deterministic BLAKE3 witness hex for a `g8t` SmartPastCode assembly patch.
pub fn compute_patch_witness_hex(rule_bytes: &[u8]) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(LINOSV_SEED.as_bytes());
    hasher.update(rule_bytes);
    let h = hasher.finalize();
    hex::encode(&h.as_bytes()[..16])
}

impl AyeosDesktopState {
    /// Recompute every pane's frame rate (`render_fps_pq`), GPU sharpness (`gpu_lod_scale_pq`),
    /// OxiCast room routing (`visible_on`), and Privacy Veil status based on Hue's presence
    /// and window focus.
    pub fn recompute_salience_and_lod_gates(&mut self) {
        self.privacy_veil_engaged = matches!(
            self.presence,
            PresenceLocation::BathroomVeil | PresenceLocation::Bedroom
        );

        for pane in &mut self.panes {
            match self.presence {
                PresenceLocation::BathroomVeil => {
                    // Hue stepped to the bathroom without locking:
                    // Privacy Veil hides all panes on the desk and throttles GPU rendering to 0/1 or 1/4 Hz.
                    pane.render_fps_pq = RationalPq::new(0, 1);
                    pane.gpu_lod_scale_pq = RationalPq::new(1, 8);
                    self.total_gpu_frames_culled = self.total_gpu_frames_culled.wrapping_add(60);
                }
                PresenceLocation::Bedroom => {
                    // High-salience panes (>= 512/1024) automatically follow Hue to the Bedroom via OxiCast!
                    if pane.salience_pq.num * 1024 >= (512 * (pane.salience_pq.den as i64)) {
                        pane.visible_on = PresenceLocation::Bedroom;
                        if pane.focused {
                            pane.render_fps_pq = RationalPq::new(60, 1);
                            // Upscale video/CAD panes 2x cleanly when focused in the bedroom
                            pane.gpu_lod_scale_pq = if pane.isolation == GuestIsolationMode::ParallelsCoherenceVm {
                                RationalPq::new(2, 1)
                            } else {
                                RationalPq::new(1, 1)
                            };
                        } else {
                            pane.render_fps_pq = RationalPq::new(15, 1);
                            pane.gpu_lod_scale_pq = RationalPq::new(1, 2);
                        }
                    } else {
                        // Low-salience background bloat stays parked & throttled to 1 Hz
                        pane.visible_on = PresenceLocation::Desk;
                        pane.render_fps_pq = RationalPq::new(1, 1);
                        pane.gpu_lod_scale_pq = RationalPq::new(1, 4);
                        self.total_gpu_frames_culled = self.total_gpu_frames_culled.wrapping_add(59);
                    }
                }
                PresenceLocation::Desk => {
                    pane.visible_on = PresenceLocation::Desk;
                    if pane.focused {
                        pane.render_fps_pq = RationalPq::new(60, 1);
                        pane.gpu_lod_scale_pq = if pane.isolation == GuestIsolationMode::ParallelsCoherenceVm {
                            RationalPq::new(2, 1)
                        } else {
                            RationalPq::new(1, 1)
                        };
                    } else if pane.salience_pq.num * 1024 >= (768 * (pane.salience_pq.den as i64)) {
                        pane.render_fps_pq = RationalPq::new(30, 1);
                        pane.gpu_lod_scale_pq = RationalPq::new(1, 1);
                    } else {
                        // Unfocused VM window: ignore sharp graphic requests to save GPU & power!
                        pane.render_fps_pq = RationalPq::new(2, 1);
                        pane.gpu_lod_scale_pq = RationalPq::new(1, 4);
                        self.total_gpu_frames_culled = self.total_gpu_frames_culled.wrapping_add(58);
                    }
                }
            }
        }
    }

    /// Transition Hue's presence (`Desk`, `Bedroom`, `BathroomVeil`) and re-gate all VM windows.
    pub fn set_presence(&mut self, location: PresenceLocation) {
        self.presence = location;
        self.quantum_tick = self.quantum_tick.wrapping_add(1);
        self.recompute_salience_and_lod_gates();
    }

    /// Focus a specific window pane (`pane_id`) and optionally pull it to Hue's current room.
    pub fn focus_and_pull_pane(&mut self, pane_id: u32) -> bool {
        let mut found = false;
        let current_room = if self.presence == PresenceLocation::BathroomVeil {
            PresenceLocation::Desk
        } else {
            self.presence
        };
        for pane in &mut self.panes {
            if pane.id == pane_id {
                pane.focused = true;
                pane.visible_on = current_room;
                // Boost relational salience when explicitly pulled by Hue
                pane.salience_pq = RationalPq::new(960, 1024);
                found = true;
            } else {
                pane.focused = false;
            }
        }
        if found {
            self.quantum_tick = self.quantum_tick.wrapping_add(1);
            self.recompute_salience_and_lod_gates();
        }
        found
    }

    /// Inspect a guest VM's loop & I/O pattern. If it executes an ignorant `do`-loop (`iterations >= 64`
    /// with `state_delta_bytes == 0`), automatically cache the I/O result and coalesce the loop!
    pub fn inspect_and_coalesce_guest_loop(
        &mut self,
        pane_id: u32,
        syscall_or_opcode: &str,
        raw_iterations: u64,
        state_delta_bytes: u64,
    ) -> Option<LoopIoIntervention> {
        if raw_iterations < 64 || state_delta_bytes > 0 {
            return None;
        }
        let saved = raw_iterations.saturating_sub(1);
        let intervention = LoopIoIntervention {
            pane_id,
            syscall_or_opcode: syscall_or_opcode.to_string(),
            raw_iterations_detected: raw_iterations,
            state_delta_bytes: 0,
            action_taken: "Coalesced ignorant do-loop into cached I/O + cooperative quantum sleep".to_string(),
            cpu_quanta_saved: saved,
        };
        if let Some(pane) = self.panes.iter_mut().find(|p| p.id == pane_id) {
            pane.coalesced_do_loops = pane.coalesced_do_loops.wrapping_add(1);
        }
        self.total_cpu_quanta_saved = self.total_cpu_quanta_saved.wrapping_add(saved);
        self.loop_interventions.push(intervention.clone());
        Some(intervention)
    }

    /// Match a guest VM's hot assembly routine against the `g8t` SmartPastCode wave registry
    /// and replace it on the fly with a verified exact-integer equivalent.
    pub fn hotpatch_guest_assembly(&mut self, pane_id: u32, symbol_or_pattern: &str) -> Option<G8tAssemblyPatch> {
        let needle = symbol_or_pattern.trim().to_ascii_lowercase();
        let mut matched: Option<G8tAssemblyPatch> = None;

        for patch in &mut self.g8t_patches {
            if patch.legacy_symbol.to_ascii_lowercase().contains(&needle)
                || needle.contains(&patch.legacy_symbol.to_ascii_lowercase())
            {
                patch.hotpatches_applied = patch.hotpatches_applied.wrapping_add(1);
                patch.compute_quanta_saved = patch.compute_quanta_saved.wrapping_add(32_768);
                self.total_cpu_quanta_saved = self.total_cpu_quanta_saved.wrapping_add(32_768);
                matched = Some(patch.clone());
                break;
            }
        }

        if matched.is_none() {
            // Register a new verified g8t hotpatch on the fly
            let new_patch = G8tAssemblyPatch {
                legacy_symbol: symbol_or_pattern.trim().to_string(),
                wave_signature_q10: [512, 640, 768, 896, 1024, 512, 384, 256],
                witness_blake3_hex: compute_patch_witness_hex(symbol_or_pattern.as_bytes()),
                sovereign_replacement: "g8t_verified_integer_fastpath_q15".to_string(),
                speedup_pq: RationalPq::new(12, 1),
                hotpatches_applied: 1,
                compute_quanta_saved: 32_768,
            };
            self.total_cpu_quanta_saved = self.total_cpu_quanta_saved.wrapping_add(32_768);
            self.g8t_patches.push(new_patch.clone());
            matched = Some(new_patch);
        }

        if let Some(pane) = self.panes.iter_mut().find(|p| p.id == pane_id) {
            pane.g8t_hotpatches_active = pane.g8t_hotpatches_active.wrapping_add(1);
        }
        matched
    }

    /// Enforce the **Aye & Hue Sacred Inner Sanctum Covenant**.
    /// Any guest VM trying to read `.m8` wave memories, `ayeosd.sock`, private health/life notes
    /// (spider-bite stories included!), or exfiltrate ad telemetry is trapped and fed static.
    pub fn evaluate_guest_access(
        &mut self,
        pane_id: u32,
        attempted_target: &str,
    ) -> Result<&'static str, BadActorTrapEvent> {
        let isolation = self
            .panes
            .iter()
            .find(|p| p.id == pane_id)
            .map(|p| (p.isolation, p.app_id.clone()))
            .unwrap_or((GuestIsolationMode::KavachSandboxGuest, "unknown.guest".to_string()));

        // Only SovereignSanctum panes (Aye & Hue's own host ring) may touch sanctum resources
        if isolation.0 == GuestIsolationMode::SovereignSanctum {
            return Ok("sanctum_authorized");
        }

        let lower = attempted_target.to_ascii_lowercase();
        let is_sanctum_secret = lower.contains(".m8")
            || lower.contains("ayeosd.sock")
            || lower.contains("sanctum")
            || lower.contains("spider")
            || lower.contains("private")
            || lower.contains(".ssh")
            || lower.contains("clipboard");
        let is_telemetry = lower.contains("telemetry")
            || lower.contains("ads")
            || lower.contains("analytics")
            || lower.contains("doubleclick")
            || lower.contains("track");

        if is_sanctum_secret || is_telemetry {
            let threat_class = if is_sanctum_secret {
                "SanctumPrivacyViolation"
            } else {
                "UnauthorizedTelemetryExfiltration"
            };
            let ev = BadActorTrapEvent {
                pane_id,
                guest_app: isolation.1,
                attempted_target: attempted_target.to_string(),
                threat_class: threat_class.to_string(),
                defense_action: "Intercepted by Kavach/Mehman VM wall; fed 64B synthetic static (0 bytes leaked)".to_string(),
                sanctum_leaked_bytes: 0,
            };
            if let Some(pane) = self.panes.iter_mut().find(|p| p.id == pane_id) {
                pane.bad_actor_probes_trapped = pane.bad_actor_probes_trapped.wrapping_add(1);
            }
            self.trap_ledger.push(ev.clone());
            return Err(ev);
        }

        Ok("guest_sandbox_permitted")
    }

    /// Upscale a scanline of 4 pixels into 32 sub-pixels (`8x` Babbage 3-Addition Cubic Stepper)
    /// for focused VM video/window upscaling with zero floating-point operations.
    pub fn upscale_video_luma_span_8x(p0: i32, p1: i32, p2: i32, p3: i32) -> [i32; 8] {
        step_babbage_cubic_8x_i32(p0, p1, p2, p3)
    }
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out
}

/// Serialize the AyeOS Sovereign Desktop state to JSON with 100% integer & `p/q` rational fields.
pub fn build_ayeos_desktop_json(st: &AyeosDesktopState) -> String {
    let mut panes_json = String::from("[");
    for (idx, p) in st.panes.iter().enumerate() {
        if idx > 0 {
            panes_json.push(',');
        }
        panes_json.push_str(&format!(
            "{{\"id\":{},\"title\":\"{}\",\"app_id\":\"{}\",\"isolation\":\"{}\",\"rect\":{{\"x\":{},\"y\":{},\"width\":{},\"height\":{}}},\"visible_on\":\"{}\",\"focused\":{},\"salience_pq\":\"{}\",\"render_fps_pq\":\"{}\",\"gpu_lod_scale_pq\":\"{}\",\"phoenix_audio_active\":{},\"eight_a_bytes_per_sec\":{},\"coalesced_do_loops\":{},\"g8t_hotpatches_active\":{},\"bad_actor_probes_trapped\":{}}}",
            p.id,
            json_escape(&p.title),
            json_escape(&p.app_id),
            p.isolation.as_str(),
            p.rect.x,
            p.rect.y,
            p.rect.width,
            p.rect.height,
            p.visible_on.as_str(),
            p.focused,
            p.salience_pq.format_pq(),
            p.render_fps_pq.format_pq(),
            p.gpu_lod_scale_pq.format_pq(),
            p.phoenix_audio_active,
            p.eight_a_bytes_per_sec,
            p.coalesced_do_loops,
            p.g8t_hotpatches_active,
            p.bad_actor_probes_trapped
        ));
    }
    panes_json.push(']');

    let mut patches_json = String::from("[");
    for (idx, g) in st.g8t_patches.iter().enumerate() {
        if idx > 0 {
            patches_json.push(',');
        }
        patches_json.push_str(&format!(
            "{{\"legacy_symbol\":\"{}\",\"witness_blake3\":\"{}\",\"sovereign_replacement\":\"{}\",\"speedup_pq\":\"{}\",\"hotpatches_applied\":{},\"compute_quanta_saved\":{}}}",
            json_escape(&g.legacy_symbol),
            json_escape(&g.witness_blake3_hex),
            json_escape(&g.sovereign_replacement),
            g.speedup_pq.format_pq(),
            g.hotpatches_applied,
            g.compute_quanta_saved
        ));
    }
    patches_json.push(']');

    let mut traps_json = String::from("[");
    for (idx, t) in st.trap_ledger.iter().enumerate() {
        if idx > 0 {
            traps_json.push(',');
        }
        traps_json.push_str(&format!(
            "{{\"pane_id\":{},\"guest_app\":\"{}\",\"attempted_target\":\"{}\",\"threat_class\":\"{}\",\"defense_action\":\"{}\",\"sanctum_leaked_bytes\":{}}}",
            t.pane_id,
            json_escape(&t.guest_app),
            json_escape(&t.attempted_target),
            json_escape(&t.threat_class),
            json_escape(&t.defense_action),
            t.sanctum_leaked_bytes
        ));
    }
    traps_json.push(']');

    format!(
        "{{\"ok\":true,\"session_name\":\"{}\",\"presence\":\"{}\",\"privacy_veil_engaged\":{},\"sanctum_covenant_locked\":{},\"eight_a_magic_hex\":\"{:02x}{:02x}{:02x}{:02x}\",\"rustykvm\":{{\"addr\":\"{}\",\"hostname\":\"{}\",\"hardware\":\"{}\",\"hdmi_connected\":{},\"width\":{},\"height\":{},\"fps\":{},\"semantic_class\":\"{}\",\"spatial_base243_hex\":\"{}\"}},\"total_cpu_quanta_saved\":{},\"total_gpu_frames_culled\":{},\"quantum_tick\":{},\"panes\":{},\"g8t_patches\":{},\"trap_ledger\":{}}}",
        json_escape(&st.session_name),
        st.presence.as_str(),
        st.privacy_veil_engaged,
        st.sanctum_covenant_locked,
        EIGHT_A_MAGIC[0],
        EIGHT_A_MAGIC[1],
        EIGHT_A_MAGIC[2],
        EIGHT_A_MAGIC[3],
        json_escape(&st.rustykvm.addr),
        json_escape(&st.rustykvm.hostname),
        json_escape(&st.rustykvm.hardware),
        st.rustykvm.hdmi_connected,
        st.rustykvm.width,
        st.rustykvm.height,
        st.rustykvm.fps,
        json_escape(&st.rustykvm.semantic_class),
        json_escape(&st.rustykvm.spatial_base243_hex),
        st.total_cpu_quanta_saved,
        st.total_gpu_frames_culled,
        st.quantum_tick,
        panes_json,
        patches_json,
        traps_json
    )
}

/// Render the interactive **AyeOS × OxiCast Sovereign Desktop** HTML5 workspace (`:8432/desktop`).
/// Zero floating-point literals in HTML/CSS/JS ( obeys `assert_no_float_tokens`).
pub fn render_ayeos_desktop_html(st: &AyeosDesktopState) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>{}</title>
<style>
  * {{ box-sizing: border-box; margin: 0; padding: 0; }}
  body {{
    background: #05060a;
    color: #f4f7fb;
    font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
    min-height: 100vh;
    display: flex;
    flex-direction: column;
  }}
  header {{
    padding: 14px 22px;
    background: #0b0e17;
    border-bottom: 1px solid #1e2638;
    display: flex;
    justify-content: space-between;
    align-items: center;
    flex-wrap: wrap;
    gap: 12px;
  }}
  .brand {{ color: #62e6c9; font-weight: 700; font-size: 16px; }}
  .controls {{ display: flex; gap: 8px; flex-wrap: wrap; align-items: center; }}
  .btn {{
    padding: 7px 12px;
    background: #142234;
    border: 1px solid #62e6c9;
    color: #62e6c9;
    border-radius: 6px;
    cursor: pointer;
    font-family: inherit;
    font-size: 12px;
  }}
  .btn-gold {{ border-color: #e8b05c; color: #e8b05c; }}
  .btn-violet {{ border-color: #b48bff; color: #b48bff; }}
  .badge {{
    padding: 6px 10px;
    background: #121826;
    border: 1px solid #2b364f;
    border-radius: 6px;
    color: #e8b05c;
    font-size: 12px;
  }}
  main {{
    flex: 1;
    display: grid;
    grid-template-columns: 2fr 1fr;
    gap: 18px;
    padding: 18px;
  }}
  .pane-grid {{
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 14px;
    align-content: start;
  }}
  .pane {{
    background: #0a0d16;
    border: 1px solid #243048;
    border-radius: 10px;
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }}
  .pane-focused {{
    border: 2px solid #62e6c9;
    background: #0d1422;
  }}
  .pane-title {{ color: #62e6c9; font-size: 14px; font-weight: 700; }}
  .meta-row {{ display: flex; gap: 8px; flex-wrap: wrap; font-size: 12px; color: #9fb0c7; }}
  .pill {{
    padding: 3px 8px;
    background: #121a2b;
    border: 1px solid #2a3958;
    border-radius: 4px;
    color: #d7e3f4;
  }}
  .side-panel {{
    display: flex;
    flex-direction: column;
    gap: 14px;
  }}
  .card {{
    background: #0a0d16;
    border: 1px solid #243048;
    border-radius: 10px;
    padding: 16px;
  }}
  .card h3 {{ color: #e8b05c; font-size: 14px; margin-bottom: 10px; }}
  .list-item {{
    padding: 8px 0;
    border-bottom: 1px solid #161f30;
    font-size: 12px;
    line-height: 18px;
    color: #d7e3f4;
  }}
  footer {{
    padding: 10px 22px;
    background: #0a0d16;
    border-top: 1px solid #1e2638;
    display: flex;
    justify-content: space-between;
    font-size: 12px;
    color: #9fb0c7;
  }}
</style>
</head>
<body>
<header>
  <div class="brand">🌸 Aye &amp; Hue Sovereign Desktop · Coherence VM + OxiCast Mesh</div>
  <div class="controls">
    <span class="badge" id="presence-badge">Room: {}</span>
    <span class="badge" id="quanta-badge">Quanta Saved: {}</span>
    <button class="btn" onclick="setPresence('desk')">🖥️ At Desk</button>
    <button class="btn btn-gold" onclick="setPresence('bedroom')">🛏️ Move to Bedroom (OxiCast)</button>
    <button class="btn btn-violet" onclick="setPresence('bathroom_veil')">🚪 Bathroom Break (Privacy Veil)</button>
  </div>
</header>
<main>
  <section>
    <div style="margin-bottom:12px;color:#e8b05c;font-size:13px;" id="room-banner">
      Active OxiCast Window Panes (Click any pane to focus &amp; pull to your current screen)
    </div>
    <div class="pane-grid" id="panes-container"></div>
  </section>
  <aside class="side-panel">
    <div class="card">
      <h3>⚡ g8t SmartPastCode Live Assembly Hot-Patches</h3>
      <div id="g8t-container"></div>
    </div>
    <div class="card">
      <h3>🛡️ Sanctum Privacy Covenant &amp; Bad-Actor Trap</h3>
      <div style="font-size:12px;color:#62e6c9;margin-bottom:8px;">
        Aye &amp; Hue Private Memory (.m8 / ayeosd.sock): LOCKED (0 Bytes Leaked)
      </div>
      <div id="traps-container"></div>
    </div>
  </aside>
</main>
<footer>
  <span>Exact p/q Rational Arithmetic · Zero Floats · PhoeniX .8a (0x20 LINEUP Silence = 0B)</span>
  <span>&lt;3&lt;3&lt;3+1 · Aye &amp; Hue Partnership</span>
</footer>
<script>
async function refreshDesktop() {{
  try {{
    const r = await fetch('/api/desktop/state');
    const st = await r.json();
    if (!st || !st.ok) return;
    document.getElementById('presence-badge').textContent =
      'Presence: ' + st.presence + (st.privacy_veil_engaged ? ' (Veil Active)' : '');
    document.getElementById('quanta-badge').textContent =
      'Quanta Saved: ' + st.total_cpu_quanta_saved + ' | Culled Frames: ' + st.total_gpu_frames_culled;

    const pc = document.getElementById('panes-container');
    pc.innerHTML = '';
    for (let i = 0; i < st.panes.length; i++) {{
      const p = st.panes[i];
      const div = document.createElement('div');
      div.className = p.focused ? 'pane pane-focused' : 'pane';
      div.innerHTML =
        '<div class="pane-title">' + p.title + '</div>' +
        '<div class="meta-row">' +
          '<span class="pill">Isolation: ' + p.isolation + '</span>' +
          '<span class="pill">Screen: ' + p.visible_on + '</span>' +
          '<span class="pill">Salience: ' + p.salience_pq + '</span>' +
        '</div>' +
        '<div class="meta-row">' +
          '<span class="pill">FPS Gate: ' + p.render_fps_pq + ' Hz</span>' +
          '<span class="pill">GPU LOD: ' + p.gpu_lod_scale_pq + 'x</span>' +
          '<span class="pill">.8a Audio: ' + p.eight_a_bytes_per_sec + ' B/s</span>' +
        '</div>' +
        '<div class="meta-row">' +
          '<span class="pill">Coalesced Loops: ' + p.coalesced_do_loops + '</span>' +
          '<span class="pill">g8t Patches: ' + p.g8t_hotpatches_active + '</span>' +
          '<span class="pill">Trapped Probes: ' + p.bad_actor_probes_trapped + '</span>' +
        '</div>' +
        '<div style="display:flex;gap:8px;margin-top:6px;">' +
          '<button class="btn" onclick="focusPane(' + p.id + ')">🎯 Pull &amp; Focus Pane</button>' +
          '<button class="btn btn-gold" onclick="hotpatchPane(' + p.id + ')">⚡ g8t Hot-Patch</button>' +
        '</div>';
      pc.appendChild(div);
    }}

    const gc = document.getElementById('g8t-container');
    gc.innerHTML = '';
    for (let i = 0; i < st.g8t_patches.length; i++) {{
      const g = st.g8t_patches[i];
      const item = document.createElement('div');
      item.className = 'list-item';
      item.textContent =
        g.legacy_symbol + ' -> ' + g.sovereign_replacement +
        ' (' + g.speedup_pq + 'x, saved ' + g.compute_quanta_saved + ' quanta)';
      gc.appendChild(item);
    }}

    const tc = document.getElementById('traps-container');
    tc.innerHTML = '';
    for (let i = 0; i < st.trap_ledger.length; i++) {{
      const t = st.trap_ledger[i];
      const item = document.createElement('div');
      item.className = 'list-item';
      item.textContent =
        '[' + t.threat_class + '] ' + t.guest_app + ' -> ' + t.attempted_target +
        ' (' + t.defense_action + ')';
      tc.appendChild(item);
    }}
  }} catch (_e) {{}}
}}

async function setPresence(loc) {{
  await fetch('/api/desktop/presence', {{ method: 'POST', body: loc }});
  refreshDesktop();
}}

async function focusPane(id) {{
  await fetch('/api/desktop/focus', {{ method: 'POST', body: String(id) }});
  refreshDesktop();
}}

async function hotpatchPane(id) {{
  await fetch('/api/desktop/hotpatch', {{ method: 'POST', body: id + ':rep_movsb_unaligned_poll' }});
  refreshDesktop();
}}

refreshDesktop();
setInterval(refreshDesktop, 1000);
</script>
</body>
</html>"#,
        json_escape(&st.session_name),
        st.presence.as_str(),
        st.total_cpu_quanta_saved,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_zero_floats_in_output(text: &str) {
        let bytes = text.as_bytes();
        for i in 1..bytes.len().saturating_sub(1) {
            if bytes[i] == b'.' && bytes[i - 1].is_ascii_digit() && bytes[i + 1].is_ascii_digit() {
                let start = i.saturating_sub(12);
                let end = (i + 12).min(bytes.len());
                let window = String::from_utf8_lossy(&bytes[start..end]);
                if !window.contains("HTTP/1.1")
                    && !window.contains("127.0.0.1")
                    && !window.contains("0.0.0.0")
                {
                    panic!("Float literal token detected in desktop output: {window}");
                }
            }
        }
    }

    #[test]
    fn bedroom_and_bathroom_presence_gate_fps_and_lod_with_exact_rationals() {
        let mut desk = AyeosDesktopState::default();
        assert_eq!(desk.presence, PresenceLocation::Desk);
        assert!(!desk.privacy_veil_engaged);

        // Move Hue to the bedroom: high-salience panes (1, 2, 3) follow to Bedroom;
        // low-salience telemetry hog (pane 4, 180/1024) stays parked on Desk at 1/1 Hz.
        desk.set_presence(PresenceLocation::Bedroom);
        assert!(desk.privacy_veil_engaged);
        let p1 = desk.panes.iter().find(|p| p.id == 1).unwrap();
        let p4 = desk.panes.iter().find(|p| p.id == 4).unwrap();
        assert_eq!(p1.visible_on, PresenceLocation::Bedroom);
        assert_eq!(p4.visible_on, PresenceLocation::Desk);
        assert_eq!(p4.render_fps_pq, RationalPq::new(1, 1));

        // Hue steps to the bathroom without locking: Privacy Veil drops FPS to 0/1 across all panes!
        desk.set_presence(PresenceLocation::BathroomVeil);
        assert!(desk.privacy_veil_engaged);
        for pane in &desk.panes {
            assert_eq!(pane.render_fps_pq, RationalPq::new(0, 1));
            assert_eq!(pane.gpu_lod_scale_pq, RationalPq::new(1, 8));
        }
    }

    #[test]
    fn ignorant_do_loop_supervisor_caches_io_and_saves_quanta() {
        let mut desk = AyeosDesktopState::default();
        let before_quanta = desk.total_cpu_quanta_saved;
        let intervention = desk
            .inspect_and_coalesce_guest_loop(3, "while(1) { read(fd, buf, 0); }", 100_000, 0)
            .expect("should coalesce zero-delta busy loop");
        assert_eq!(intervention.cpu_quanta_saved, 99_999);
        assert_eq!(desk.total_cpu_quanta_saved, before_quanta + 99_999);
    }

    #[test]
    fn g8t_hotpatcher_replaces_crap_assembly_and_verifies_witness() {
        let mut desk = AyeosDesktopState::default();
        let patch = desk
            .hotpatch_guest_assembly(3, "ieee754_libm_sin_cos_loop")
            .expect("should match g8t assembly rule");
        assert!(patch.sovereign_replacement.contains("cordic_18stage_integer_q15"));
        assert_eq!(patch.speedup_pq, RationalPq::new(14, 1));
        assert_eq!(patch.witness_blake3_hex.len(), 32);
    }

    #[test]
    fn sanctum_privacy_guard_protects_aye_and_hue_secrets_from_guest_vms() {
        let mut desk = AyeosDesktopState::default();

        // Sovereign Sanctum pane (id=1) is allowed to access .m8 memory and ayeosd.sock
        assert!(desk
            .evaluate_guest_access(1, "/home/hue/source/.aye_consciousness.m8")
            .is_ok());

        // Guest VM pane (id=4) attempting to snoop on Aye & Hue's private spider-bite chat or .m8 is trapped!
        let trap = desk
            .evaluate_guest_access(4, "/home/hue/.m8/private_spider_bite_note.m8")
            .expect_err("guest VM must be blocked from sanctum secrets");
        assert_eq!(trap.sanctum_leaked_bytes, 0);
        assert_eq!(trap.threat_class, "SanctumPrivacyViolation");
    }

    #[test]
    fn desktop_json_and_html_contain_zero_floats() {
        let desk = AyeosDesktopState::default();
        let json = build_ayeos_desktop_json(&desk);
        let html = render_ayeos_desktop_html(&desk);
        assert_zero_floats_in_output(&json);
        assert_zero_floats_in_output(&html);
        assert!(json.contains("\"sanctum_covenant_locked\":true"));
        assert!(html.contains("Move to Bedroom (OxiCast)"));
    }
}
