use std::collections::HashMap;

use anyhow::{Context, Result};
use btleplug::api::{
    Central, CentralEvent, CentralState, Manager as _, Peripheral as _, PeripheralProperties,
    ScanFilter,
};
use btleplug::platform::{Adapter, Manager, Peripheral, PeripheralId};
use futures::stream::StreamExt;
use ruuvi_sensor_protocol::{MacAddress, SensorValues};
use tokio::time::{Duration, sleep};

use crate::Event::{BleStopped, RuuviUpdate};
use crate::EventSender;
use crate::ruuvi::SensorData;
use crate::ruuvi::delay::{publish_delay, random_jitter};

#[derive(Clone)]
pub struct RuuviListener {
    central: Adapter,
    tx: EventSender,
}

impl RuuviListener {
    pub async fn new(tx: EventSender) -> Result<Self> {
        let manager = Manager::new().await?;

        // get the first bluetooth adapter
        let adapters = manager.adapters().await?;
        let central = adapters
            .into_iter()
            .next()
            .context("No Bluetooth adapters found")?;

        Ok(Self { central, tx })
    }

    pub async fn start(self) -> Result<()> {
        let mut events = self.central.events().await?;

        log::info!(
            "Starting BLE scan on {}...",
            self.central.adapter_info().await?
        );
        self.central.start_scan(ScanFilter::default()).await?;

        // Start BLE event loop
        tokio::spawn(async move {
            while let Some(event) = events.next().await {
                // BlueZ powers the adapter off before it removes it, e.g. when
                // the controller disconnects or BlueZ restarts. It doesn't
                // restart our discovery when the adapter comes back.
                if let CentralEvent::StateUpdate(CentralState::PoweredOff) = event {
                    log::warn!("Bluetooth adapter powered off");
                    break;
                }
                let ruuvi = self.clone();
                tokio::spawn(async move {
                    log::trace!("BLE event: {event:?}");
                    if let Err(err) = ruuvi.on_event(event).await {
                        log::error!("Failed to handle BLE event: {err:?}");
                    }
                });
            }
            // On BlueZ, the stream itself ends only with the D-Bus connection.
            // Tell main() so the daemon exits instead of running idle.
            if self.tx.send(BleStopped).await.is_err() {
                log::error!("BLE scanning stopped and main loop is gone");
            }
        });

        Ok(())
    }

    /// `BlueZ` reports a device's first advertisement with the device object
    /// (`DeviceDiscovered`), and later ones as `ManufacturerDataAdvertisement`
    /// when the payload changes. Each reading comes from exactly one of them.
    /// btleplug also reports each device `BlueZ` already knows as
    /// `DeviceDiscovered` when the event stream starts, see
    /// [`discovered_reading`].
    ///
    /// `RssiUpdate` is not a reading. `BlueZ` after 5.86 sends it only when
    /// the RSSI changes by 8 dB or more. Older versions send it on every
    /// advertisement, in the same signal as the manufacturer data, so
    /// handling it too would send each reading twice. The btleplug `BlueZ`
    /// backend never emits `DeviceUpdated`.
    async fn on_event(self, event: CentralEvent) -> Result<()> {
        match event {
            CentralEvent::DeviceDiscovered(id) => {
                let peripheral = self.find_peripheral(&id).await?;
                let Some(properties) = peripheral.properties().await? else {
                    return Ok(());
                };
                if let Some((values, rssi)) = discovered_reading(properties) {
                    self.send_reading(&peripheral, values, Some(rssi)).await?;
                }
            }
            CentralEvent::ManufacturerDataAdvertisement {
                id,
                manufacturer_data,
            } => {
                // Parse first to skip the D-Bus lookups for other devices.
                if let Some(values) = parse_values(manufacturer_data) {
                    let peripheral = self.find_peripheral(&id).await?;
                    let rssi = peripheral.properties().await?.and_then(|p| p.rssi);
                    self.send_reading(&peripheral, values, rssi).await?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// `rssi` is the last value `BlueZ` reported. After 5.86 it can lag the
    /// current signal by up to 7 dB, which shifts the delay by up to 70 ms.
    async fn send_reading(
        &self,
        peripheral: &Peripheral,
        values: SensorValues,
        rssi: Option<i16>,
    ) -> Result<()> {
        log::trace!("BLE Peripheral: {peripheral:?}");
        log::trace!("Ruuvi event: {values:?}");
        // Only data format 5 carries the MAC in the payload.
        // Older formats use the advertiser address.
        let address = values
            .mac_address()
            .map_or_else(|| peripheral.address(), Into::into);
        let delay = publish_delay(rssi, random_jitter());
        log::debug!(
            "Reading from [{address}]: RSSI {}, delay {} ms",
            rssi.map_or_else(|| "unknown".to_string(), |r| format!("{r} dBm")),
            delay.as_millis(),
        );
        send_delayed(&self.tx, SensorData::new(address, values), delay).await
    }

    async fn find_peripheral(&self, id: &PeripheralId) -> Result<Peripheral> {
        self.central
            .peripheral(id)
            .await
            .context("Failed to find peripheral")
    }
}

fn parse_values(manufacturer_data: HashMap<u16, Vec<u8>>) -> Option<SensorValues> {
    manufacturer_data
        .into_iter()
        .find_map(|(id, data)| SensorValues::from_manufacturer_specific_data(id, data).ok())
}

/// A device `BlueZ` cached from an earlier scan has the payload it last saw,
/// which may be stale. `BlueZ` clears the RSSI when discovery stops, so a
/// device without RSSI hasn't been heard since. Its next advertisement
/// arrives as `ManufacturerDataAdvertisement` within about a second.
fn discovered_reading(properties: PeripheralProperties) -> Option<(SensorValues, i16)> {
    let values = parse_values(properties.manufacturer_data)?;
    let Some(rssi) = properties.rssi else {
        log::debug!("Skipping cached reading from [{}]", properties.address);
        return None;
    };
    Some((values, rssi))
}

/// The main loop checks the throttle only when the reading arrives, so a
/// reading that waited is throttled if another instance published meanwhile.
async fn send_delayed(tx: &EventSender, data: SensorData, delay: Duration) -> Result<()> {
    sleep(delay).await;
    tx.send(RuuviUpdate(data)).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use tokio::sync::mpsc;

    use super::*;
    use crate::Event::MqttDeviceUpdate;
    use crate::devices::{Devices, ThrottleResult};
    use crate::ruuvi::BDAddr;

    fn sensor(address: BDAddr) -> SensorData {
        // Data format 3 has no MAC in the payload.
        let payload = [3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let values = SensorValues::from_manufacturer_specific_data(0x0499, payload).unwrap();
        SensorData::new(address, values)
    }

    #[test]
    fn parses_ruuvi_manufacturer_data() {
        // Data format 3 with an unrelated Apple entry next to it.
        let data = HashMap::from([
            (0x004c, vec![0x02, 0x15]),
            (0x0499, vec![3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
        ]);
        assert!(parse_values(data).is_some());
    }

    fn ruuvi_properties(rssi: Option<i16>) -> PeripheralProperties {
        PeripheralProperties {
            rssi,
            manufacturer_data: HashMap::from([(
                0x0499,
                vec![3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            )]),
            ..PeripheralProperties::default()
        }
    }

    #[test]
    fn reads_discovered_device_with_rssi() {
        let reading = discovered_reading(ruuvi_properties(Some(-70)));
        assert_eq!(reading.map(|(_, rssi)| rssi), Some(-70));
    }

    #[test]
    fn skips_cached_device_without_rssi() {
        assert!(discovered_reading(ruuvi_properties(None)).is_none());
    }

    #[test]
    fn ignores_other_manufacturer_data() {
        let data = HashMap::from([(0x004c, vec![0x02, 0x15])]);
        assert!(parse_values(data).is_none());
    }

    #[tokio::test]
    async fn reading_is_throttled_if_another_instance_publishes_during_delay() {
        let address = BDAddr::from([1, 2, 3, 4, 5, 6]);
        let mut devices = Devices::new(&HashMap::from([(address, "tag")]), Duration::from_mins(1));
        let (tx, mut rx) = mpsc::channel(4);

        let delayed = tokio::spawn({
            let tx = tx.clone();
            async move { send_delayed(&tx, sensor(address), Duration::from_millis(50)).await }
        });
        // Another instance's state message arrives while the reading waits.
        tx.send(MqttDeviceUpdate(address)).await.unwrap();

        // Handle events in arrival order, as main() does.
        let Some(MqttDeviceUpdate(bdaddr)) = rx.recv().await else {
            panic!("the reading arrived before the delay ended");
        };
        devices.mark_published(&bdaddr);
        let Some(RuuviUpdate(data)) = rx.recv().await else {
            panic!("expected the reading");
        };
        assert_eq!(devices.try_publish(&data.bdaddr), ThrottleResult::Throttle);
        delayed.await.unwrap().unwrap();
    }
}
