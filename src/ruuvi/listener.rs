use anyhow::{Context, Result};
use btleplug::api::{Central, CentralEvent, Manager as _, Peripheral as _, ScanFilter};
use btleplug::platform::{Adapter, Manager, Peripheral, PeripheralId};
use futures::stream::StreamExt;
use rand::Rng;
use ruuvi_sensor_protocol::{MacAddress, SensorValues};
use tokio::time::{sleep, Duration};

use crate::ruuvi::SensorData;
use crate::Event::RuuviUpdate;
use crate::EventSender;

#[derive(Clone)]
pub struct RuuviListener {
    central: Adapter,
    tx: EventSender,
    sleep: Duration,
}

impl RuuviListener {
    pub async fn new(tx: EventSender, sleep: Duration) -> Result<Self> {
        let manager = Manager::new().await?;

        // get the first bluetooth adapter
        let adapters = manager.adapters().await?;
        let central = adapters
            .into_iter()
            .next()
            .context("No Bluetooth adapters found")?;

        let sleep = if sleep.is_zero() {
            Duration::from_millis(rand::rng().random_range(0..500))
        } else {
            sleep
        };
        Ok(Self { central, tx, sleep })
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
                let ruuvi = self.clone();
                tokio::spawn(async move {
                    log::trace!("BLE event: {:?}", event);
                    if let Err(err) = ruuvi.on_event(event).await {
                        log::error!("Failed to handle BLE event: {:?}", err);
                    }
                });
            }
        });

        Ok(())
    }

    async fn on_event(self, event: CentralEvent) -> Result<()> {
        match event {
            CentralEvent::DeviceDiscovered(id) | CentralEvent::DeviceUpdated(id) => {
                let peripheral = self.find_peripheral(&id).await?;
                log::trace!("BLE Peripheral: {:?}", peripheral);
                if let Some(values) = Self::parse_data(&peripheral).await? {
                    log::trace!("Ruuvi event: {:?}", values);
                    let address = values
                        .mac_address()
                        .context(format!("BDAddr not found: {peripheral:?}"))?;
                    let data = SensorData::new(address.into(), values);
                    // Sleep a bit to avoid multiple/simultaneus updates
                    sleep(self.sleep).await;
                    self.tx.send(RuuviUpdate(data)).await?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    async fn find_peripheral(&self, id: &PeripheralId) -> Result<Peripheral> {
        self.central
            .peripheral(id)
            .await
            .context("Failed to find peripheral")
    }

    async fn parse_data(peripheral: &Peripheral) -> Result<Option<SensorValues>> {
        Ok(peripheral
            .properties()
            .await?
            .unwrap()
            .manufacturer_data
            .into_iter()
            .find_map(|(id, data)| SensorValues::from_manufacturer_specific_data(id, data).ok()))
    }
}
