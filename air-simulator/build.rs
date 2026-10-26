fn main() {
    // Exports the ESP-IDF build environment that embuild sets up.
    embuild::espidf::sysenv::output();
    println!("cargo:rerun-if-env-changed=AIR_MAC");
    println!("cargo:rerun-if-env-changed=AIR_SETS");
    for name in ["LED_GPIO", "LED_TYPE", "LED_COLOR"] {
        println!("cargo:rerun-if-env-changed={name}");
    }
}
