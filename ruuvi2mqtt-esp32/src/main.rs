use std::sync::atomic::{AtomicU32, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::Context;
use esp_idf_svc::{
    eventloop::EspSystemEventLoop, hal::peripherals::Peripherals, log::EspIdfLogger,
    nvs::EspDefaultNvsPartition, sys,
};
use log::{error, info, warn};
use ruuvi2mqtt_esp32::diagnostics::Diagnostics;

mod ble;
mod config;
mod led;
mod mqtt;
mod system;
mod wifi;

fn main() {
    // Required by esp-idf-svc: links esp-idf glue patches.
    sys::link_patches();

    // Initialise logging. LOG_LEVEL only applies to our own crate; library
    // code stays at info to avoid noise when debugging.
    log::set_max_level(APP_LEVEL);
    log::set_logger(&AppLogger).unwrap();

    info!(
        "{} {} starting",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    );

    // RTC memory keeps the count through deep sleep. Only this thread uses
    // it, so a plain load and store will do.
    let cycle = CYCLE.load(Ordering::Relaxed).wrapping_add(1);
    CYCLE.store(cycle, Ordering::Relaxed);

    // A panic or watchdog reset restarts main() immediately. Sleep instead, so
    // that a persistent failure costs one cycle per sleep period rather than a
    // continuous >100mA boot loop that drains the battery.
    match unsafe { sys::esp_reset_reason() } {
        sys::esp_reset_reason_t_ESP_RST_PANIC
        | sys::esp_reset_reason_t_ESP_RST_INT_WDT
        | sys::esp_reset_reason_t_ESP_RST_TASK_WDT => {
            warn!("Previous cycle crashed, skipping this one");
            deep_sleep(SLEEP_SECS);
        }
        // A weak battery browns out under Wi-Fi load. Give it time to recover.
        sys::esp_reset_reason_t_ESP_RST_BROWNOUT => {
            warn!("Previous cycle browned out (weak battery?), skipping this one");
            deep_sleep(BROWNOUT_SLEEP_SECS);
        }
        _ => {}
    }

    info!(
        "Cycle: {}s BLE scan, {}s deep sleep",
        config::BLE_SCAN_DURATION,
        config::BLE_SLEEP_DURATION
    );

    let start = Instant::now();

    if let Err(e) = run(start, cycle) {
        error!("Cycle failed: {e:#}");
    }

    deep_sleep(SLEEP_SECS);
}

/// Cycles since a cold boot or a crash, which reset RTC memory.
#[link_section = ".rtc.data.cycle"]
static CYCLE: AtomicU32 = AtomicU32::new(0);

/// Normal deep sleep between cycles.
const SLEEP_SECS: u64 = config::BLE_SLEEP_DURATION as u64;

/// How long to wait for the broker to accept the connection.
const MQTT_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// How long to wait for the broker to acknowledge the publishes.
const MQTT_ACK_TIMEOUT: Duration = Duration::from_secs(5);

/// Hard cap on awake time. The Wi-Fi and MQTT steps have their own timeouts,
/// but a task blocked on an event does not trip the task watchdog.
const CYCLE_TIMEOUT_SECS: u64 = 40 + config::BLE_SCAN_DURATION as u64;

/// Deep sleep after a brownout reset.
const BROWNOUT_SLEEP_SECS: u64 = 4 * SLEEP_SECS;

/// Enter deep sleep. On wake the chip reboots (`main()` runs fresh).
/// Deep sleep draws ~5-10µA vs >100mA active.
fn deep_sleep(secs: u64) -> ! {
    info!("Entering deep sleep for {secs}s");
    unsafe { sys::esp_deep_sleep(secs * 1_000_000) }
}

/// Milliseconds since `start`, for the diagnostics.
fn ms_since(start: Instant) -> u32 {
    ms_between(start, Instant::now())
}

fn ms_between(start: Instant, end: Instant) -> u32 {
    u32::try_from(end.saturating_duration_since(start).as_millis()).unwrap_or(u32::MAX)
}

/// One scan-connect-publish cycle.
fn run(start: Instant, cycle: u32) -> anyhow::Result<()> {
    thread::Builder::new()
        .stack_size(4096)
        .spawn(|| {
            thread::sleep(Duration::from_secs(CYCLE_TIMEOUT_SECS));
            error!("Cycle timed out after {CYCLE_TIMEOUT_SECS}s");
            deep_sleep(SLEEP_SECS);
        })
        .context("Failed to spawn cycle timeout thread")?;

    let peripherals = Peripherals::take()?;
    let sysloop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;

    // ── LED ──────────────────────────────────────────────────────────────────
    let mut led = led::Led::new()?;
    led.apply_mode();

    // ── BLE scan + Wi-Fi connect in parallel ─────────────────────────────────
    // Wi-Fi and BLE share one 2.4 GHz radio. Scanning while Wi-Fi connects
    // saves a few seconds of active time per cycle, but until Wi-Fi has
    // connected, the scan gets only part of the airtime.
    let ble_handle = thread::Builder::new()
        .stack_size(4096)
        .spawn(move || ble::scan(start))
        .context("Failed to spawn BLE scan thread")?;

    let (_wifi, wifi_cache) = wifi::connect(peripherals.modem, sysloop, nvs)?;
    let wifi_ms = ms_since(start);
    ble::wifi_connected();

    // ── MQTT ─────────────────────────────────────────────────────────────────
    let mut mqtt = mqtt::Mqtt::connect()?;

    // ── Publish readings ─────────────────────────────────────────────────────
    let (scan, error) = finish_scan(ble_handle, start);

    let mqtt_ms = ms_between(start, mqtt.wait_connected(MQTT_CONNECT_TIMEOUT)?);

    for reading in &scan.readings {
        let topic = format!(
            "{}/{}",
            config::MQTT_BASE_TOPIC,
            reading.mac.to_topic_string()
        );
        match mqtt.publish(&topic, &reading.payload) {
            Ok(()) => info!("Updating: [{}]", reading.mac),
            Err(e) => error!("Failed to publish [{}]: {e}", reading.mac),
        }
    }

    // ── Diagnostics ──────────────────────────────────────────────────────────
    let ap_info = system::ap_info();
    let diag = Diagnostics {
        firmware: concat!(env!("CARGO_PKG_NAME"), " ", env!("BUILD_VERSION")),
        client_id: config::MQTT_CLIENT_ID,
        chip: system::chip(),
        cycle,
        wifi_rssi: ap_info.map(|(rssi, _)| rssi),
        wifi_channel: ap_info.map(|(_, channel)| channel),
        wifi_cache,
        free_heap: system::free_heap(),
        scan_start_ms: scan.started_ms,
        scan_end_ms: scan.finished_ms,
        wifi_ms,
        mqtt_ms,
        cycle_ms: ms_since(start),
        scan: scan.stats,
        error,
    };
    let topic = format!(
        "{}/diagnostics/{}",
        config::MQTT_BASE_TOPIC,
        config::DEVICE_HOSTNAME,
    );
    let payload = diag.to_json();
    info!("Diagnostics: {payload}");
    if let Err(e) = mqtt.publish(&topic, &payload) {
        error!("Failed to publish diagnostics: {e}");
    }

    mqtt.wait_published(MQTT_ACK_TIMEOUT)?;

    led.off();
    Ok(())
}

/// Wait for the scan to finish. A failed scan is still reported in the
/// diagnostics, so it returns the error instead.
fn finish_scan(
    handle: JoinHandle<anyhow::Result<ble::Scan>>,
    start: Instant,
) -> (ble::Scan, Option<String>) {
    let error = match handle.join() {
        Ok(Ok(scan)) => return (scan, None),
        Ok(Err(e)) => format!("BLE scan failed: {e:#}"),
        Err(_) => "BLE scan thread panicked".to_string(),
    };
    error!("{error}");
    let now = ms_since(start);
    let scan = ble::Scan {
        started_ms: now,
        finished_ms: now,
        ..ble::Scan::default()
    };
    (scan, Some(error))
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
