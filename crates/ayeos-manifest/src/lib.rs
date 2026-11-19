//! `ayeos-manifest` — Common inspectable capsule manifest with distinct permissions,
//! lifecycles, anti-downgrade signature policies, and hard admission/isolation gates.
//!
//! Architectural invariants:
//! 1. Small headers & narrow interfaces — NO fixed memory sizes and NO raw register ABI.
//! 2. `g8t` competes on pure functions and nominates candidates (`authorizes_device_install == false`);
//!    AyeOS local policy alone authorizes what gets to run.
//! 3. `Function`, `App`, and `Driver` share one manifest envelope but have strictly separate
//!    permission profiles and lifecycles.
//! 4. Ed25519 signatures are never replaced by BLAKE3 digests; dual-signed capsules are never
//!    silently downgraded to an Ed25519-only local demo policy.
//! 5. Live driver upgrades and bare-metal dynamic capsule activation stay explicitly gated on
//!    unmet hardware/kernel isolation & drain prerequisites.

#![no_std]
#![deny(clippy::float_arithmetic)]

extern crate alloc;
#[cfg(feature = "std")]
extern crate std;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;

#[cfg(feature = "std")]
use serde::{Deserialize, Serialize};

/// Canonical domain prefix signed by off-device Ed25519 keys for capsule manifests.
pub const CAPSULE_SIG_DOMAIN_V1: &[u8] = b"AYEOS_CAPSULE_MANIFEST_V1\0";

/// Three capsule classes with distinct permissions and lifecycles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "std", serde(rename_all = "snake_case"))]
pub enum CapsuleKind {
    Function,
    App,
    Driver,
}

/// Narrow, typed interface contract declared by a capsule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "std", serde(tag = "kind", rename_all = "snake_case"))]
pub enum NarrowInterface {
    /// Pure deterministic integer function: `extern "C" fn(i64, i64) -> i64`.
    PureFnI64x2ToI64,
    /// Local read/control inspector UI & IPC surface.
    InspectorSurfaceV1,
    /// Boot-pinned PCI network device (read-only inspection).
    BootPinnedPciNet { vendor_id: u16, device_id: u16 },
}

/// Permissions for pure deterministic function capsules (`no_fs`, `no_net`, `no_dma`, `no_sanctum`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
pub struct FunctionPermissions {
    pub pure_compute_only: bool,
    pub allow_fs: bool,
    pub allow_net: bool,
    pub allow_dma: bool,
    pub allow_sanctum_m8: bool,
    /// Parent-enforced wall-clock deadline per invocation (milliseconds).
    pub wall_deadline_ms: u32,
}

impl FunctionPermissions {
    pub const fn strict_default(wall_deadline_ms: u32) -> Self {
        Self {
            pure_compute_only: true,
            allow_fs: false,
            allow_net: false,
            allow_dma: false,
            allow_sanctum_m8: false,
            wall_deadline_ms,
        }
    }

    pub const fn is_pure_and_isolated(&self) -> bool {
        self.pure_compute_only
            && !self.allow_fs
            && !self.allow_net
            && !self.allow_dma
            && !self.allow_sanctum_m8
            && self.wall_deadline_ms > 0
    }
}

/// Permissions for isolated micro-apps (narrow UI/IPC surface; never raw DMA or Sanctum `.m8`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
pub struct AppPermissions {
    pub ui_surface: bool,
    pub local_ipc: bool,
    pub allow_net: bool,
    pub allow_dma: bool,
    pub allow_sanctum_m8: bool,
}

impl AppPermissions {
    pub const fn inspector_local() -> Self {
        Self {
            ui_surface: true,
            local_ipc: true,
            allow_net: false,
            allow_dma: false,
            allow_sanctum_m8: false,
        }
    }

    pub const fn respects_sanctum_boundary(&self) -> bool {
        !self.allow_dma && !self.allow_sanctum_m8
    }
}

/// Permissions for boot-pinned hardware drivers (read-only in Inspector; live upgrades locked).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
pub struct DriverPermissions {
    pub pci_vendor_id: u16,
    pub pci_device_id: u16,
    pub boot_pinned_only: bool,
    pub live_upgrade_allowed: bool,
}

impl DriverPermissions {
    pub const fn boot_pinned_virtio_net() -> Self {
        Self {
            pci_vendor_id: 0x1AF4,
            pci_device_id: 0x1000,
            boot_pinned_only: true,
            live_upgrade_allowed: false,
        }
    }
}

/// Kind-specific permission profile inside the common manifest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "std", serde(tag = "profile", rename_all = "snake_case"))]
pub enum CapsulePermissions {
    Function(FunctionPermissions),
    App(AppPermissions),
    Driver(DriverPermissions),
}

/// Lifecycle for hot-swappable pure function capsules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "std", serde(rename_all = "snake_case"))]
pub enum FunctionLifecycle {
    BaselineRust,
    NominatedCandidate,
    StagedLocal,
    Authorized,
    ActiveSignedCapsule,
    RolledBackToBaseline,
    AutoRolledBackOnFault,
}

/// Lifecycle for isolated apps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "std", serde(rename_all = "snake_case"))]
pub enum AppLifecycle {
    Installed,
    Admitted,
    Running,
    Suspended,
    Terminated,
}

/// Lifecycle for boot-pinned hardware drivers (read-only until drain/revoke/recovery exist).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "std", serde(rename_all = "snake_case"))]
pub enum DriverLifecycle {
    BootPinnedReadOnly,
    Absent,
}

/// Kind-specific lifecycle state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "std", serde(tag = "lifecycle_kind", content = "state", rename_all = "snake_case"))]
pub enum CapsuleLifecycle {
    Function(FunctionLifecycle),
    App(AppLifecycle),
    Driver(DriverLifecycle),
}

/// Signature requirement declared by the capsule manifest itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "std", serde(rename_all = "snake_case"))]
pub enum SignatureRequirement {
    /// Local Inspector demo capsules signed with an off-device Ed25519 key.
    Ed25519Only,
    /// Dual-signed `g8t` / hybrid capsules requiring both signatures (never silently downgraded).
    DualSignedRequired,
}

/// Active verifier signature policy configured in AyeOS.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "std", serde(rename_all = "snake_case"))]
pub enum SignaturePolicy {
    /// Explicit policy for the manual local Inspector demo: verifies Ed25519-only demo capsules
    /// and HARD-REJECTS any capsule marked `DualSignedRequired` so dual-signed requirements can
    /// never be silently downgraded.
    Ed25519LocalInspectorDemo,
    /// Strict dual-signature policy requiring both Ed25519 and secondary signature verification.
    DualSignedStrict,
}

/// Advisory candidate nomination from `g8t.is` — NEVER authorizes device installation.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
pub struct G8tNomination {
    pub g8t_capsule_id: String,
    pub leaderboard_rank: u32,
    pub net_votes: i64,
    pub reported_cycles: u64,
    /// Always `false`: a leaderboard win nominates a candidate, never authorizes a device.
    pub authorizes_device_install: bool,
}

impl G8tNomination {
    pub fn advisory(
        g8t_capsule_id: impl Into<String>,
        leaderboard_rank: u32,
        net_votes: i64,
        reported_cycles: u64,
    ) -> Self {
        Self {
            g8t_capsule_id: g8t_capsule_id.into(),
            leaderboard_rank,
            net_votes,
            reported_cycles,
            authorizes_device_install: false,
        }
    }
}

/// Device-local AyeOS authorization decision (completely separate from `g8t` nomination).
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "std", serde(tag = "state", rename_all = "snake_case"))]
pub enum DeviceAuthorization {
    Unreviewed,
    /// Candidate was nominated (e.g. from `g8t`); CANNOT activate without local device approval.
    NominatedOnly,
    /// Explicitly authorized locally on this AyeOS device for a specific verified BLAKE3 digest.
    LocallyAuthorized {
        authorized_by: String,
        epoch_s: u64,
        artifact_blake3_hex: String,
    },
    /// Static boot-pinned hardware driver (read-only).
    BootPinned,
    Revoked {
        reason: String,
    },
}

/// Small, inspectable common capsule manifest shared across `Function`, `App`, and `Driver`.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
pub struct CapsuleManifest {
    pub id: String,
    pub version: u32,
    pub kind: CapsuleKind,
    pub target_arch: String,
    pub interface: NarrowInterface,
    pub permissions: CapsulePermissions,
    pub lifecycle: CapsuleLifecycle,
    pub signature_requirement: SignatureRequirement,
    pub zero_float_declared: bool,
    /// BLAKE3 hex digest of the executable artifact bytes (strictly a digest, NOT a signature).
    pub artifact_blake3_hex: String,
    /// Off-device Ed25519 signature (128 hex chars = 64 bytes) over `canonical_signing_bytes()`.
    pub ed25519_signature_hex: String,
    /// Optional secondary signature hex for dual-signed `g8t` capsules.
    pub secondary_signature_hex: Option<String>,
    /// Optional advisory nomination from `g8t.is`.
    pub nomination: Option<G8tNomination>,
    /// AyeOS local device authorization state.
    pub authorization: DeviceAuthorization,
}

impl CapsuleManifest {
    /// Deterministic byte payload signed by the off-device Ed25519 signing key.
    /// Binds manifest identity, kind, interface, permission profile, signature requirement,
    /// and the BLAKE3 artifact digest together.
    pub fn canonical_signing_bytes(&self) -> Vec<u8> {
        let kind_str = match self.kind {
            CapsuleKind::Function => "function",
            CapsuleKind::App => "app",
            CapsuleKind::Driver => "driver",
        };
        let req_str = match self.signature_requirement {
            SignatureRequirement::Ed25519Only => "ed25519_only",
            SignatureRequirement::DualSignedRequired => "dual_signed_required",
        };
        let iface_str = match self.interface {
            NarrowInterface::PureFnI64x2ToI64 => "pure_fn_i64x2_to_i64".to_string(),
            NarrowInterface::InspectorSurfaceV1 => "inspector_surface_v1".to_string(),
            NarrowInterface::BootPinnedPciNet {
                vendor_id,
                device_id,
            } => alloc::format!("boot_pinned_pci_net:{:04x}:{:04x}", vendor_id, device_id),
        };
        let mut out = Vec::new();
        out.extend_from_slice(CAPSULE_SIG_DOMAIN_V1);
        out.extend_from_slice(
            alloc::format!(
                "id={}\nver={}\nkind={}\narch={}\niface={}\nsig_req={}\nzero_float={}\nblake3={}\n",
                self.id,
                self.version,
                kind_str,
                self.target_arch,
                iface_str,
                req_str,
                self.zero_float_declared,
                self.artifact_blake3_hex
            )
            .as_bytes(),
        );
        out
    }

    /// Validate that kind, permissions, and lifecycle agree (prevents privilege confusion).
    pub fn validate_shape(&self) -> Result<(), PolicyError> {
        match (self.kind, self.permissions, self.lifecycle) {
            (
                CapsuleKind::Function,
                CapsulePermissions::Function(p),
                CapsuleLifecycle::Function(_),
            ) => {
                if !p.is_pure_and_isolated() {
                    return Err(PolicyError::PermissionViolation(
                        "Function capsules must be pure_compute_only with zero fs/net/dma/sanctum access",
                    ));
                }
                Ok(())
            }
            (CapsuleKind::App, CapsulePermissions::App(p), CapsuleLifecycle::App(_)) => {
                if !p.respects_sanctum_boundary() {
                    return Err(PolicyError::PermissionViolation(
                        "App capsules cannot request raw DMA or Sanctum .m8 access",
                    ));
                }
                Ok(())
            }
            (
                CapsuleKind::Driver,
                CapsulePermissions::Driver(p),
                CapsuleLifecycle::Driver(_),
            ) => {
                if !p.boot_pinned_only || p.live_upgrade_allowed {
                    return Err(PolicyError::PermissionViolation(
                        "Drivers must remain boot_pinned_only until live upgrade prerequisites are met",
                    ));
                }
                Ok(())
            }
            _ => Err(PolicyError::KindMismatch),
        }
    }
}

/// Hard prerequisite gate for live driver upgrades (currently all false -> blocked).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
pub struct DriverLiveUpgradeGate {
    pub io_drain_supported: bool,
    pub dma_irq_revocation_supported: bool,
    pub independent_recovery_supported: bool,
}

impl DriverLiveUpgradeGate {
    pub const fn current() -> Self {
        Self {
            io_drain_supported: false,
            dma_irq_revocation_supported: false,
            independent_recovery_supported: false,
        }
    }

    pub fn check_live_upgrade(&self) -> Result<(), PolicyError> {
        if self.io_drain_supported
            && self.dma_irq_revocation_supported
            && self.independent_recovery_supported
        {
            Ok(())
        } else {
            Err(PolicyError::DriverLiveUpgradeBlocked {
                io_drain: self.io_drain_supported,
                dma_irq_revocation: self.dma_irq_revocation_supported,
                independent_recovery: self.independent_recovery_supported,
            })
        }
    }
}

/// Hard prerequisite gate for dynamic bare-metal capsule activation (currently false -> gated).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
pub struct BareMetalActivationGate {
    pub mmu_isolation_enforced: bool,
    pub iommu_dma_isolation_enforced: bool,
}

impl BareMetalActivationGate {
    pub const fn current() -> Self {
        Self {
            mmu_isolation_enforced: false,
            iommu_dma_isolation_enforced: false,
        }
    }

    pub fn check_dynamic_activation(&self) -> Result<(), PolicyError> {
        if self.mmu_isolation_enforced && self.iommu_dma_isolation_enforced {
            Ok(())
        } else {
            Err(PolicyError::BareMetalActivationGated {
                mmu_isolation: self.mmu_isolation_enforced,
                iommu_dma_isolation: self.iommu_dma_isolation_enforced,
            })
        }
    }
}

/// Policy and admission rejection reasons.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PolicyError {
    KindMismatch,
    PermissionViolation(&'static str),
    /// Refuses silent downgrade of a dual-signed capsule under an Ed25519-only demo policy.
    PolicyDowngradeForbidden,
    /// Dual-signed strict policy requires a secondary signature.
    SecondarySignatureMissing,
    /// `g8t` leaderboard nominations can never authorize installation.
    NominationDoesNotAuthorizeInstall,
    /// Device authorization is missing, revoked, or bound to a different artifact digest.
    NotLocallyAuthorized,
    DriverLiveUpgradeBlocked {
        io_drain: bool,
        dma_irq_revocation: bool,
        independent_recovery: bool,
    },
    BareMetalActivationGated {
        mmu_isolation: bool,
        iommu_dma_isolation: bool,
    },
}

impl fmt::Display for PolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::KindMismatch => write!(f, "manifest kind/permissions/lifecycle mismatch"),
            Self::PermissionViolation(msg) => write!(f, "permission violation: {msg}"),
            Self::PolicyDowngradeForbidden => write!(
                f,
                "POLICY_DOWNGRADE_FORBIDDEN: dual-signed capsule cannot be verified under Ed25519-only demo policy"
            ),
            Self::SecondarySignatureMissing => {
                write!(f, "SECONDARY_SIGNATURE_MISSING: dual-signed policy requires secondary signature")
            }
            Self::NominationDoesNotAuthorizeInstall => write!(
                f,
                "NOMINATION_ONLY: a g8t leaderboard win nominates a candidate, never authorizes device install"
            ),
            Self::NotLocallyAuthorized => {
                write!(f, "NOT_LOCALLY_AUTHORIZED: AyeOS local owner authorization required")
            }
            Self::DriverLiveUpgradeBlocked {
                io_drain,
                dma_irq_revocation,
                independent_recovery,
            } => write!(
                f,
                "DRIVER_LIVE_UPGRADE_BLOCKED: io_drain={io_drain}, dma_irq_revocation={dma_irq_revocation}, independent_recovery={independent_recovery}"
            ),
            Self::BareMetalActivationGated {
                mmu_isolation,
                iommu_dma_isolation,
            } => write!(
                f,
                "BARE_METAL_ACTIVATION_GATED: mmu_isolation={mmu_isolation}, iommu_dma_isolation={iommu_dma_isolation}"
            ),
        }
    }
}

/// Enforce signature policy with explicit anti-downgrade protection.
pub fn check_signature_policy(
    policy: SignaturePolicy,
    manifest: &CapsuleManifest,
) -> Result<(), PolicyError> {
    match (policy, manifest.signature_requirement) {
        (SignaturePolicy::Ed25519LocalInspectorDemo, SignatureRequirement::Ed25519Only) => Ok(()),
        (SignaturePolicy::Ed25519LocalInspectorDemo, SignatureRequirement::DualSignedRequired) => {
            Err(PolicyError::PolicyDowngradeForbidden)
        }
        (SignaturePolicy::DualSignedStrict, SignatureRequirement::Ed25519Only) => {
            Err(PolicyError::SecondarySignatureMissing)
        }
        (SignaturePolicy::DualSignedStrict, SignatureRequirement::DualSignedRequired) => {
            if manifest
                .secondary_signature_hex
                .as_deref()
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false)
            {
                Ok(())
            } else {
                Err(PolicyError::SecondarySignatureMissing)
            }
        }
    }
}

/// Verify that a `Function` capsule manifest is admissible for sandboxed activation under `policy`.
pub fn check_function_activation_policy(
    policy: SignaturePolicy,
    manifest: &CapsuleManifest,
) -> Result<(), PolicyError> {
    if manifest.kind != CapsuleKind::Function {
        return Err(PolicyError::KindMismatch);
    }
    manifest.validate_shape()?;
    check_signature_policy(policy, manifest)?;

    if let Some(ref nom) = manifest.nomination {
        if nom.authorizes_device_install {
            return Err(PolicyError::NominationDoesNotAuthorizeInstall);
        }
    }

    match &manifest.authorization {
        DeviceAuthorization::NominatedOnly => Err(PolicyError::NominationDoesNotAuthorizeInstall),
        DeviceAuthorization::LocallyAuthorized {
            artifact_blake3_hex,
            ..
        } if *artifact_blake3_hex == manifest.artifact_blake3_hex => Ok(()),
        _ => Err(PolicyError::NotLocallyAuthorized),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_fn_manifest(req: SignatureRequirement, auth: DeviceAuthorization) -> CapsuleManifest {
        CapsuleManifest {
            id: "fn_harmonic_fold_v2".to_string(),
            version: 2,
            kind: CapsuleKind::Function,
            target_arch: "x86_64".to_string(),
            interface: NarrowInterface::PureFnI64x2ToI64,
            permissions: CapsulePermissions::Function(FunctionPermissions::strict_default(250)),
            lifecycle: CapsuleLifecycle::Function(FunctionLifecycle::StagedLocal),
            signature_requirement: req,
            zero_float_declared: true,
            artifact_blake3_hex: "aa".repeat(32),
            ed25519_signature_hex: "bb".repeat(64),
            secondary_signature_hex: None,
            nomination: Some(G8tNomination::advisory("fn_harmonic_fold_v2", 1, 42, 180)),
            authorization: auth,
        }
    }

    #[test]
    fn nomination_alone_never_authorizes_activation() {
        let m = sample_fn_manifest(
            SignatureRequirement::Ed25519Only,
            DeviceAuthorization::NominatedOnly,
        );
        assert_eq!(
            check_function_activation_policy(SignaturePolicy::Ed25519LocalInspectorDemo, &m),
            Err(PolicyError::NominationDoesNotAuthorizeInstall)
        );
    }

    #[test]
    fn dual_signed_capsule_is_never_silently_downgraded() {
        let m = sample_fn_manifest(
            SignatureRequirement::DualSignedRequired,
            DeviceAuthorization::LocallyAuthorized {
                authorized_by: "hue-local".to_string(),
                epoch_s: 1795000000,
                artifact_blake3_hex: "aa".repeat(32),
            },
        );
        assert_eq!(
            check_function_activation_policy(SignaturePolicy::Ed25519LocalInspectorDemo, &m),
            Err(PolicyError::PolicyDowngradeForbidden)
        );
    }

    #[test]
    fn locally_authorized_ed25519_demo_capsule_passes_policy() {
        let m = sample_fn_manifest(
            SignatureRequirement::Ed25519Only,
            DeviceAuthorization::LocallyAuthorized {
                authorized_by: "hue-local".to_string(),
                epoch_s: 1795000000,
                artifact_blake3_hex: "aa".repeat(32),
            },
        );
        assert!(
            check_function_activation_policy(SignaturePolicy::Ed25519LocalInspectorDemo, &m)
                .is_ok()
        );
    }

    #[test]
    fn driver_live_upgrade_and_bare_metal_activation_stay_locked() {
        assert!(matches!(
            DriverLiveUpgradeGate::current().check_live_upgrade(),
            Err(PolicyError::DriverLiveUpgradeBlocked {
                io_drain: false,
                dma_irq_revocation: false,
                independent_recovery: false,
            })
        ));
        assert!(matches!(
            BareMetalActivationGate::current().check_dynamic_activation(),
            Err(PolicyError::BareMetalActivationGated {
                mmu_isolation: false,
                iommu_dma_isolation: false,
            })
        ));
    }

    #[test]
    fn distinct_kind_permissions_reject_privilege_confusion() {
        let mut m = sample_fn_manifest(
            SignatureRequirement::Ed25519Only,
            DeviceAuthorization::Unreviewed,
        );
        m.permissions = CapsulePermissions::Function(FunctionPermissions {
            allow_fs: true,
            ..FunctionPermissions::strict_default(250)
        });
        assert!(matches!(
            m.validate_shape(),
            Err(PolicyError::PermissionViolation(_))
        ));

        let mut app_bad = CapsuleManifest {
            id: "inspector".to_string(),
            version: 1,
            kind: CapsuleKind::App,
            target_arch: "x86_64".to_string(),
            interface: NarrowInterface::InspectorSurfaceV1,
            permissions: CapsulePermissions::App(AppPermissions {
                allow_sanctum_m8: true,
                ..AppPermissions::inspector_local()
            }),
            lifecycle: CapsuleLifecycle::App(AppLifecycle::Running),
            signature_requirement: SignatureRequirement::Ed25519Only,
            zero_float_declared: true,
            artifact_blake3_hex: "00".repeat(32),
            ed25519_signature_hex: String::new(),
            secondary_signature_hex: None,
            nomination: None,
            authorization: DeviceAuthorization::Unreviewed,
        };
        assert!(matches!(
            app_bad.validate_shape(),
            Err(PolicyError::PermissionViolation(_))
        ));
        app_bad.permissions = CapsulePermissions::App(AppPermissions::inspector_local());
        assert!(app_bad.validate_shape().is_ok());
    }
}
