use std::{env, path::PathBuf, process::Command};

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn main() {
    // An intentionally absent input forces a fresh Git identity/dirty check on
    // each invocation, including edits outside this package and ref updates.
    println!("cargo:rerun-if-changed=.always-check-build-identity");
    println!("cargo:rerun-if-env-changed=AYEOS_BUILD_REVISION");
    println!("cargo:rerun-if-env-changed=AYEOS_BUILD_DIRTY");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=vendor");
    for name in [
        "HEAD".to_owned(),
        "index".to_owned(),
        git(&["symbolic-ref", "HEAD"]).unwrap_or_default(),
    ] {
        if let Some(path) = git(&["rev-parse", "--git-path", &name]) {
            println!("cargo:rerun-if-changed={}", PathBuf::from(path).display());
        }
    }
    let revision = env::var("AYEOS_BUILD_REVISION")
        .ok()
        .or_else(|| git(&["rev-parse", "HEAD"]))
        .unwrap_or_else(|| "0".repeat(40));
    assert!(revision.len() == 40 && revision.bytes().all(|b| b.is_ascii_hexdigit()));
    let dirty = env::var("AYEOS_BUILD_DIRTY").unwrap_or_else(|_| {
        if git(&["status", "--porcelain"]).is_some_and(|s| s.is_empty()) {
            "0"
        } else {
            "1"
        }
        .into()
    });
    assert!(dirty == "0" || dirty == "1");
    println!("cargo:rustc-env=AYEOS_BUILD_REVISION={revision}");
    println!("cargo:rustc-env=AYEOS_BUILD_DIRTY={dirty}");
}
