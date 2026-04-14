use std::{thread, time::Duration};

use esp_idf_svc::{
    eventloop::EspSystemEventLoop, hal::peripherals::Peripherals, log::EspIdfLogger,
    nvs::EspDefaultNvsPartition,
};
use log::{error, info};

mod ble;
mod config;
mod led;
mod mac;
mod mqtt;
mod wifi;

fn main() {
    // Required by esp-idf-svc: links esp-idf glue patches.
    esp_idf_svc::sys::link_patches();

    // Initialise logging. LOG_LEVEL only applies to our own crate; library
    // code stays at info to avoid noise when debugging.
    log::set_max_level(APP_LEVEL);
    log::set_logger(&AppLogger).unwrap();

    info!(
        "{} {} starting",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    );

    info!(
        "Cycle: {}s BLE scan, {}s deep sleep",
        config::BLE_SCAN_DURATION,
        config::BLE_SLEEP_DURATION
    );

    if let Err(e) = run() {
        error!("Cycle failed: {e:#}");
    }

    deep_sleep();
}

/// Enter deep sleep. On wake the chip reboots (`main()` runs fresh).
/// Deep sleep draws ~5-10µA vs >100mA active.
fn deep_sleep() -> ! {
    info!("Entering deep sleep for {}s", config::BLE_SLEEP_DURATION);
    unsafe {
        esp_idf_svc::sys::esp_deep_sleep(config::BLE_SLEEP_DURATION as u64 * 1_000_000);
    }
}

/// One scan-connect-publish cycle.
fn run() -> anyhow::Result<()> {
    let peripherals = Peripherals::take()?;
    let sysloop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;

    // ── LED ──────────────────────────────────────────────────────────────────
    let mut led = led::Led::new()?;
    led.apply_mode();

    // ── BLE scan + Wi-Fi connect in parallel ─────────────────────────────────
    // BLE and Wi-Fi use independent radios, so scan while connecting to save
    // a few seconds of active time per cycle.
    let ble_handle = thread::Builder::new()
        .stack_size(4096)
        .spawn(ble::scan_once)
        .expect("failed to spawn BLE scan thread");

    let _wifi = wifi::connect(peripherals.modem, sysloop, nvs)?;

    // ── MQTT ─────────────────────────────────────────────────────────────────
    let (mut mqtt_client, mqtt_conn) = mqtt::connect()?;

    thread::Builder::new()
        .stack_size(8192)
        .spawn(move || mqtt::run_event_loop(mqtt_conn))
        .expect("failed to spawn MQTT event-loop thread");

    // ── Publish readings ─────────────────────────────────────────────────────
    let readings = ble_handle.join().expect("BLE scan thread panicked")?;
    for reading in readings {
        let topic_mac = reading.mac.to_topic_string();
        match mqtt::publish(&mut mqtt_client, &topic_mac, &reading.payload) {
            Ok(()) => info!("Updating: [{}]", reading.mac),
            Err(e) => error!("Failed to publish [{}]: {e}", reading.mac),
        }
    }

    // Brief delay to let QoS 1 publishes get acknowledged.
    thread::sleep(Duration::from_millis(500));

    led.off();
    Ok(())
}

/// Thin logger wrapper that applies `LOG_LEVEL` only to this crate's modules
/// and keeps library code at `Info`.
struct AppLogger;

/// Compile-time log level parsed from `LOG_LEVEL`.
static APP_LEVEL: log::LevelFilter = match config::LOG_LEVEL.as_bytes() {
    b"trace" | b"TRACE" => log::LevelFilter::Trace,
    b"debug" | b"DEBUG" => log::LevelFilter::Debug,
    b"warn" | b"WARN" => log::LevelFilter::Warn,
    b"error" | b"ERROR" => log::LevelFilter::Error,
    b"off" | b"OFF" => log::LevelFilter::Off,
    _ => log::LevelFilter::Info,
};

const APP_MODULE_PREFIX: &str = env!("CARGO_CRATE_NAME");

/// Noop-filtered ESP-IDF logger — we handle filtering in `AppLogger::enabled`.
static ESP_LOGGER: EspIdfLogger<()> = EspIdfLogger::new(());

impl log::Log for AppLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        let level = if metadata.target().starts_with(APP_MODULE_PREFIX) {
            APP_LEVEL
        } else {
            log::LevelFilter::Info
        };
        metadata.level() <= level
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            ESP_LOGGER.log(record);
        }
    }

    fn flush(&self) {
        ESP_LOGGER.flush();
    }
}
