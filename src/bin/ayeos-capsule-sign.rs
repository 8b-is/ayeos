//! Off-device Ed25519 signer CLI for AyeOS capsules (`ayeos-capsule-sign`).
//!
//! Keeps the Ed25519 private signing key strictly off-device; AyeOS / `ayeos-inspector`
//! receives ONLY the 32-byte Ed25519 public key. Uses BLAKE3 strictly as the artifact
//! content digest, signed by Ed25519 over `CapsuleManifest::canonical_signing_bytes()`.

use ayeos_manifest::{
    CapsuleKind, CapsuleLifecycle, CapsuleManifest, CapsulePermissions, DeviceAuthorization,
    FunctionLifecycle, FunctionPermissions, NarrowInterface, SignatureRequirement,
};
use ed25519_dalek::{Signer, SigningKey};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process;

fn read_os_random_32() -> [u8; 32] {
    let mut seed = [0u8; 32];
    #[cfg(target_os = "linux")]
    {
        use std::io::Read;
        let mut f = fs::File::open("/dev/urandom").expect("open /dev/urandom");
        f.read_exact(&mut seed).expect("read 32 random bytes");
    }
    #[cfg(not(target_os = "linux"))]
    {
        panic!("keygen requires /dev/urandom");
    }
    seed
}

fn parse_hex_32(s: &str) -> Result<[u8; 32], String> {
    let raw = hex::decode(s.trim()).map_err(|e| format!("invalid hex: {e}"))?;
    if raw.len() != 32 {
        return Err(format!("expected 32 bytes (64 hex chars), got {}", raw.len()));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&raw);
    Ok(out)
}

fn usage() -> ! {
    eprintln!(
        "Usage:\n  \
         ayeos-capsule-sign keygen --secret <secret.hex> --public <public.hex>\n  \
         ayeos-capsule-sign sign --key <secret.hex> --artifact <file.so> --id <capsule_id> \
         [--version <u32>] [--deadline-ms <u32>] [--sig-req <ed25519|dual>] --out <manifest.json>"
    );
    process::exit(2);
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        usage();
    }

    match args[1].as_str() {
        "keygen" => {
            let mut secret_out: Option<PathBuf> = None;
            let mut public_out: Option<PathBuf> = None;
            let mut i = 2;
            while i < args.len() {
                match args[i].as_str() {
                    "--secret" if i + 1 < args.len() => {
                        secret_out = Some(PathBuf::from(&args[i + 1]));
                        i += 2;
                    }
                    "--public" if i + 1 < args.len() => {
                        public_out = Some(PathBuf::from(&args[i + 1]));
                        i += 2;
                    }
                    _ => usage(),
                }
            }
            let (Some(sec_path), Some(pub_path)) = (secret_out, public_out) else {
                usage();
            };
            let seed = read_os_random_32();
            let signing_key = SigningKey::from_bytes(&seed);
            let verifying_key = signing_key.verifying_key();
            fs::write(&sec_path, format!("{}\n", hex::encode(signing_key.to_bytes())))
                .expect("write secret key");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&sec_path, fs::Permissions::from_mode(0o600));
            }
            fs::write(&pub_path, format!("{}\n", hex::encode(verifying_key.to_bytes())))
                .expect("write public key");
            println!(
                "{{\"ok\":true,\"public_key_hex\":\"{}\",\"secret_path\":\"{}\",\"public_path\":\"{}\"}}",
                hex::encode(verifying_key.to_bytes()),
                sec_path.display(),
                pub_path.display()
            );
        }
        "sign" => {
            let mut key_path: Option<PathBuf> = None;
            let mut artifact_path: Option<PathBuf> = None;
            let mut capsule_id: Option<String> = None;
            let mut out_path: Option<PathBuf> = None;
            let mut version: u32 = 1;
            let mut deadline_ms: u32 = 150;
            let mut sig_req = SignatureRequirement::Ed25519Only;

            let mut i = 2;
            while i < args.len() {
                match args[i].as_str() {
                    "--key" if i + 1 < args.len() => {
                        key_path = Some(PathBuf::from(&args[i + 1]));
                        i += 2;
                    }
                    "--artifact" if i + 1 < args.len() => {
                        artifact_path = Some(PathBuf::from(&args[i + 1]));
                        i += 2;
                    }
                    "--id" if i + 1 < args.len() => {
                        capsule_id = Some(args[i + 1].clone());
                        i += 2;
                    }
                    "--version" if i + 1 < args.len() => {
                        version = args[i + 1].parse().expect("valid u32 version");
                        i += 2;
                    }
                    "--deadline-ms" if i + 1 < args.len() => {
                        deadline_ms = args[i + 1].parse().expect("valid u32 deadline");
                        i += 2;
                    }
                    "--sig-req" if i + 1 < args.len() => {
                        sig_req = match args[i + 1].as_str() {
                            "ed25519" => SignatureRequirement::Ed25519Only,
                            "dual" => SignatureRequirement::DualSignedRequired,
                            _ => usage(),
                        };
                        i += 2;
                    }
                    "--out" if i + 1 < args.len() => {
                        out_path = Some(PathBuf::from(&args[i + 1]));
                        i += 2;
                    }
                    _ => usage(),
                }
            }
            let (Some(key_p), Some(art_p), Some(id), Some(out_p)) =
                (key_path, artifact_path, capsule_id, out_path)
            else {
                usage();
            };

            let key_hex = fs::read_to_string(&key_p).expect("read secret key");
            let secret_bytes = parse_hex_32(&key_hex).expect("parse secret key hex");
            let signing_key = SigningKey::from_bytes(&secret_bytes);

            let artifact_bytes = fs::read(&art_p).expect("read artifact");
            let blake3_hex = blake3::hash(&artifact_bytes).to_hex().to_string();

            let mut manifest = CapsuleManifest {
                id,
                version,
                kind: CapsuleKind::Function,
                target_arch: env::consts::ARCH.to_string(),
                interface: NarrowInterface::PureFnI64x2ToI64,
                permissions: CapsulePermissions::Function(FunctionPermissions::strict_default(
                    deadline_ms,
                )),
                lifecycle: CapsuleLifecycle::Function(FunctionLifecycle::StagedLocal),
                signature_requirement: sig_req,
                zero_float_declared: true,
                artifact_blake3_hex: blake3_hex.clone(),
                ed25519_signature_hex: String::new(),
                secondary_signature_hex: None,
                nomination: None,
                authorization: DeviceAuthorization::Unreviewed,
            };

            let canonical = manifest.canonical_signing_bytes();
            let sig = signing_key.sign(&canonical);
            manifest.ed25519_signature_hex = hex::encode(sig.to_bytes());

            let json = serde_json::to_string_pretty(&manifest).expect("serialize manifest");
            fs::write(&out_p, json).expect("write manifest");

            println!(
                "{{\"ok\":true,\"id\":\"{}\",\"artifact_blake3\":\"{}\",\"manifest\":\"{}\"}}",
                manifest.id,
                blake3_hex,
                out_p.display()
            );
        }
        _ => usage(),
    }
}
