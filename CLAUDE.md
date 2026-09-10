# ruuvi2mqtt

Two independent Rust crates that read Ruuvi BLE sensor broadcasts and publish
them to MQTT. They share no code, only the `ruuvi-sensor-protocol` dependency.

## Linux daemon (repository root)

Tokio app, edition 2024. Runs continuously, throttles updates per sensor, and
publishes Home Assistant MQTT discovery. Released for Linux; the optional
`dbus` dependency is Linux-only.

- `src/main.rs`: startup, the `Event` enum, and the event loop that ties BLE and MQTT together.
- `src/config.rs`: CLI (clap) and YAML config (serde_norway, serde_with), version info (sysinfo).
- `src/ruuvi/listener.rs`: BLE scanning with btleplug, sends `Event::RuuviUpdate`.
- `src/ruuvi/sensor_data.rs`: decodes ruuvi-sensor-protocol payloads into the JSON published to MQTT. Has unit tests.
- `src/mqtt.rs`: rumqttc client and event loop, TLS via rustls-native-certs.
- `src/homeassistant.rs`: Home Assistant discovery payloads and topics.
- `src/devices.rs`: per-device state and update throttling.
- `tests/cmd/`: trycmd CLI tests (help output and config errors only).

Commands: `make lint`, `make test`, `make local`. Cross builds use `cross`.

## ESP32 firmware (`ruuvi2mqtt-esp32/`)

Deep-sleep firmware on esp-idf-svc (ESP-IDF pinned in `.cargo/config.toml`).
Every wake: start a BLE scan thread, connect Wi-Fi, publish the readings over
MQTT, deep sleep. Configuration is
compile-time via environment variables, see `ruuvi2mqtt-esp32/README.md`.

- `src/main.rs`: the wake cycle and logger setup.
- `src/config.rs`: compile-time config from env vars (konst).
- `src/wifi.rs`, `src/mqtt.rs`, `src/ble.rs`, `src/led.rs`, `src/diagnostics.rs`: one esp-idf-svc subsystem each; `ble.rs` uses esp32-nimble.
- `src/mac.rs`: MAC address parsing, pure Rust with unit tests.
- `build.rs`: embuild glue for ESP-IDF.

esp-idf-svc is used by every module here, so an update to it touches the whole
crate. Commands: `make -C ruuvi2mqtt-esp32 lint|test|build`. The firmware
build needs the ESP toolchain (`make setup`); CI builds it for ESP32-C6 only.

## What CI covers

fmt and clippy (pedantic) for both crates, the trycmd CLI tests, ESP32
host-side unit tests for the pure-Rust modules, the Linux binary for six
targets, and a release firmware build for ESP32-C6.

Not covered: no MQTT broker, no Bluetooth hardware, and no ESP32 runtime.
Runtime behavior in `mqtt.rs`, `ruuvi/listener.rs`, `wifi.rs`, and `ble.rs`,
as well as YAML config parsing beyond the two trycmd cases, is only verified
manually.

## Writing style

Be concise. Prefer short sentences that say one thing, in docs, code
comments, commit messages, and replies. Comments explain why, not what the
code already says. Commit messages state what changed and why in a few
plain sentences. Leave out summaries, restatements, and hedging.
