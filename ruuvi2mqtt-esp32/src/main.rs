use std::convert::Infallible;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::Context;
use esp_idf_svc::{
    eventloop::EspSystemEventLoop, hal::peripherals::Peripherals, log::EspIdfLogger,
    nvs::EspDefaultNvsPartition, sys,
};
use log::{error, info, warn};
use ruuvi2mqtt_esp32::diagnostics::{CacheUse, Diagnostics, Settings};
use ruuvi2mqtt_esp32::mac::Mac;

mod antenna;
mod ble;
mod config;
mod led;
mod mqtt;
mod system;
mod web;
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

    let reset_reason = unsafe { sys::esp_reset_reason() };

    // The LED blinks while the previous cycle failed, until this one
    // succeeds. That way the deep-sleep modes show a failure in the next
    // wake, as they sleep right after it.
    let failed_before = match reset_reason {
        sys::esp_reset_reason_t_ESP_RST_DEEPSLEEP | sys::esp_reset_reason_t_ESP_RST_SW => {
            LAST_CYCLE_FAILED.load(Ordering::Relaxed) == 1
        }
        sys::esp_reset_reason_t_ESP_RST_PANIC
        | sys::esp_reset_reason_t_ESP_RST_INT_WDT
        | sys::esp_reset_reason_t_ESP_RST_TASK_WDT
        | sys::esp_reset_reason_t_ESP_RST_BROWNOUT => true,
        _ => false,
    };
    LAST_CYCLE_FAILED.store(1, Ordering::Relaxed);
    led::set_failed(failed_before);
    if let Err(e) = led::start() {
        warn!("LED failed: {e:#}");
    }

    // Deep sleep and sleep()'s restart continue the count, other resets start
    // it over. Only this thread uses it, so a plain load and store will do.
    let cycle = match reset_reason {
        sys::esp_reset_reason_t_ESP_RST_DEEPSLEEP | sys::esp_reset_reason_t_ESP_RST_SW => {
            CYCLE.load(Ordering::Relaxed).wrapping_add(1)
        }
        _ => 1,
    };
    CYCLE.store(cycle, Ordering::Relaxed);

    // A panic or watchdog reset restarts main() immediately. Sleep instead, so
    // that a persistent failure costs one cycle per sleep period rather than a
    // continuous >100mA boot loop that drains the battery.
    match reset_reason {
        sys::esp_reset_reason_t_ESP_RST_PANIC
        | sys::esp_reset_reason_t_ESP_RST_INT_WDT
        | sys::esp_reset_reason_t_ESP_RST_TASK_WDT => {
            warn!("Previous cycle crashed, skipping this one");
            sleep(SLEEP_SECS);
        }
        // A weak battery browns out under Wi-Fi load. Give it time to recover.
        sys::esp_reset_reason_t_ESP_RST_BROWNOUT => {
            warn!("Previous cycle browned out (weak battery?), skipping this one");
            sleep(BROWNOUT_SLEEP_SECS);
        }
        _ => {}
    }

    if config::BLE_SCAN_CONTINUOUS {
        info!("Cycle: {}s continuous BLE scan", config::BLE_SCAN_DURATION);
        let Err(e) = run_continuous(cycle);
        error!("Continuous scan failed: {e:#}");
    } else {
        info!(
            "Cycle: {}s {} BLE scan, {}s deep sleep",
            config::BLE_SCAN_DURATION,
            config::BLE_SCAN_MODE,
            config::BLE_SLEEP_DURATION
        );
        match run(cycle) {
            Ok(()) => LAST_CYCLE_FAILED.store(0, Ordering::Relaxed),
            Err(e) => error!("Cycle failed: {e:#}"),
        }
    }

    sleep(SLEEP_SECS);
}

/// Cycles since a cold boot or a crash. Uninitialized RTC memory, which
/// `esp_restart()` keeps, unlike `.rtc.data`. `main()` sets it every boot.
#[link_section = ".rtc_noinit.cycle"]
static CYCLE: AtomicU32 = AtomicU32::new(0);

/// 1 until a cycle succeeds. Uninitialized, as `CYCLE`, so `main()` reads
/// it only after deep sleep or `sleep()`'s restart. The continuous mode
/// restarts only after a failure, so it never clears it.
#[link_section = ".rtc_noinit.last_cycle_failed"]
static LAST_CYCLE_FAILED: AtomicU8 = AtomicU8::new(0);

/// Set while `sleep()` waits awake, so the cycle timeout doesn't fire.
static STAYING_AWAKE: AtomicBool = AtomicBool::new(false);

/// When the cycle timeout fires, in seconds since boot.
static DEADLINE_SECS: AtomicU32 = AtomicU32::new(u32::MAX);

/// Normal deep sleep between cycles.
const SLEEP_SECS: u64 = config::BLE_SLEEP_DURATION as u64;

/// One BLE scan.
const SCAN_SECS: u64 = config::BLE_SCAN_DURATION as u64;

/// How long to wait for the broker to accept the connection.
const MQTT_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// How long to wait for the broker to acknowledge the publishes.
const MQTT_ACK_TIMEOUT: Duration = Duration::from_secs(5);

/// Hard cap on awake time. The Wi-Fi and MQTT steps have their own timeouts,
/// but a task blocked on an event does not trip the task watchdog. In the
/// continuous mode, it is the time since the last cycle that scanned and
/// published, so one cycle may fail, as when Wi-Fi drops for a while.
const CYCLE_TIMEOUT_SECS: u32 = if config::BLE_SCAN_CONTINUOUS {
    40 + 2 * config::BLE_SCAN_DURATION.unsigned_abs()
} else {
    40 + config::BLE_SCAN_DURATION.unsigned_abs()
};

/// Deep sleep after a brownout reset.
const BROWNOUT_SLEEP_SECS: u64 = 4 * SLEEP_SECS;

/// Enter deep sleep. On wake the chip reboots (`main()` runs fresh).
/// Deep sleep draws ~5-10µA vs >100mA active.
///
/// Deep sleep turns off the chip's own USB port. While a computer is
/// connected to it, wait awake instead and restart, so the port stays up
/// for flashing and monitoring. `esp_restart()` resets the CPU, not the port.
///
/// Scanning continuously, only a failure gets here. The board is on mains
/// power, so it waits awake, and the LED blinks.
fn sleep(secs: u64) -> ! {
    if config::BLE_SCAN_CONTINUOUS || (config::USB_STAY_AWAKE && usb_host_connected()) {
        STAYING_AWAKE.store(true, Ordering::Relaxed);
        if config::BLE_SCAN_CONTINUOUS {
            led::set_failed(true);
            info!("Restarting in {secs}s");
        } else {
            led::off();
            info!("USB host connected, staying awake for {secs}s");
        }
        thread::sleep(Duration::from_secs(secs));
        unsafe { sys::esp_restart() }
    }
    led::off();
    antenna::power_off();
    info!("Entering deep sleep for {secs}s");
    unsafe { sys::esp_deep_sleep(secs * 1_000_000) }
}

/// A computer sends a USB frame every millisecond, a charger none.
#[cfg(esp_idf_soc_usb_serial_jtag_supported)]
fn usb_host_connected() -> bool {
    unsafe { sys::usb_serial_jtag_is_connected() }
}

#[cfg(not(esp_idf_soc_usb_serial_jtag_supported))]
fn usb_host_connected() -> bool {
    false
}

/// Milliseconds since `start`, for the diagnostics.
fn ms_since(start: Instant) -> u32 {
    ms_between(start, Instant::now())
}

fn ms_between(start: Instant, end: Instant) -> u32 {
    u32::try_from(end.saturating_duration_since(start).as_millis()).unwrap_or(u32::MAX)
}

fn uptime_secs() -> u32 {
    u32::try_from(unsafe { sys::esp_timer_get_time() } / 1_000_000).unwrap_or(u32::MAX)
}

/// Move the cycle timeout to `CYCLE_TIMEOUT_SECS` from now.
fn extend_deadline() {
    DEADLINE_SECS.store(
        uptime_secs().saturating_add(CYCLE_TIMEOUT_SECS),
        Ordering::Relaxed,
    );
}

/// Start the cycle timeout, which deep sleeps once the deadline has passed.
fn start_cycle_timeout() -> anyhow::Result<()> {
    extend_deadline();
    thread::Builder::new()
        .stack_size(4096)
        .spawn(|| loop {
            thread::sleep(Duration::from_secs(1));
            if uptime_secs() > DEADLINE_SECS.load(Ordering::Relaxed)
                && !STAYING_AWAKE.load(Ordering::Relaxed)
            {
                error!("Cycle timed out after {CYCLE_TIMEOUT_SECS}s");
                sleep(SLEEP_SECS);
            }
        })
        .context("Failed to spawn cycle timeout thread")?;
    Ok(())
}

/// One scan-connect-publish cycle, before deep sleep.
fn run(number: u32) -> anyhow::Result<()> {
    let start = Instant::now();
    start_cycle_timeout()?;

    let peripherals = Peripherals::take()?;
    let sysloop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;

    antenna::select();

    // ── BLE scan + Wi-Fi connect ─────────────────────────────────────────────
    // Wi-Fi and BLE share one 2.4 GHz radio. Scanning while Wi-Fi connects
    // saves a few seconds of active time per cycle, but until Wi-Fi has
    // connected, the scan gets only part of the airtime. The sequential mode
    // finishes the scan before Wi-Fi starts.
    let ble_handle = spawn_scan(start)?;
    let scan = if config::BLE_SCAN_SEQUENTIAL {
        ScanThread::Finished(finish_scan(ble_handle, start))
    } else {
        ScanThread::Running(ble_handle)
    };

    let (_wifi, wifi_cache) = wifi::connect(peripherals.modem, sysloop, nvs)?;
    let wifi_ms = ms_since(start);
    ble::set_wifi_connected(true);

    // ── MQTT ─────────────────────────────────────────────────────────────────
    let mut mqtt = mqtt::Mqtt::connect()?;

    let (scan, error) = scan.finish(start);
    let cycle = Cycle {
        number,
        start,
        wifi_cache,
        wifi_ms,
    };
    publish(&mut mqtt, &cycle, scan, error)?;
    Ok(())
}

/// Scan all the time, and publish each scan's readings and diagnostics while
/// the next scan runs. Wi-Fi and MQTT stay connected. Returns only on a
/// failure to start.
fn run_continuous(mut number: u32) -> anyhow::Result<Infallible> {
    start_cycle_timeout()?;

    let peripherals = Peripherals::take()?;
    let sysloop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;

    antenna::select();

    // One thread scans for good. The channel holds one scan, so that the
    // next one runs while this thread publishes.
    let (scan_tx, scans) = mpsc::sync_channel(1);
    thread::Builder::new()
        .stack_size(4096)
        .spawn(move || loop {
            let start = Instant::now();
            let result = ble::scan(start);
            let failed = result.is_err();
            if scan_tx.send((start, result)).is_err() {
                return;
            }
            // A failed scan returns at once. Wait out the scan, so that the
            // cycle keeps its length and the diagnostics their rate.
            if failed {
                thread::sleep(Duration::from_secs(SCAN_SECS));
            }
        })
        .context("Failed to spawn BLE scan thread")?;

    // Show a bad spot from boot, not only after the first restart.
    led::set_wifi_down(true);
    let (_wifi, wifi_cache) = wifi::connect(peripherals.modem, sysloop.clone(), nvs)?;
    let _reconnect = wifi::keep_connected(&sysloop)?;
    // The board works without it.
    let _web = web::start()
        .inspect_err(|e| warn!("Status page failed: {e:#}"))
        .ok();
    let mut mqtt = mqtt::Mqtt::connect()?;
    // Subscribes to the tag names for the page now, not after the first
    // scan. The cycles wait for the broker anyway.
    if let Err(e) = mqtt.wait_connected(MQTT_CONNECT_TIMEOUT) {
        warn!("{e:#}");
    }

    loop {
        CYCLE.store(number, Ordering::Relaxed);
        let (cycle_start, result) = scans.recv().context("BLE scan thread ended")?;
        let (result, error) = scan_result(result, cycle_start);
        let scan_failed = error.is_some();

        // Zero when Wi-Fi was up all along, the whole cycle when it was down.
        let wifi_ms = wifi::connected_since()
            .map_or_else(|| ms_since(cycle_start), |at| ms_between(cycle_start, at));
        let cycle = Cycle {
            number,
            start: cycle_start,
            wifi_cache,
            wifi_ms,
        };
        // A failed scan is published too, but doesn't count. That way a
        // broken BLE stack restarts the board, as a dead Wi-Fi does.
        let failed = match publish(&mut mqtt, &cycle, result, error) {
            Ok(()) => scan_failed,
            Err(e) => {
                error!("Cycle {number} failed: {e:#}");
                true
            }
        };
        if !failed {
            extend_deadline();
        }
        led::set_failed(failed);
        web::cycle_done(failed);

        number = number.wrapping_add(1);
    }
}

/// A cycle's values for the diagnostics, besides the scan's.
struct Cycle {
    number: u32,
    start: Instant,
    /// How Wi-Fi used its cache when it first connected.
    wifi_cache: CacheUse,
    /// When Wi-Fi was up, in ms from the start.
    wifi_ms: u32,
}

/// Publish a cycle's readings and its diagnostics, once the broker has
/// accepted the connection, and wait for the broker to acknowledge them.
fn publish(
    mqtt: &mut mqtt::Mqtt,
    cycle: &Cycle,
    scan: ble::Scan,
    error: Option<String>,
) -> anyhow::Result<()> {
    let mqtt_ms = ms_between(cycle.start, mqtt.wait_connected(MQTT_CONNECT_TIMEOUT)?);

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

    let ap = system::ap_record();
    let diag = Diagnostics {
        firmware: concat!(env!("CARGO_PKG_NAME"), " ", env!("BUILD_VERSION")),
        client_id: config::MQTT_CLIENT_ID,
        chip: system::chip(),
        cycle: cycle.number,
        settings: Settings {
            scan_mode: config::BLE_SCAN_MODE,
            scan_duration: config::BLE_SCAN_DURATION.unsigned_abs(),
            antenna: config::ANTENNA,
        },
        wifi_rssi: ap.map(|ap| ap.rssi),
        wifi_channel: ap.map(|ap| ap.primary),
        wifi_bssid: ap.map(|ap| Mac::from(ap.bssid)),
        wifi_cache: cycle.wifi_cache,
        free_heap: system::free_heap(),
        scan_start_ms: scan.started_ms,
        scan_end_ms: scan.finished_ms,
        wifi_ms: cycle.wifi_ms,
        mqtt_ms,
        cycle_ms: ms_since(cycle.start),
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

    mqtt.wait_published(MQTT_ACK_TIMEOUT)
}

fn spawn_scan(start: Instant) -> anyhow::Result<JoinHandle<anyhow::Result<ble::Scan>>> {
    thread::Builder::new()
        .stack_size(4096)
        .spawn(move || ble::scan(start))
        .context("Failed to spawn BLE scan thread")
}

/// The BLE scan thread, or its result.
enum ScanThread {
    Running(JoinHandle<anyhow::Result<ble::Scan>>),
    Finished((ble::Scan, Option<String>)),
}

impl ScanThread {
    fn finish(self, start: Instant) -> (ble::Scan, Option<String>) {
        match self {
            Self::Running(handle) => finish_scan(handle, start),
            Self::Finished(result) => result,
        }
    }
}

/// Wait for the scan to finish.
fn finish_scan(
    handle: JoinHandle<anyhow::Result<ble::Scan>>,
    start: Instant,
) -> (ble::Scan, Option<String>) {
    match handle.join() {
        Ok(result) => scan_result(result, start),
        Err(_) => failed_scan("BLE scan thread panicked".to_string(), start),
    }
}

/// A failed scan is still reported in the diagnostics, so it returns the
/// error instead.
fn scan_result(result: anyhow::Result<ble::Scan>, start: Instant) -> (ble::Scan, Option<String>) {
    match result {
        Ok(scan) => (scan, None),
        Err(e) => failed_scan(format!("BLE scan failed: {e:#}"), start),
    }
}

fn failed_scan(error: String, start: Instant) -> (ble::Scan, Option<String>) {
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
