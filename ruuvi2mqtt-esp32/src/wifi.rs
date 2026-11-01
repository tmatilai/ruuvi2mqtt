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
use ruuvi2mqtt_esp32::roam::{self, Decision};
use std::ffi::CString;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::{ble, config, led, system};

/// Connect to Wi-Fi and return the `BlockingWifi` guard, and how the NVS
/// cache was used.
///
/// The guard keeps the Wi-Fi driver alive. Dropping it disconnects.
pub fn connect(
    modem: Modem<'static>,
    sysloop: EspSystemEventLoop,
    nvs: EspDefaultNvsPartition,
) -> anyhow::Result<(BlockingWifi<EspWifi<'static>>, CacheUse)> {
    let esp_wifi = EspWifi::new(modem, sysloop.clone(), Some(nvs))?;

    let hostname =
        CString::new(config::DEVICE_HOSTNAME).expect("DEVICE_HOSTNAME contains a null byte");
    if let Err(e) = unsafe {
        sys::esp!(sys::esp_netif_set_hostname(
            esp_wifi.sta_netif().handle(),
            hostname.as_ptr()
        ))
    } {
        log::warn!("Could not set hostname: {e}");
    }

    if let Some(ip) = config::WIFI_IP {
        configure_static_ip(esp_wifi.sta_netif().handle(), ip)?;
    }

    // For connect_cached, which watches for disconnects.
    let events = sysloop.clone();
    let mut wifi = BlockingWifi::wrap(esp_wifi, sysloop)?;

    let cached = load_wifi_cache();
    if let Some(cache) = &cached {
        info!(
            "Fast reconnect: channel {}, BSSID {}",
            cache.channel,
            Mac::from(cache.bssid),
        );
    } else {
        info!("No WiFi cache — full channel scan");
    }

    wifi.set_configuration(&client_configuration(cached.as_ref()))?;

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
            log::warn!("Fast reconnect failed ({e}), retrying with full scan");
            wifi.set_configuration(&client_configuration(None))?;
            wifi.connect()?;
            CacheUse::Stale
        }
        Err(e) => return Err(e),
    };

    if config::WIFI_IP.is_none() {
        wifi.wait_netif_up()?;
    }
    info!("Connected to Wi-Fi as '{}'", config::DEVICE_HOSTNAME);

    update_wifi_cache(cached.as_ref());

    Ok((wifi, cache_use))
}

/// The station's configuration. A cached access point pins the channel and
/// BSSID, and skips the full scan.
fn client_configuration(cache: Option<&WifiCache>) -> Configuration {
    Configuration::Client(ClientConfiguration {
        // config.rs checks both lengths at build time.
        ssid: config::WIFI_SSID.try_into().expect("WIFI_SSID fits"),
        password: config::WIFI_PASS
            .unwrap_or_default()
            .try_into()
            .expect("WIFI_PASS fits"),
        // The weakest security the driver accepts from an access point.
        auth_method: if config::WIFI_OPEN {
            AuthMethod::None
        } else {
            AuthMethod::WPA2Personal
        },
        channel: cache.map(|c| c.channel),
        bssid: cache.map(|c| c.bssid),
        scan_method: if cache.is_some() {
            ScanMethod::FastScan
        } else {
            ScanMethod::default()
        },
        ..Default::default()
    })
}

/// When Wi-Fi connected, while it is connected. Set by [`keep_connected`].
static CONNECTED_SINCE: Mutex<Option<Instant>> = Mutex::new(None);

/// When Wi-Fi connected, or `None` while it is down.
pub fn connected_since() -> Option<Instant> {
    *CONNECTED_SINCE.lock().unwrap()
}

/// Reconnect whenever Wi-Fi drops, and roam, for the continuous mode. Call
/// after [`connect`], and keep the subscription: dropping it stops both.
///
/// After a drop, the first attempt retries the access point the board was
/// on, which is fast. The next one picks the strongest, as that one may be
/// gone. A weak signal starts a scan for a stronger one
/// ([`scan_for_stronger`]).
pub fn keep_connected(
    sysloop: &EspSystemEventLoop,
) -> anyhow::Result<EspSubscription<'static, System>> {
    *CONNECTED_SINCE.lock().unwrap() = Some(Instant::now());
    ble::set_wifi_connected(true);
    led::set_wifi_down(false);
    enable_ipv6();

    // The event loop's task has a small stack, and a scan blocks.
    let (roam_tx, roam_rx) = mpsc::channel();
    thread::Builder::new().stack_size(4096).spawn(move || {
        // While the link is weak without a stronger access point.
        let mut rescan_at: Option<Instant> = None;
        loop {
            let event = match rescan_at {
                Some(at) => {
                    match roam_rx.recv_timeout(at.saturating_duration_since(Instant::now())) {
                        Ok(event) => event,
                        Err(mpsc::RecvTimeoutError::Timeout) => Roam::WeakSignal,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }
                None => match roam_rx.recv() {
                    Ok(event) => event,
                    Err(mpsc::RecvError) => break,
                },
            };
            match event {
                Roam::Connected => {
                    rescan_at = None;
                    // The driver falls back to a weak access point when the
                    // strongest refuses. Don't pin the next attempt, or the
                    // next boot, to it.
                    if let Some(ap) = system::ap_record().filter(|ap| ap.rssi >= roam::THRESHOLD) {
                        if let Err(e) = pin(ap.bssid, ap.primary) {
                            log::warn!("Could not pin the access point: {e}");
                        }
                        update_wifi_cache(load_wifi_cache().as_ref());
                    }
                    set_rssi_threshold(roam::THRESHOLD);
                }
                Roam::WeakSignal => {
                    let still_weak = match scan_for_stronger() {
                        Ok(still_weak) => still_weak,
                        Err(e) => {
                            log::warn!("Roaming failed: {e:#}");
                            true
                        }
                    };
                    rescan_at = still_weak.then(|| Instant::now() + roam::RESCAN);
                }
            }
        }
    })?;

    let subscription = sysloop.subscribe::<WifiEvent, _>({
        let roam_tx = roam_tx.clone();
        move |event| match event {
            WifiEvent::StaConnected(_) => {
                info!("Wi-Fi reconnected");
                *CONNECTED_SINCE.lock().unwrap() = Some(Instant::now());
                ble::set_wifi_connected(true);
                led::set_wifi_down(false);
                enable_ipv6();
                let _ = roam_tx.send(Roam::Connected);
            }
            WifiEvent::StaDisconnected(d) => {
                // Each failed attempt ends with another disconnect. The first
                // one is a drop, unless the board left on purpose.
                if CONNECTED_SINCE.lock().unwrap().take().is_some() {
                    if !ROAMING.swap(false, Ordering::Relaxed) {
                        // The station leaves itself only when Wi-Fi stops,
                        // for a restart.
                        if u32::from(d.reason()) == sys::wifi_err_reason_t_WIFI_REASON_ASSOC_LEAVE {
                            return;
                        }
                        log::warn!("Wi-Fi disconnected, reason {}", d.reason());
                        crate::web::wifi_dropped();
                    }
                } else if let Err(e) = unpin() {
                    log::warn!("Could not unpin the access point: {e}");
                }
                ble::set_wifi_connected(false);
                led::set_wifi_down(true);
                if let Err(e) = sys::esp!(unsafe { sys::esp_wifi_connect() }) {
                    log::warn!("Wi-Fi reconnect failed: {e}");
                }
            }
            WifiEvent::StaBssRssiLow => {
                let _ = roam_tx.send(Roam::WeakSignal);
            }
            _ => {}
        }
    })?;
    // Pin and arm the threshold as after a reconnect. A weak link raises
    // the event at once, so subscribe first.
    let _ = roam_tx.send(Roam::Connected);
    Ok(subscription)
}

/// Give the station an IPv6 address, so that mDNS answers an AAAA query. A
/// resolver asks for both addresses, and macOS waits 5 s for the missing
/// answer before it opens the status page. The link-local address is lost
/// on a disconnect. A router's advertisements add a global one.
fn enable_ipv6() {
    let netif = unsafe { sys::esp_netif_get_handle_from_ifkey(c"WIFI_STA_DEF".as_ptr()) };
    if let Err(e) = sys::esp!(unsafe { sys::esp_netif_create_ip6_linklocal(netif) }) {
        log::warn!("Could not enable IPv6: {e}");
    }
}

/// The events for the roaming thread of [`keep_connected`].
enum Roam {
    Connected,
    WeakSignal,
}

/// Set while the board leaves an access point on purpose, so that the
/// disconnect doesn't count as a drop.
static ROAMING: AtomicBool = AtomicBool::new(false);

/// Raise `StaBssRssiLow` once the signal falls below `rssi`.
fn set_rssi_threshold(rssi: i8) {
    if let Err(e) = sys::esp!(unsafe { sys::esp_wifi_set_rssi_threshold(rssi.into()) }) {
        log::warn!("Could not set the RSSI threshold: {e}");
    }
}

/// Let the next connect go straight to this access point. The config goes
/// to NVS flash, so it writes only a change.
fn pin(bssid: [u8; 6], channel: u8) -> Result<(), sys::EspError> {
    unsafe {
        let mut conf: sys::wifi_config_t = std::mem::zeroed();
        sys::esp!(sys::esp_wifi_get_config(
            sys::wifi_interface_t_WIFI_IF_STA,
            &raw mut conf
        ))?;
        let sta = &mut conf.sta;
        if sta.bssid_set
            && sta.bssid == bssid
            && sta.channel == channel
            && sta.scan_method == sys::wifi_scan_method_t_WIFI_FAST_SCAN
        {
            return Ok(());
        }
        sta.bssid_set = true;
        sta.bssid = bssid;
        sta.channel = channel;
        sta.scan_method = sys::wifi_scan_method_t_WIFI_FAST_SCAN;
        sys::esp!(sys::esp_wifi_set_config(
            sys::wifi_interface_t_WIFI_IF_STA,
            &raw mut conf
        ))
    }
}

/// Let the next connect pick the strongest access point of the network,
/// instead of the pinned one. See [`pin`].
fn unpin() -> Result<(), sys::EspError> {
    unsafe {
        let mut conf: sys::wifi_config_t = std::mem::zeroed();
        sys::esp!(sys::esp_wifi_get_config(
            sys::wifi_interface_t_WIFI_IF_STA,
            &raw mut conf
        ))?;
        let sta = &mut conf.sta;
        if !sta.bssid_set
            && sta.channel == 0
            && sta.scan_method == sys::wifi_scan_method_t_WIFI_ALL_CHANNEL_SCAN
            && sta.sort_method == sys::wifi_sort_method_t_WIFI_CONNECT_AP_BY_SIGNAL
        {
            return Ok(());
        }
        sta.bssid_set = false;
        sta.channel = 0;
        sta.scan_method = sys::wifi_scan_method_t_WIFI_ALL_CHANNEL_SCAN;
        sta.sort_method = sys::wifi_sort_method_t_WIFI_CONNECT_AP_BY_SIGNAL;
        sys::esp!(sys::esp_wifi_set_config(
            sys::wifi_interface_t_WIFI_IF_STA,
            &raw mut conf
        ))
    }
}

/// How many access points of the network a scan keeps.
const MAX_SCANNED: u16 = 16;

/// Scan the network's access points, and leave for a clearly stronger one.
/// Returns whether the link stays weak without one, so that a later scan
/// looks again.
fn scan_for_stronger() -> anyhow::Result<bool> {
    let Some(ap) = system::ap_record() else {
        return Ok(false);
    };
    let current = Mac::from(ap.bssid);

    let ssid = CString::new(config::WIFI_SSID)?;
    let scanned = unsafe {
        let mut scan: sys::wifi_scan_config_t = std::mem::zeroed();
        scan.ssid = ssid.as_ptr().cast_mut().cast();
        // Blocks this thread for about 5 s, and BLE hears little meanwhile.
        sys::esp!(sys::esp_wifi_scan_start(&raw const scan, true))?;
        let mut count = MAX_SCANNED;
        let mut records: Vec<sys::wifi_ap_record_t> =
            vec![std::mem::zeroed(); usize::from(MAX_SCANNED)];
        let read = sys::esp!(sys::esp_wifi_scan_get_ap_records(
            &raw mut count,
            records.as_mut_ptr()
        ));
        if read.is_err() {
            // Reading the records is what frees them.
            sys::esp_wifi_clear_ap_list();
        }
        read?;
        records.truncate(usize::from(count));
        records
            .iter()
            .map(|r| roam::Ap {
                bssid: Mac::from(r.bssid),
                channel: r.primary,
                rssi: r.rssi,
            })
            .collect::<Vec<_>>()
    };

    match roam::decide(current, ap.rssi, &scanned) {
        Decision::Switch(to) => {
            info!(
                "Roaming from {current} ({} dBm) to {} ({} dBm)",
                ap.rssi, to.bssid, to.rssi
            );
            // The reconnect in `keep_connected` goes straight to it.
            pin(to.bssid.0, to.channel)?;
            ROAMING.store(true, Ordering::Relaxed);
            if let Err(e) = sys::esp!(unsafe { sys::esp_wifi_disconnect() }) {
                ROAMING.store(false, Ordering::Relaxed);
                return Err(e.into());
            }
            Ok(false)
        }
        Decision::Stay { best } => {
            match best {
                Some(best) => info!(
                    "No access point clearly stronger than {current} ({} dBm), the best other at {best} dBm",
                    ap.rssi
                ),
                None => info!("No other access point than {current} ({} dBm)", ap.rssi),
            }
            // The signal may have recovered since the event.
            if ap.rssi >= roam::THRESHOLD {
                set_rssi_threshold(roam::THRESHOLD);
                return Ok(false);
            }
            Ok(true)
        }
    }
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
