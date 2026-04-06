use std::path::PathBuf;

fn main() {
    // Required by esp-idf-sys (via embuild) to export ESP-IDF build environment
    // variables so that the linker and the Rust crate can find ESP-IDF headers
    // and libraries that are downloaded/built by embuild.
    embuild::espidf::sysenv::output();

    println!("cargo:rustc-check-cfg=cfg(mqtt_ca_file)");

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
}
