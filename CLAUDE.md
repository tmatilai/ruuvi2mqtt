# ruuvi2mqtt

Two independent Rust crates that read Ruuvi BLE sensor broadcasts and publish
them to MQTT. They share no code, only the `ruuvi-sensor-protocol` dependency.
Two test bed crates: `checker/` checks the Linux app, and `air-simulator/`
advertises like a Ruuvi Air.

## Linux daemon (repository root)

Tokio app, edition 2024. Runs continuously, throttles updates per sensor, and
publishes Home Assistant MQTT discovery. Released for Linux; the optional
`dbus` dependency is Linux-only.

- `src/main.rs`: startup, the `Event` enum, and the event loop that ties BLE and MQTT together.
- `src/config.rs`: CLI (clap) and YAML config (serde_norway, serde_with), version info (sysinfo).
- `src/ruuvi/listener.rs`: BLE scanning with btleplug, sends `Event::RuuviUpdate` after a per-reading delay.
- `src/ruuvi/delay.rs`: maps RSSI plus random jitter to that delay, so the closest instance publishes first. Has unit tests.
- `src/ruuvi/formats.rs`: drops an Air's format 6 readings while its E1 ones arrive. Has unit tests.
- `src/ruuvi/sensor_data.rs`: decodes ruuvi-sensor-protocol payloads into the JSON published to MQTT. Has unit tests.
- `src/mqtt.rs`: rumqttc client and event loop, TLS via rustls-native-certs, the status topic and its last will.
- `src/homeassistant.rs`: Home Assistant discovery payloads and topics.
- `src/devices.rs`: per-device state and update throttling.
- `build.rs`: sets `BUILD_VERSION`, the package version plus the git commit. The ESP32 `build.rs` has a copy.
- `tests/cmd/`: trycmd CLI tests: help output, and config checks with `--check-config`. A `<name>.in/` directory is the test's working directory.

Commands: `make lint`, `make test`, `make local`. Cross builds use `cross`.

## ESP32 firmware (`ruuvi2mqtt-esp32/`)

Firmware on esp-idf-svc (ESP-IDF pinned in `.cargo/config.toml`). By
default it scans continuously, stays connected, and publishes each scan's
readings over MQTT while the next scan runs. The deep-sleep modes run one
cycle per wake instead: start a BLE scan thread, connect Wi-Fi, publish,
deep sleep. Configuration is compile-time via environment variables, see
`ruuvi2mqtt-esp32/README.md`.

- `src/main.rs`: the wake cycle and logger setup.
- `src/config.rs`: compile-time config from env vars (konst).
- `src/wifi.rs`, `src/mqtt.rs`, `src/ble.rs`, `src/led.rs`: one esp-idf-svc subsystem each; `ble.rs` uses esp32-nimble. `src/system.rs` reads values for the diagnostics. `src/antenna.rs` drives the XIAO ESP32-C6's RF switch.
- `src/lib.rs`: the pure-Rust modules, `mac.rs` (MAC addresses), `payload.rs` (the reading's JSON), `scan.rs` (BLE reception statistics), `diagnostics.rs` (the diagnostics JSON) and `led_pattern.rs` (the LED's breath and blinks). esp-idf-svc and esp32-nimble are espidf-only dependencies, so the lib's unit tests run on the host with stable Rust (`--target host-tuple`). The bin has `test = false`.
- `build.rs`: embuild glue for ESP-IDF.
- `boards.sh`: finds the attached boards and their serial ports for the Makefile, from macOS `ioreg` or Linux udev.

esp-idf-svc is used by every module outside the lib, so an update to it
touches the whole firmware. Commands: `make -C ruuvi2mqtt-esp32 lint|test|build`. The firmware
build needs the ESP toolchain (`make setup`); CI builds it for ESP32-C5,
ESP32-C6 and ESP32-S3.

Attached boards: `make boards` lists them, `make flash DEVICE=<name>`
flashes one, and `make log DEVICE=<name> SECS=70` prints its serial
output. `make monitor` is interactive, so don't use it. Identifying an
unregistered board behind a USB-UART bridge resets it, so ask before
touching boards that may be running tests.

## Test bed checker (`checker/`)

A member of the root crate's workspace, so it shares the lockfile. The
ESP32 crate is not a member. The checker watches a test bed's broker after
a commit is deployed there. It checks the instances' statuses, readings and
discovery configs, and the ESP32 boards' diagnostics. See `checker/README.md`.

- `src/check.rs`: the checks, without MQTT. Has unit tests. `ENTITIES` and
  `VALUES` mirror the app's payloads, and `parse_diagnostics` the ESP32
  diagnostics. Update them when the payloads change.
- `src/main.rs`: the CLI and the MQTT loop.

Commands: `cargo test -p ruuvi2mqtt-checker`; `make lint` and `make test`
cover it.

## Air simulator (`air-simulator/`)

ESP32 firmware, not a workspace member. It shares the firmware's
`.cargo/config.toml` and `rust-toolchain.toml` through symlinks, pins the
same crate versions, and finds its board with the firmware's `boards.sh`,
`devices/devices.conf` and `devices/<name>.mk`.
`air-simulator/README.md` lists its advertising sets.

- `src/lib.rs`: the format 6 and E1 encoders and the advertising data. Has
  unit tests against the specs' test vectors and ruuvi-sensor-protocol.
- `src/main.rs`: the advertising sets, through esp32-nimble and NimBLE.
- `src/led.rs`: the LED's slow breath. Its curve is in `lib.rs`.

Commands: `make -C air-simulator lint|test|build|flash|log CHIP=<chip>`.

## What CI covers

fmt and clippy (pedantic) for the Linux app, the firmware and the checker,
the trycmd CLI tests, ESP32 host-side unit tests for the pure-Rust modules,
the checker's unit tests, the Linux binary for six targets, and release
firmware builds for ESP32-C5, ESP32-C6 and ESP32-S3. For the Air simulator,
only fmt, and clippy and the unit tests on the host.

Not covered: no MQTT broker, no Bluetooth hardware, and no ESP32 runtime.
Runtime behavior in `mqtt.rs`, `ruuvi/listener.rs`, `wifi.rs`, and `ble.rs`,
as well as YAML config parsing beyond the trycmd cases, is verified manually
and, for the Linux app, on the test bed with the checker.

## Writing style

Be concise. Prefer short sentences that say one thing, in docs, code
comments, commit messages, and replies. Comments explain why, not what the
code already says. Commit messages state what changed and why in a few
plain sentences. Leave out summaries, restatements, and hedging.

Commit messages have no trailers, such as `Co-Authored-By`.
