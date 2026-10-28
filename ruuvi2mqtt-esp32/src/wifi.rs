use esp_idf_svc::{
    eventloop::{EspSubscription, EspSystemEventLoop, System},
    hal::modem::Modem,
    handle::RawHandle,
    nvs::EspDefaultNvsPartition,
    sys,
    wifi::{
        AuthMethod, BlockingWifi, ClientConfiguration, Configuration, EspWifi, ScanMethod,
        WifiEvent,
    },
};
use log::info;
use ruuvi2mqtt_esp32::diagnostics::CacheUse;
use ruuvi2mqtt_esp32::mac::Mac;
use std::ffi::CString;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::{ble, config, led};

/// Connect to Wi-Fi and return the `BlockingWifi` guard, and how the NVS
/// cache was used.
///
/// Uses NVS-cached channel/BSSID for fast reconnect when available, falling
/// back to a full scan on first boot or when the cache is stale. Optionally
/// configures a static IP to skip DHCP.
///
/// The guard keeps the Wi-Fi driver alive.  Dropping it disconnects.
pub fn connect(
    modem: Modem<'static>,
    sysloop: EspSystemEventLoop,
    nvs: EspDefaultNvsPartition,
) -> anyhow::Result<(BlockingWifi<EspWifi<'static>>, CacheUse)> {
    let esp_wifi = EspWifi::new(modem, sysloop.clone(), Some(nvs))?;

    let hostname = CString::new(config::DEVICE_HOSTNAME)
        .expect("ESP32_DEVICE_HOSTNAME contains an unexpected null byte");
    if let Err(e) = unsafe {
        sys::esp!(sys::esp_netif_set_hostname(
            esp_wifi.sta_netif().handle(),
            hostname.as_ptr()
        ))
    } {
        log::warn!("Could not set hostname: {e}");
    }

    // ── Static IP ────────────────────────────────────────────────────────────
    if let Some(ip) = config::WIFI_IP {
        configure_static_ip(esp_wifi.sta_netif().handle(), ip)?;
    }

    // For connect_cached, which watches for disconnects.
    let events = sysloop.clone();
    let mut wifi = BlockingWifi::wrap(esp_wifi, sysloop)?;

    let auth = if config::WIFI_PASS.is_empty() {
        AuthMethod::None
    } else {
        AuthMethod::WPA2Personal
    };

    // ── Fast reconnect from NVS cache ────────────────────────────────────────
    let cached = load_wifi_cache();
    let (channel, bssid, scan_method) = if let Some(cache) = &cached {
        info!(
            "Fast reconnect: channel {}, BSSID {}",
            cache.channel,
            Mac::from(cache.bssid),
        );
        (Some(cache.channel), Some(cache.bssid), ScanMethod::FastScan)
    } else {
        info!("No WiFi cache — full channel scan");
        (None, None, ScanMethod::default())
    };

    wifi.set_configuration(&Configuration::Client(ClientConfiguration {
        ssid: config::WIFI_SSID
            .try_into()
            .expect("WIFI_SSID too long (max 32 chars)"),
        password: config::WIFI_PASS
            .try_into()
            .expect("WIFI_PASS too long (max 64 chars)"),
        auth_method: auth,
        channel,
        bssid,
        scan_method,
        ..Default::default()
    }))?;

    wifi.start()?;
    set_band()?;
    info!("Connecting to Wi-Fi '{}'...", config::WIFI_SSID);

    // A fast reconnect fails as soon as the driver gives up on the cached AP.
    // A full scan moves on to the next AP itself, so it waits for the result.
    let result = if cached.is_some() {
        connect_cached(&mut wifi, &events)
    } else {
        wifi.connect().map_err(Into::into)
    };
    let cache_use = match result {
        Ok(()) if cached.is_some() => CacheUse::Hit,
        Ok(()) => CacheUse::Miss,
        Err(e) if cached.is_some() => {
            // Cached channel/BSSID may be stale — retry with full scan.
            log::warn!("Fast reconnect failed ({e}), retrying with full scan");
            wifi.set_configuration(&Configuration::Client(ClientConfiguration {
                ssid: config::WIFI_SSID
                    .try_into()
                    .expect("WIFI_SSID too long (max 32 chars)"),
                password: config::WIFI_PASS
                    .try_into()
                    .expect("WIFI_PASS too long (max 64 chars)"),
                auth_method: auth,
                ..Default::default()
            }))?;
            wifi.connect()?;
            CacheUse::Stale
        }
        Err(e) => return Err(e),
    };

    if config::WIFI_IP.is_none() {
        wifi.wait_netif_up()?;
    }
    info!("Connected to Wi-Fi as '{}'", config::DEVICE_HOSTNAME);

    // ── Update NVS cache ─────────────────────────────────────────────────────
    update_wifi_cache(cached.as_ref());

    Ok((wifi, cache_use))
}

/// When Wi-Fi connected, while it is connected. Set by [`keep_connected`].
static CONNECTED_SINCE: Mutex<Option<Instant>> = Mutex::new(None);

/// When Wi-Fi connected, or `None` while it is down.
pub fn connected_since() -> Option<Instant> {
    *CONNECTED_SINCE.lock().unwrap()
}

/// Reconnect whenever Wi-Fi drops, for the continuous mode. Call after
/// [`connect`], and keep the subscription: dropping it stops the reconnects.
///
/// The driver keeps the cached BSSID, so after its access point is gone, the
/// board keeps trying that one. The cycle timeout restarts the board then,
/// and the full scan finds another.
pub fn keep_connected(
    sysloop: &EspSystemEventLoop,
) -> anyhow::Result<EspSubscription<'static, System>> {
    *CONNECTED_SINCE.lock().unwrap() = Some(Instant::now());
    ble::set_wifi_connected(true);
    led::set_wifi_down(false);
    let subscription = sysloop.subscribe::<WifiEvent, _>(|event| match event {
        WifiEvent::StaConnected(_) => {
            info!("Wi-Fi reconnected");
            *CONNECTED_SINCE.lock().unwrap() = Some(Instant::now());
            ble::set_wifi_connected(true);
            led::set_wifi_down(false);
        }
        WifiEvent::StaDisconnected(d) => {
            // Each failed attempt ends with another disconnect, so log only
            // the first.
            if CONNECTED_SINCE.lock().unwrap().take().is_some() {
                log::warn!("Wi-Fi disconnected, reason {}", d.reason());
            }
            ble::set_wifi_connected(false);
            led::set_wifi_down(true);
            if let Err(e) = sys::esp!(unsafe { sys::esp_wifi_connect() }) {
                log::warn!("Wi-Fi reconnect failed: {e}");
            }
        }
        _ => {}
    })?;
    Ok(subscription)
}

/// Restrict Wi-Fi to `WIFI_BAND`. The default band mode uses both.
fn set_band() -> anyhow::Result<()> {
    let mode = match config::WIFI_BAND {
        Some("2.4") => sys::wifi_band_mode_t_WIFI_BAND_MODE_2G_ONLY,
        Some(_) => sys::wifi_band_mode_t_WIFI_BAND_MODE_5G_ONLY,
        None => return Ok(()),
    };
    sys::esp!(unsafe { sys::esp_wifi_set_band_mode(mode) })?;
    info!("Wi-Fi band: {} GHz", config::WIFI_BAND.unwrap_or_default());
    Ok(())
}

/// How long to wait for the cached AP. `BlockingWifi` also waits 15 s.
const CACHED_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// Connect to the cached AP, but fail on the first disconnect.
///
/// `BlockingWifi::connect` waits out its whole timeout after the driver has
/// given up, e.g. when the cached AP isn't heard. That leaves too little of
/// the cycle for the full scan.
fn connect_cached(
    wifi: &mut BlockingWifi<EspWifi<'static>>,
    sysloop: &EspSystemEventLoop,
) -> anyhow::Result<()> {
    // The disconnect reason, or 0 while none was seen.
    let reason = Arc::new(AtomicU16::new(0));
    let reason_tx = reason.clone();
    let _subscription = sysloop.subscribe::<WifiEvent, _>(move |event| {
        if let WifiEvent::StaDisconnected(d) = event {
            reason_tx.store(d.reason().max(1), Ordering::Relaxed);
        }
    })?;

    let start = Instant::now();
    wifi.wifi_mut().connect()?;
    while !wifi.is_connected()? {
        match reason.load(Ordering::Relaxed) {
            0 if start.elapsed() < CACHED_CONNECT_TIMEOUT => {
                thread::sleep(Duration::from_millis(50));
            }
            0 => anyhow::bail!("timed out after {}s", CACHED_CONNECT_TIMEOUT.as_secs()),
            r => anyhow::bail!(
                "disconnected after {}ms, reason {r}",
                start.elapsed().as_millis()
            ),
        }
    }
    Ok(())
}

/// Configure static IP on the STA netif, bypassing DHCP.
fn configure_static_ip(netif: *mut sys::esp_netif_t, ip: &str) -> anyhow::Result<()> {
    let gateway = config::WIFI_GATEWAY.expect("WIFI_GATEWAY must be set when WIFI_IP is set");
    let netmask = config::WIFI_NETMASK.unwrap_or("255.255.255.0");
    let dns = config::WIFI_DNS.unwrap_or(gateway);

    let ip_info = sys::esp_netif_ip_info_t {
        ip: str_to_ip4(ip),
        netmask: str_to_ip4(netmask),
        gw: str_to_ip4(gateway),
    };

    let mut dns_info = sys::esp_netif_dns_info_t {
        ip: sys::esp_ip_addr_t {
            u_addr: sys::_ip_addr__bindgen_ty_1 {
                ip4: str_to_ip4(dns),
            },
            #[allow(clippy::cast_possible_truncation)] // ESP-IDF constant is always 0
            type_: sys::ESP_IPADDR_TYPE_V4 as u8,
        },
    };

    unsafe {
        sys::esp!(sys::esp_netif_dhcpc_stop(netif))?;
        sys::esp!(sys::esp_netif_set_ip_info(netif, &raw const ip_info))?;
        sys::esp!(sys::esp_netif_set_dns_info(
            netif,
            sys::esp_netif_dns_type_t_ESP_NETIF_DNS_MAIN,
            &raw mut dns_info,
        ))?;
    }

    info!("Static IP: {ip}, gateway: {gateway}, netmask: {netmask}, DNS: {dns}");
    Ok(())
}

/// Parse a dotted-decimal IPv4 string into an `esp_ip4_addr_t`.
fn str_to_ip4(s: &str) -> sys::esp_ip4_addr_t {
    let cstr = CString::new(s).expect("IP address contains null byte");
    sys::esp_ip4_addr_t {
        addr: unsafe { sys::esp_ip4addr_aton(cstr.as_ptr()) },
    }
}

// ---------------------------------------------------------------------------
// NVS WiFi cache — stores channel + BSSID for fast reconnect
// ---------------------------------------------------------------------------

struct WifiCache {
    channel: u8,
    bssid: [u8; 6],
}

/// Load cached Wi-Fi channel and BSSID from NVS.
fn load_wifi_cache() -> Option<WifiCache> {
    unsafe {
        let mut handle: sys::nvs_handle_t = 0;
        let ret = sys::nvs_open(
            c"wifi".as_ptr(),
            sys::nvs_open_mode_t_NVS_READONLY,
            &raw mut handle,
        );
        if ret != sys::ESP_OK {
            return None;
        }

        let mut channel: u8 = 0;
        let ret = sys::nvs_get_u8(handle, c"channel".as_ptr(), &raw mut channel);
        if ret != sys::ESP_OK {
            sys::nvs_close(handle);
            return None;
        }

        let mut bssid = [0u8; 6];
        let mut len: usize = 6;
        let ret = sys::nvs_get_blob(
            handle,
            c"bssid".as_ptr(),
            bssid.as_mut_ptr().cast(),
            &raw mut len,
        );
        sys::nvs_close(handle);

        if ret != sys::ESP_OK || len != 6 {
            return None;
        }

        Some(WifiCache { channel, bssid })
    }
}

/// Read current AP info and update NVS cache if channel or BSSID changed.
fn update_wifi_cache(cached: Option<&WifiCache>) {
    unsafe {
        let mut ap_info: sys::wifi_ap_record_t = std::mem::zeroed();
        if sys::esp_wifi_sta_get_ap_info(&raw mut ap_info) != sys::ESP_OK {
            log::warn!("Could not read AP info for WiFi cache");
            return;
        }

        let channel = ap_info.primary;
        let bssid = ap_info.bssid;

        if let Some(cached) = cached {
            if cached.channel == channel && cached.bssid == bssid {
                return;
            }
        }

        let mut handle: sys::nvs_handle_t = 0;
        if sys::nvs_open(
            c"wifi".as_ptr(),
            sys::nvs_open_mode_t_NVS_READWRITE,
            &raw mut handle,
        ) != sys::ESP_OK
        {
            log::warn!("Could not open NVS for WiFi cache write");
            return;
        }

        let result = sys::esp!(sys::nvs_set_u8(handle, c"channel".as_ptr(), channel))
            .and_then(|()| {
                sys::esp!(sys::nvs_set_blob(
                    handle,
                    c"bssid".as_ptr(),
                    bssid.as_ptr().cast(),
                    6
                ))
            })
            .and_then(|()| sys::esp!(sys::nvs_commit(handle)));
        sys::nvs_close(handle);

        match result {
            Ok(()) => info!(
                "WiFi cache updated: channel {channel}, BSSID {}",
                Mac::from(bssid),
            ),
            Err(e) => log::warn!("Could not write WiFi cache: {e}"),
        }
    }
}
