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

    /// Returns `Update` and starts the throttle timer, so a second reading
    /// that arrives before our own message comes back over MQTT is throttled.
    pub fn try_publish(&mut self, device_id: &K) -> ThrottleResult {
        let Some(device) = self.devices.get_mut(device_id) else {
            return ThrottleResult::UnknownDevice;
        };
        if self.throttle.is_zero() {
            return ThrottleResult::Update;
        }
        if device
            .last_updated()
            .is_some_and(|last_updated| last_updated.elapsed() < self.throttle)
        {
            return ThrottleResult::Throttle;
        }
        device.mark_published();
        ThrottleResult::Update
    }

    /// Restarts the throttle timer when a message for the device arrives over
    /// MQTT, from this instance or another.
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
        let mut devs = make_devices(&[1], Duration::from_mins(1));
        assert_eq!(devs.try_publish(&2), ThrottleResult::UnknownDevice);
    }

    #[test]
    fn get_returns_device_data() {
        let devs = make_devices(&[1], Duration::from_mins(1));
        assert_eq!(devs.get(&1).map(String::as_str), Some("data-1"));
    }

    #[test]
    fn get_unknown_device_returns_none() {
        let devs = make_devices(&[1], Duration::from_mins(1));
        assert_eq!(devs.get(&2), None);
    }

    #[test]
    fn first_try_publish_returns_update() {
        let mut devs = make_devices(&[1], Duration::from_mins(1));
        assert_eq!(devs.try_publish(&1), ThrottleResult::Update);
    }

    #[test]
    fn second_reading_before_mqtt_echo_is_throttled() {
        // Two readings of one tag reach the main loop before our own message
        // comes back over MQTT.
        let mut devs = make_devices(&[1], Duration::from_mins(1));
        assert_eq!(devs.try_publish(&1), ThrottleResult::Update);
        assert_eq!(devs.try_publish(&1), ThrottleResult::Throttle);
    }

    #[test]
    fn try_publish_marks_only_its_device() {
        let mut devs = make_devices(&[1, 2], Duration::from_mins(1));
        assert_eq!(devs.try_publish(&1), ThrottleResult::Update);
        assert_eq!(devs.try_publish(&2), ThrottleResult::Update);
    }

    #[test]
    fn mark_published_sets_timestamp_and_try_publish_throttles() {
        let mut devs = make_devices(&[1], Duration::from_mins(1));
        // Another instance's message arrives over MQTT.
        devs.mark_published(&1);
        assert_eq!(devs.try_publish(&1), ThrottleResult::Throttle);
    }

    #[test]
    fn throttle_expires_after_duration() {
        let mut devs = make_devices(&[1], Duration::from_millis(10));
        assert_eq!(devs.try_publish(&1), ThrottleResult::Update);
        assert_eq!(devs.try_publish(&1), ThrottleResult::Throttle);

        std::thread::sleep(Duration::from_millis(15));
        assert_eq!(devs.try_publish(&1), ThrottleResult::Update);
    }

    #[test]
    fn mqtt_message_restarts_throttle() {
        let mut devs = make_devices(&[1], Duration::from_millis(200));
        assert_eq!(devs.try_publish(&1), ThrottleResult::Update);

        std::thread::sleep(Duration::from_millis(120));
        // Another instance published before our message came back.
        devs.mark_published(&1);

        // 240 ms after our publish, but only 120 ms after the MQTT message.
        std::thread::sleep(Duration::from_millis(120));
        assert_eq!(devs.try_publish(&1), ThrottleResult::Throttle);
    }

    #[test]
    fn zero_throttle_always_returns_update() {
        let mut devs = make_devices(&[1], Duration::ZERO);
        assert_eq!(devs.try_publish(&1), ThrottleResult::Update);
        assert_eq!(devs.try_publish(&1), ThrottleResult::Update);
    }

    #[test]
    fn zero_throttle_mark_published_is_noop() {
        let mut devs = make_devices(&[1], Duration::ZERO);
        assert!(devs.mark_published(&1).is_none());
        assert_eq!(devs.try_publish(&1), ThrottleResult::Update);
    }

    #[test]
    fn mark_published_unknown_device_returns_none() {
        let mut devs = make_devices(&[1], Duration::from_mins(1));
        assert!(devs.mark_published(&2).is_none());
    }

    #[test]
    fn mark_published_returns_device_data() {
        let mut devs = make_devices(&[1], Duration::from_mins(1));
        let result = devs.mark_published(&1);
        assert_eq!(result.as_deref(), Some("data-1"));
    }
}
