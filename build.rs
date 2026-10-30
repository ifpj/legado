//! Capture the build identity at compile time. The value comes from
//! `CF_GATEWAY_BUILD_VERSION` when a CI or container build provides it, else
//! from `git describe` on the checkout. `src/version.rs` parses it.
use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!text.is_empty()).then_some(text)
}

fn main() {
    println!("cargo:rerun-if-env-changed=CF_GATEWAY_BUILD_VERSION");
    // Any tracked input change can flip the dirty flag; these are cheap checks.
    for path in [
        "Cargo.toml",
        "src",
        "assets",
        "profiles",
        "tests",
        "build.rs",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
    if let Some(dir) = git(&["rev-parse", "--git-dir"]) {
        for name in ["HEAD", "index", "packed-refs"] {
            println!("cargo:rerun-if-changed={dir}/{name}");
        }
        if let Some(common) = git(&["rev-parse", "--git-common-dir"]) {
            println!("cargo:rerun-if-changed={common}/packed-refs");
            println!("cargo:rerun-if-changed={common}/refs/tags");
        }
    }
    let describe = std::env::var("CF_GATEWAY_BUILD_VERSION")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .or_else(|| {
            git(&[
                "describe",
                "--tags",
                "--match",
                "cf-v*",
                "--long",
                "--dirty",
                "--always",
                "--abbrev=9",
            ])
        })
        .unwrap_or_default();
    println!("cargo:rustc-env=CF_GATEWAY_DESCRIBE={describe}");
}
