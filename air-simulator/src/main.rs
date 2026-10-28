//! Runs a Ruuvi Air's advertising sets, as the README lists them.

use std::{thread, time::Duration};

use air_simulator::{
    Reading, Sets, extended_adv_data, format_mac, legacy_adv_data, name, parse_mac, static_random,
};
use anyhow::{Context, Result, bail};
use esp_idf_svc::sys;
use esp32_nimble::{
    BLEAddress, BLEAddressType, BLEDevice, BLEExtAdvertisement,
    enums::{PrimPhy, SecPhy},
};
use log::{info, warn};

mod antenna;
mod led;

const LEGACY: u8 = 0;
const EXTENDED: u8 = 1;
const CODED: u8 = 2;

/// The Air's advertising intervals, in 0.625 ms units: 211.25–318.75 ms,
/// and 800–1000 ms on the Coded PHY.
const INTERVAL: (u32, u32) = (338, 510);
const CODED_INTERVAL: (u32, u32) = (1280, 1600);

const PERIOD: Duration = Duration::from_secs(1);

fn main() -> Result<()> {
    sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();
    led::start()?;
    antenna::select()?;

    let mac = match option_env!("AIR_MAC") {
        Some(text) => parse_mac(text).map_err(anyhow::Error::msg)?,
        None => static_random(bt_mac()?),
    };
    let sets = match option_env!("AIR_SETS") {
        Some(list) => Sets::parse(list).map_err(anyhow::Error::msg)?,
        None => Sets::ALL,
    };
    let address = BLEAddress::from_be_bytes(mac, BLEAddressType::Random);

    let device = BLEDevice::take();
    let reading = Reading::simulated(0);
    let mut running = Sets::NONE;
    {
        let mut advertising = device.get_advertising().lock();

        if sets.legacy {
            let mut adv = BLEExtAdvertisement::new(PrimPhy::Phy1M, SecPhy::Phy1M);
            adv.legacy_advertising(true);
            adv.scannable(true);
            configure(&mut adv, &address, INTERVAL);
            advertising.set_instance_data(LEGACY, &mut adv)?;
            let mut response = BLEExtAdvertisement::new(PrimPhy::Phy1M, SecPhy::Phy1M);
            response.name(&name(&mac));
            advertising.set_scan_response_data(LEGACY, &response)?;
            set_data(LEGACY, &legacy_adv_data(&reading, &mac))?;
            advertising.start(LEGACY)?;
            running.legacy = true;
        }

        if sets.extended {
            let mut adv = BLEExtAdvertisement::new(PrimPhy::Phy1M, SecPhy::Phy2M);
            configure(&mut adv, &address, INTERVAL);
            advertising.set_instance_data(EXTENDED, &mut adv)?;
            set_data(EXTENDED, &extended_adv_data(&reading, &mac))?;
            advertising.start(EXTENDED)?;
            running.extended = true;
        }

        if sets.coded {
            let mut adv = BLEExtAdvertisement::new(PrimPhy::Coded, SecPhy::Coded);
            configure(&mut adv, &address, CODED_INTERVAL);
            // Not every chip has the Coded PHY. The other sets still work.
            let started = advertising
                .set_instance_data(CODED, &mut adv)
                .map_err(anyhow::Error::from)
                .and_then(|()| set_data(CODED, &extended_adv_data(&reading, &mac)))
                .and_then(|()| advertising.start(CODED).map_err(anyhow::Error::from));
            match started {
                Ok(()) => running.coded = true,
                Err(e) => warn!("No Coded PHY advertising: {e:#}"),
            }
        }
    }
    info!(
        "Advertising as {} ({}): {running:?}",
        format_mac(&mac),
        name(&mac)
    );

    let mut sequence: u32 = 0;
    loop {
        thread::sleep(PERIOD);
        sequence = (sequence + 1) & 0x00FF_FFFF;
        let reading = Reading::simulated(sequence);
        if let Err(e) = update(running, &reading, &mac) {
            warn!("Updating sequence {sequence} failed: {e:#}");
        }
        if sequence % 60 == 0 {
            info!("Sequence {sequence}");
        }
    }
}

/// Settings all three sets share. The Air's extended set is connectable,
/// but the simulator has nothing to connect to.
fn configure(adv: &mut BLEExtAdvertisement, address: &BLEAddress, interval: (u32, u32)) {
    adv.address(address);
    adv.connectable(false);
    adv.min_interval(interval.0);
    adv.max_interval(interval.1);
}

fn update(running: Sets, reading: &Reading, mac: &[u8; 6]) -> Result<()> {
    if running.legacy {
        set_data(LEGACY, &legacy_adv_data(reading, mac))?;
    }
    let extended = extended_adv_data(reading, mac);
    if running.extended {
        set_data(EXTENDED, &extended)?;
    }
    if running.coded {
        set_data(CODED, &extended)?;
    }
    Ok(())
}

/// Replaces an instance's advertising data, also while it advertises.
/// esp32-nimble can only set data while it configures an instance.
fn set_data(instance: u8, data: &[u8]) -> Result<()> {
    let len = u16::try_from(data.len())?;
    // SAFETY: ble_hs_mbuf_from_flat copies the data. ble_gap_ext_adv_set_data
    // takes the mbuf and frees it, also on failure.
    let rc = unsafe {
        let buf = sys::ble_hs_mbuf_from_flat(data.as_ptr().cast(), len);
        if buf.is_null() {
            bail!("no mbuf for {len} bytes");
        }
        sys::ble_gap_ext_adv_set_data(instance, buf)
    };
    if rc != 0 {
        bail!("ble_gap_ext_adv_set_data({instance}) failed with {rc}");
    }
    Ok(())
}

/// The chip's Bluetooth MAC, read from the eFuse.
fn bt_mac() -> Result<[u8; 6]> {
    let mut mac = [0; 6];
    // SAFETY: esp_read_mac writes 6 bytes for the BT MAC type.
    sys::esp!(unsafe { sys::esp_read_mac(mac.as_mut_ptr(), sys::esp_mac_type_t_ESP_MAC_BT) })
        .context("reading the Bluetooth MAC")?;
    Ok(mac)
}
