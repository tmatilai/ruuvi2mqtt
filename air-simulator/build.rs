fn main() {
    // Exports the ESP-IDF build environment that embuild sets up.
    embuild::espidf::sysenv::output();
    println!("cargo:rerun-if-env-changed=AIR_MAC");
    println!("cargo:rerun-if-env-changed=AIR_SETS");
    println!("cargo:rerun-if-env-changed=ANTENNA");
    for name in [
        "LED_MODE",
        "LED_TYPE",
        "LED_GPIO",
        "LED_ACTIVE_LOW",
        "LED_COLOR",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
    }
}
