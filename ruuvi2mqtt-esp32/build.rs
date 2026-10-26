use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=MQTT_CA_FILE");

    // Required by esp-idf-sys (via embuild) to export ESP-IDF build environment
    // variables so that the linker and the Rust crate can find ESP-IDF headers
    // and libraries that are downloaded/built by embuild.
    embuild::espidf::sysenv::output();

    println!("cargo:rustc-check-cfg=cfg(mqtt_ca_file)");
    // Set by esp-idf-sys on chips with 5 GHz Wi-Fi. config.rs checks WIFI_BAND with it.
    println!("cargo:rustc-check-cfg=cfg(esp_idf_soc_wifi_support_5g)");
    // Set by esp-idf-sys on chips with a USB port of their own. main.rs checks it.
    println!("cargo:rustc-check-cfg=cfg(esp_idf_soc_usb_serial_jtag_supported)");

    // If MQTT_CA_FILE is set, copy the CA certificate into OUT_DIR so that
    // the source code can embed it with `include_bytes!`.
    if let Ok(ca_path) = std::env::var("MQTT_CA_FILE") {
        let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
        let dest = out_dir.join("mqtt_ca.pem");
        std::fs::copy(&ca_path, &dest).unwrap_or_else(|e| {
            panic!("Failed to copy MQTT_CA_FILE ({ca_path}): {e}");
        });
        println!("cargo:rerun-if-changed={ca_path}");
        println!("cargo:rustc-env=MQTT_CA_PEM_PATH={}", dest.display());
        println!("cargo:rustc-cfg=mqtt_ca_file");
    }

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
fn git_revision() -> Option<String> {
    // HEAD and its reflog change on commit and checkout, the index on
    // staging, and src on edits.
    let git_dir = git(&["rev-parse", "--absolute-git-dir"])?;
    for path in ["HEAD", "logs/HEAD", "index"] {
        println!("cargo:rerun-if-changed={git_dir}/{path}");
    }
    println!("cargo:rerun-if-changed=src");

    // `--exclude=*` skips the tags, so the output is only the hash.
    git(&["describe", "--always", "--dirty", "--exclude=*"])
}

fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    Some(stdout.trim().to_string())
}
