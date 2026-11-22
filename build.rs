//! Embeds a build version string in `DMBDIP_VERSION`.
//!
//! Builds made exactly at the release tag `v<version>` (and builds without git,
//! e.g. from crates.io) report the plain package version. Any other build
//! appends `git describe` output, e.g. `0.6.0 (v0.6.0-3-g7279474-dirty)`.

use std::path::PathBuf;
use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// Only trust git when the crate root is the repository root; otherwise we may be
/// building a packaged crate that merely sits inside some unrelated repository.
fn in_own_repo() -> bool {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let top = git(&["rev-parse", "--show-toplevel"]).map(PathBuf::from);
    match (
        manifest.canonicalize(),
        top.and_then(|t| t.canonicalize().ok()),
    ) {
        (Ok(m), Some(t)) => m == t,
        _ => false,
    }
}

fn main() {
    let pkg = env!("CARGO_PKG_VERSION");
    let tag = format!("v{pkg}");
    println!("cargo:rerun-if-env-changed=GITHUB_REF");
    let ci_tag_build = std::env::var("GITHUB_REF").is_ok_and(|r| r == format!("refs/tags/{tag}"));

    let describe = in_own_repo()
        .then(|| git(&["describe", "--tags", "--match", "v*", "--always", "--dirty"]))
        .flatten();
    let version = match describe {
        Some(d) if d != tag && !ci_tag_build => format!("{pkg} ({d})"),
        _ => pkg.to_string(),
    };
    println!("cargo:rustc-env=DMBDIP_VERSION={version}");

    if !in_own_repo() {
        return;
    }
    // Re-run when HEAD moves, refs/tags change, or sources change (dirty flag).
    println!("cargo:rerun-if-changed=src");
    if let Some(git_dir) = git(&["rev-parse", "--git-dir"]) {
        let git_dir = PathBuf::from(git_dir);
        println!("cargo:rerun-if-changed={}", git_dir.join("HEAD").display());
        println!("cargo:rerun-if-changed={}", git_dir.join("index").display());
    }
    if let Some(common) = git(&["rev-parse", "--git-common-dir"]) {
        let common = PathBuf::from(common);
        println!("cargo:rerun-if-changed={}", common.join("refs").display());
        println!(
            "cargo:rerun-if-changed={}",
            common.join("packed-refs").display()
        );
    }
}
