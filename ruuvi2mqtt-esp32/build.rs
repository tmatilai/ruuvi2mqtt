//! The embuild glue for ESP-IDF, and `BUILD_VERSION`.

use std::process::Command;

fn main() {
    // Required by esp-idf-sys (via embuild) to export ESP-IDF build environment
    // variables so that the linker and the Rust crate can find ESP-IDF headers
    // and libraries that are downloaded/built by embuild.
    embuild::espidf::sysenv::output();

    // Set by esp-idf-sys on chips with 5 GHz Wi-Fi.
    println!("cargo:rustc-check-cfg=cfg(esp_idf_soc_wifi_support_5g)");
    // Set by esp-idf-sys on chips with a USB port of their own. main.rs checks it.
    println!("cargo:rustc-check-cfg=cfg(esp_idf_soc_usb_serial_jtag_supported)");

    // The package version plus the git commit, so the diagnostics show
    // exactly what a board was flashed with.
    let version = env!("CARGO_PKG_VERSION");
    let build_version = match git_revision() {
        Some(revision) => format!("{version} ({revision})"),
        None => version.to_string(),
    };
    println!("cargo:rustc-env=BUILD_VERSION={build_version}");
}

/// The short commit hash, with `-dirty` for uncommitted changes.
///
/// The same as in the app's `build.rs`, which works with git 2.7.
fn git_revision() -> Option<String> {
    // Rerun when a commit, checkout or staging changes HEAD, its reflog or
    // the index, and when src changes. git and Cargo both take a relative
    // git dir as relative to the package directory.
    let git_dir = git(&["rev-parse", "--git-dir"])?;
    for path in ["HEAD", "logs/HEAD", "index"] {
        println!("cargo:rerun-if-changed={git_dir}/{path}");
    }
    println!("cargo:rerun-if-changed=src");

    let hash = git(&["rev-parse", "--short", "HEAD"])?;
    // Untracked files don't count, as with `git describe --dirty`.
    let changes = git(&["status", "--porcelain", "--untracked-files=no"])?;
    Some(if changes.is_empty() {
        hash
    } else {
        format!("{hash}-dirty")
    })
}

fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    Some(stdout.trim().to_string())
}
