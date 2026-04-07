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

#[derive(Debug, PartialEq, Eq)]
pub enum ThrottleResult {
    Update,
    Throttle,
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

    pub fn get(&self, device_id: &K) -> Option<&V> {
        self.devices.get(device_id).map(|d| &d.data)
    }

    pub fn should_publish(&self, device_id: &K) -> ThrottleResult {
        match self.devices.get(device_id) {
            None => ThrottleResult::UnknownDevice,
            Some(device) => match device.last_updated() {
                // Throttling disabled, don't set the timestamp
                None if self.throttle.is_zero() => ThrottleResult::Update,
                // If timestamp is set, the interval can't be None
                Some(last_updated) if last_updated.elapsed() < self.throttle => {
                    ThrottleResult::Throttle
                }
                _ => ThrottleResult::Update,
            },
        }
    }

    pub fn mark_published(&mut self, device_id: &K) -> Option<V> {
        if self.throttle.is_zero() {
            return None;
        }
        let device = self.devices.get_mut(device_id)?;
        device.mark_published();
        Some(device.data())
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

    pub fn mark_published(&mut self) {
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
        let devs = make_devices(&[1], Duration::from_secs(60));
        assert_eq!(devs.should_publish(&2), ThrottleResult::UnknownDevice);
    }

    #[test]
    fn get_returns_device_data() {
        let devs = make_devices(&[1], Duration::from_secs(60));
        assert_eq!(devs.get(&1).map(String::as_str), Some("data-1"));
    }

    #[test]
    fn get_unknown_device_returns_none() {
        let devs = make_devices(&[1], Duration::from_secs(60));
        assert_eq!(devs.get(&2), None);
    }

    #[test]
    fn first_should_publish_returns_update() {
        let devs = make_devices(&[1], Duration::from_secs(60));
        assert_eq!(devs.should_publish(&1), ThrottleResult::Update);
    }

    #[test]
    fn should_publish_without_timestamp_always_returns_update() {
        let devs = make_devices(&[1], Duration::from_secs(60));
        // should_publish doesn't set the timestamp, so repeated calls still return Update
        assert_eq!(devs.should_publish(&1), ThrottleResult::Update);
        assert_eq!(devs.should_publish(&1), ThrottleResult::Update);
    }

    #[test]
    fn mark_published_sets_timestamp_and_should_publish_throttles() {
        let mut devs = make_devices(&[1], Duration::from_secs(60));
        // Simulate MQTT feedback: mark_published() sets the timestamp
        devs.mark_published(&1);
        // Now should_publish should throttle
        assert_eq!(devs.should_publish(&1), ThrottleResult::Throttle);
    }

    #[test]
    fn throttle_expires_after_duration() {
        let mut devs = make_devices(&[1], Duration::from_millis(10));
        devs.mark_published(&1);
        assert_eq!(devs.should_publish(&1), ThrottleResult::Throttle);

        std::thread::sleep(Duration::from_millis(15));
        assert_eq!(devs.should_publish(&1), ThrottleResult::Update);
    }

    #[test]
    fn zero_throttle_always_returns_update() {
        let devs = make_devices(&[1], Duration::ZERO);
        // With zero throttle, should_publish always returns Update
        assert_eq!(devs.should_publish(&1), ThrottleResult::Update);
        assert_eq!(devs.should_publish(&1), ThrottleResult::Update);
    }

    #[test]
    fn zero_throttle_mark_published_is_noop() {
        let mut devs = make_devices(&[1], Duration::ZERO);
        // mark_published() with zero throttle doesn't set timestamp
        assert!(devs.mark_published(&1).is_none());
        // Still returns Update
        assert_eq!(devs.should_publish(&1), ThrottleResult::Update);
    }

    #[test]
    fn mark_published_unknown_device_returns_none() {
        let mut devs = make_devices(&[1], Duration::from_secs(60));
        assert!(devs.mark_published(&2).is_none());
    }

    #[test]
    fn mark_published_returns_device_data() {
        let mut devs = make_devices(&[1], Duration::from_secs(60));
        let result = devs.mark_published(&1);
        assert_eq!(result.as_deref(), Some("data-1"));
    }

    #[test]
    fn cross_instance_throttle_flow() {
        // Simulates: BLE scan → should_publish(Update) → publish → MQTT arrives → mark_published() → BLE scan → should_publish(Throttle)
        let mut devs = make_devices(&[1], Duration::from_secs(60));

        // First BLE reading passes through
        assert_eq!(devs.should_publish(&1), ThrottleResult::Update);

        // MQTT message arrives (from self or another instance), setting the timestamp
        devs.mark_published(&1);

        // Next BLE reading is throttled
        assert_eq!(devs.should_publish(&1), ThrottleResult::Throttle);
    }
}
