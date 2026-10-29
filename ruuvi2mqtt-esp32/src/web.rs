//! The status page on `http://<hostname>.local/`, for finding a board's
//! spot, and `/api/status`, its JSON.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;

use anyhow::Context;
use esp_idf_svc::{
    http::{
        server::{Configuration, EspHttpServer},
        Method,
    },
    io::Write,
    mdns::EspMdns,
    sys,
};
use log::info;
use ruuvi2mqtt_esp32::mac::Mac;
use ruuvi2mqtt_esp32::status::{self, Board, Live};

use crate::{config, system};

static LIVE: Mutex<Live> = Mutex::new(Live::new());

/// Since boot, for the page's grades.
static WIFI_DROPS: AtomicU32 = AtomicU32::new(0);
static CYCLES: AtomicU32 = AtomicU32::new(0);
static CYCLES_FAILED: AtomicU32 = AtomicU32::new(0);
static LAST_CYCLE_FAILED: AtomicBool = AtomicBool::new(false);

/// Counts a Wi-Fi drop.
pub fn wifi_dropped() {
    WIFI_DROPS.fetch_add(1, Ordering::Relaxed);
}

/// Counts a cycle that published, or failed to.
pub fn cycle_done(failed: bool) {
    CYCLES.fetch_add(1, Ordering::Relaxed);
    if failed {
        CYCLES_FAILED.fetch_add(1, Ordering::Relaxed);
    }
    LAST_CYCLE_FAILED.store(failed, Ordering::Relaxed);
}

static PAGE: &str = include_str!("status.html");

/// Records a Ruuvi advertisement for the page.
pub fn record(mac: Mac, rssi: i8) {
    LIVE.lock().unwrap().record(mac, rssi, uptime_ms());
}

/// Handles a message on [`status::DISCOVERY_TOPIC`].
pub fn discovery(topic: &str, payload: &[u8]) {
    if let Some((mac, name)) = status::discovery_name(topic, payload) {
        LIVE.lock().unwrap().set_name(mac, name);
    }
}

/// The server and the mDNS name. Dropping them stops the page.
pub struct Web {
    _server: EspHttpServer<'static>,
    _mdns: EspMdns,
}

/// Serve the page, once Wi-Fi is up.
pub fn start() -> anyhow::Result<Web> {
    let mut mdns = EspMdns::take()?;
    mdns.set_hostname(config::DEVICE_HOSTNAME)?;
    mdns.set_instance_name(config::DEVICE_HOSTNAME)?;
    mdns.add_service(None, "_http", "_tcp", 80, &[])?;

    let mut server = EspHttpServer::new(&Configuration::default())?;
    server.fn_handler::<anyhow::Error, _>("/", Method::Get, |req| {
        req.into_response(200, None, &[("Content-Type", "text/html; charset=utf-8")])?
            .write_all(PAGE.as_bytes())?;
        Ok(())
    })?;
    server.fn_handler::<anyhow::Error, _>("/api/status", Method::Get, |req| {
        let json = report(&status::targets(req.uri()))?;
        req.into_response(
            200,
            None,
            &[
                ("Content-Type", "application/json"),
                ("Cache-Control", "no-store"),
            ],
        )?
        .write_all(json.as_bytes())?;
        Ok(())
    })?;

    info!("Status page: http://{}.local/", config::DEVICE_HOSTNAME);
    Ok(Web {
        _server: server,
        _mdns: mdns,
    })
}

fn report(targets: &[Mac]) -> anyhow::Result<String> {
    let board = Board {
        hostname: config::DEVICE_HOSTNAME,
        firmware: concat!(env!("CARGO_PKG_NAME"), " ", env!("BUILD_VERSION")),
        antenna: config::ANTENNA,
        uptime_ms: uptime_ms(),
        // The request came over Wi-Fi.
        wifi: system::ap_info().context("Wi-Fi is down")?,
        wifi_drops: WIFI_DROPS.load(Ordering::Relaxed),
        cycles: CYCLES.load(Ordering::Relaxed),
        cycles_failed: CYCLES_FAILED.load(Ordering::Relaxed),
        last_cycle_failed: LAST_CYCLE_FAILED.load(Ordering::Relaxed),
    };
    let report = LIVE.lock().unwrap().report(&board, targets);
    Ok(serde_json::to_string(&report)?)
}

fn uptime_ms() -> u64 {
    u64::try_from(unsafe { sys::esp_timer_get_time() } / 1000).unwrap_or(0)
}
