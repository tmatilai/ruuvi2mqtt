use std::collections::HashMap;
use std::time::{Duration, Instant};
use std::{clone::Clone, cmp::Eq, hash::Hash};

pub struct Devices<K, V> {
    devices: HashMap<K, DeviceData<V>>,
    throttle: Duration,
}

struct DeviceData<T> {
    data: T,
    last_updated: Option<Instant>,
}

#[derive(Debug)]
pub enum TryUpdate<T> {
    Update(T),
    Throttle(T),
    UnknownDevice,
}

impl<K, V> Devices<K, V>
where
    K: Eq + Copy + Hash,
    V: Clone,
{
    pub fn new(devices: &HashMap<K, V>, throttle: Duration) -> Self {
        let devices = devices
            .iter()
            .map(|(id, data)| (*id, DeviceData::new(data)))
            .collect();
        Self { devices, throttle }
    }

    pub fn try_update(&mut self, device_id: &K) -> TryUpdate<V> {
        match self.devices.get(device_id) {
            None => TryUpdate::UnknownDevice,
            Some(device) => {
                let device_data = device.data();
                match device.last_updated() {
                    // Throttling disabled, don't set the timestamp
                    None if self.throttle.is_zero() => TryUpdate::Update(device_data),
                    // If timestamp is set, the interval can't be None
                    Some(last_updated) if last_updated.elapsed() < self.throttle => {
                        TryUpdate::Throttle(device_data)
                    }
                    _ => TryUpdate::Update(device_data),
                }
            }
        }
    }

    pub fn update(&mut self, device_id: &K) -> Option<V> {
        if self.throttle.is_zero() {
        } else if let Some(device) = self.devices.get_mut(device_id) {
            device.update();
            return Some(device.data());
        }
        None
    }
}

impl<T: std::clone::Clone> DeviceData<T> {
    pub fn new(data: &T) -> Self {
        Self {
            data: data.clone(),
            last_updated: None,
        }
    }

    pub fn last_updated(&self) -> Option<&Instant> {
        self.last_updated.as_ref()
    }

    pub fn update(&mut self) {
        self.last_updated = Some(Instant::now());
    }

    pub fn data(&self) -> T {
        self.data.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_devices(keys: &[u32], throttle: Duration) -> Devices<u32, String> {
        let map: HashMap<u32, String> = keys.iter().map(|k| (*k, format!("data-{k}"))).collect();
        Devices::new(&map, throttle)
    }

    #[test]
    fn unknown_device_returns_unknown() {
        let mut devs = make_devices(&[1], Duration::from_secs(60));
        assert!(matches!(devs.try_update(&2), TryUpdate::UnknownDevice));
    }

    #[test]
    fn first_try_update_returns_update() {
        let mut devs = make_devices(&[1], Duration::from_secs(60));
        let result = devs.try_update(&1);
        assert!(matches!(result, TryUpdate::Update(ref v) if v == "data-1"));
    }

    #[test]
    fn try_update_without_timestamp_always_returns_update() {
        let mut devs = make_devices(&[1], Duration::from_secs(60));
        // try_update doesn't set the timestamp, so repeated calls still return Update
        assert!(matches!(devs.try_update(&1), TryUpdate::Update(_)));
        assert!(matches!(devs.try_update(&1), TryUpdate::Update(_)));
    }

    #[test]
    fn update_sets_timestamp_and_try_update_throttles() {
        let mut devs = make_devices(&[1], Duration::from_secs(60));
        // Simulate MQTT feedback: update() sets the timestamp
        devs.update(&1);
        // Now try_update should throttle
        assert!(matches!(devs.try_update(&1), TryUpdate::Throttle(_)));
    }

    #[test]
    fn throttle_expires_after_duration() {
        let mut devs = make_devices(&[1], Duration::from_millis(10));
        devs.update(&1);
        assert!(matches!(devs.try_update(&1), TryUpdate::Throttle(_)));

        std::thread::sleep(Duration::from_millis(15));
        assert!(matches!(devs.try_update(&1), TryUpdate::Update(_)));
    }

    #[test]
    fn zero_throttle_always_returns_update() {
        let mut devs = make_devices(&[1], Duration::ZERO);
        // With zero throttle, try_update always returns Update
        assert!(matches!(devs.try_update(&1), TryUpdate::Update(_)));
        assert!(matches!(devs.try_update(&1), TryUpdate::Update(_)));
    }

    #[test]
    fn zero_throttle_update_is_noop() {
        let mut devs = make_devices(&[1], Duration::ZERO);
        // update() with zero throttle doesn't set timestamp
        assert!(devs.update(&1).is_none());
        // Still returns Update
        assert!(matches!(devs.try_update(&1), TryUpdate::Update(_)));
    }

    #[test]
    fn update_unknown_device_returns_none() {
        let mut devs = make_devices(&[1], Duration::from_secs(60));
        assert!(devs.update(&2).is_none());
    }

    #[test]
    fn update_returns_device_data() {
        let mut devs = make_devices(&[1], Duration::from_secs(60));
        let result = devs.update(&1);
        assert_eq!(result.as_deref(), Some("data-1"));
    }

    #[test]
    fn cross_instance_throttle_flow() {
        // Simulates: BLE scan → try_update(Update) → publish → MQTT arrives → update() → BLE scan → try_update(Throttle)
        let mut devs = make_devices(&[1], Duration::from_secs(60));

        // First BLE reading passes through
        assert!(matches!(devs.try_update(&1), TryUpdate::Update(_)));

        // MQTT message arrives (from self or another instance), setting the timestamp
        devs.update(&1);

        // Next BLE reading is throttled
        assert!(matches!(devs.try_update(&1), TryUpdate::Throttle(_)));
    }
}
