# ruuvi2mqtt

Two independent Rust crates that read Ruuvi BLE sensor broadcasts and publish
them to MQTT. They share no code, only the `ruuvi-sensor-protocol` dependency.
A third crate, `checker/`, checks the Linux app on a test bed.

## Linux daemon (repository root)

Tokio app, edition 2024. Runs continuously, throttles updates per sensor, and
publishes Home Assistant MQTT discovery. Released for Linux; the optional
`dbus` dependency is Linux-only.

- `src/main.rs`: startup, the `Event` enum, and the event loop that ties BLE and MQTT together.
- `src/config.rs`: CLI (clap) and YAML config (serde_norway, serde_with), version info (sysinfo).
- `src/ruuvi/listener.rs`: BLE scanning with btleplug, sends `Event::RuuviUpdate` after a per-reading delay.
- `src/ruuvi/delay.rs`: maps RSSI plus random jitter to that delay, so the closest instance publishes first. Has unit tests.
- `src/ruuvi/sensor_data.rs`: decodes ruuvi-sensor-protocol payloads into the JSON published to MQTT. Has unit tests.
- `src/mqtt.rs`: rumqttc client and event loop, TLS via rustls-native-certs, the status topic and its last will.
- `src/homeassistant.rs`: Home Assistant discovery payloads and topics.
- `src/devices.rs`: per-device state and update throttling.
- `build.rs`: sets `BUILD_VERSION`, the package version plus the git commit. The ESP32 `build.rs` has a copy.
- `tests/cmd/`: trycmd CLI tests: help output, and config checks with `--check-config`. A `<name>.in/` directory is the test's working directory.

Commands: `make lint`, `make test`, `make local`. Cross builds use `cross`.

## ESP32 firmware (`ruuvi2mqtt-esp32/`)

Deep-sleep firmware on esp-idf-svc (ESP-IDF pinned in `.cargo/config.toml`).
Every wake: start a BLE scan thread, connect Wi-Fi, publish the readings over
MQTT, deep sleep. Configuration is
compile-time via environment variables, see `ruuvi2mqtt-esp32/README.md`.

- `src/main.rs`: the wake cycle and logger setup.
- `src/config.rs`: compile-time config from env vars (konst).
- `src/wifi.rs`, `src/mqtt.rs`, `src/ble.rs`, `src/led.rs`, `src/diagnostics.rs`: one esp-idf-svc subsystem each; `ble.rs` uses esp32-nimble.
- `src/lib.rs`: the pure-Rust modules, `mac.rs` (MAC addresses) and `payload.rs` (the reading's JSON). esp-idf-svc and esp32-nimble are espidf-only dependencies, so the lib's unit tests run on the host with stable Rust (`--target host-tuple`). The bin has `test = false`.
- `build.rs`: embuild glue for ESP-IDF.

esp-idf-svc is used by every module outside the lib, so an update to it
touches the whole firmware. Commands: `make -C ruuvi2mqtt-esp32 lint|test|build`. The firmware
build needs the ESP toolchain (`make setup`); CI builds it for ESP32-C6 only.

## Test bed checker (`checker/`)

A member of the root crate's workspace, so it shares the lockfile. The
ESP32 crate is not a member. The checker watches a test bed's broker after
a commit is deployed there. It checks the instances' statuses, readings and
discovery configs. See `checker/README.md`.

- `src/check.rs`: the checks, without MQTT. Has unit tests. `ENTITIES` and
  `VALUES` mirror the app's payloads. Update them when the payloads change.
- `src/main.rs`: the CLI and the MQTT loop.

Commands: `cargo test -p ruuvi2mqtt-checker`; `make lint` and `make test`
cover it.

## What CI covers

fmt and clippy (pedantic) for all three crates, the trycmd CLI tests, ESP32
host-side unit tests for the pure-Rust modules, the checker's unit tests, the
Linux binary for six targets, and a release firmware build for ESP32-C6.

Not covered: no MQTT broker, no Bluetooth hardware, and no ESP32 runtime.
Runtime behavior in `mqtt.rs`, `ruuvi/listener.rs`, `wifi.rs`, and `ble.rs`,
as well as YAML config parsing beyond the trycmd cases, is verified manually
and, for the Linux app, on the test bed with the checker.

## Writing style

Be concise. Prefer short sentences that say one thing, in docs, code
comments, commit messages, and replies. Comments explain why, not what the
code already says. Commit messages state what changed and why in a few
plain sentences. Leave out summaries, restatements, and hedging.
