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

    pub const fn last_updated(&self) -> &Option<Instant> {
        &self.last_updated
    }

    pub fn update(&mut self) {
        self.last_updated = Some(Instant::now());
    }

    pub fn data(&self) -> T {
        self.data.clone()
    }
}
