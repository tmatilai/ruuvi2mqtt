# ruuvi2mqtt

Two independent Rust crates that read Ruuvi BLE sensor broadcasts and publish
them to MQTT. They share no code, only the `ruuvi-sensor-protocol` dependency.
Two test bed crates: `checker/` checks the app and the ESP32 boards, and
`air-simulator/` advertises like a Ruuvi Air.

## App (repository root)

Tokio app, edition 2024. Runs continuously, throttles updates per sensor, and
publishes Home Assistant MQTT discovery. Released for Linux, also runs on macOS;
the optional `dbus` dependency is Linux-only.

- `src/main.rs`: startup, the `Event` enum, and the event loop that ties BLE and
  MQTT together.
- `src/config.rs`: CLI (clap) and YAML config (serde_norway, serde_with), the
  hostname for the default client ID (sysinfo).
- `src/ruuvi/listener.rs`: BLE scanning with btleplug, sends
  `Event::RuuviUpdate` after a per-reading delay.
- `src/ruuvi/delay.rs`: maps RSSI plus random jitter to that delay, so the
  closest instance publishes first. Has unit tests.
- `src/ruuvi/formats.rs`: drops an Air's format 6 readings while its E1 ones
  arrive. Has unit tests.
- `src/ruuvi/sensor_data.rs`: wraps ruuvi-sensor-protocol's decoded values, with
  unit conversions and the low battery check. Has unit tests.
- `src/mqtt.rs`: rumqttc client and event loop, TLS via rustls-native-certs, the
  status topic and its last will.
- `src/homeassistant.rs`: the reading's JSON, and the Home Assistant discovery
  payloads and topics.
- `src/devices.rs`: per-device state and update throttling.
- `build.rs`: sets `BUILD_VERSION`, the package version plus the git commit. The
  ESP32 `build.rs` has a copy.
- `tests/cmd/`: trycmd CLI tests: help output, and config checks with
  `--check-config`. A `<name>.in/` directory is the test's working directory.

Commands: `make lint`, `make test`, `make local`. Cross builds use `cross`.

## ESP32 firmware (`ruuvi2mqtt-esp32/`)

Firmware on esp-idf-svc (ESP-IDF pinned in `.cargo/config.toml`). By default it
scans continuously, stays connected, and publishes each scan's readings over
MQTT while the next scan runs. The deep-sleep mode runs one cycle per wake
instead: start a BLE scan thread, connect Wi-Fi, publish, deep sleep.
Configuration is compile-time via environment variables, see
`ruuvi2mqtt-esp32/README.md`.

- `src/main.rs`: the continuous loop, the deep-sleep wake cycle, and logger
  setup.
- `src/config.rs`: compile-time config from env vars (konst).
- `src/wifi.rs`, `src/mqtt.rs`, `src/ble.rs`, `src/led.rs`: one esp-idf-svc
  subsystem each; `ble.rs` uses esp32-nimble. `src/system.rs` reads board values
  from ESP-IDF. `src/antenna.rs` drives the XIAO ESP32-C6's RF switch.
  `src/web.rs` serves the status page (`src/status.html`) and mDNS, scanning
  continuously.
- `src/lib.rs`: the pure-Rust modules, `mac.rs` (MAC addresses), `payload.rs`
  (the reading's JSON, and an Air's format 6 versus E1), `roam.rs` (when to
  switch access points), `scan.rs` (BLE reception statistics), `diagnostics.rs`
  (the diagnostics JSON), `status.rs` (the status page's report and grades) and
  `led_pattern.rs` (the LED's breath and blinks). esp-idf-svc and esp32-nimble
  are espidf-only dependencies, so the lib's unit tests run on the host with
  stable Rust (`--target host-tuple`). The bin has `test = false`.
- `build.rs`: embuild glue for ESP-IDF, the embedded CA certificate, and
  `BUILD_VERSION`.
- `boards.sh`: finds the attached boards and their serial ports for the
  Makefile, from macOS `ioreg` or Linux udev.

esp-idf-svc is used by every module outside the lib, so an update to it touches
the whole firmware. Commands: `make -C ruuvi2mqtt-esp32 lint|test|build`. The
firmware build needs the ESP toolchain (`make setup`); CI builds it for
ESP32-C5, ESP32-C6 and ESP32-S3.

Attached boards: `make boards` lists them, `make flash DEVICE=<name>` flashes
one, and `make log DEVICE=<name> SECS=70` prints its serial output.
`make monitor` is interactive, so don't use it. Identifying an unregistered
board behind a USB-UART bridge resets it, so ask before touching boards that may
be running tests.

## Test bed checker (`checker/`)

A member of the root crate's workspace, so it shares the lockfile. The ESP32
crate is not a member. The checker watches a test bed's broker after a commit is
deployed there. It checks the instances' statuses, readings and discovery
configs, and the ESP32 boards' diagnostics. Its `record` mode runs all the time,
and tells the runs which tags to expect and how often they come. See
`checker/README.md`.

- `src/payload.rs`: the app's readings, discovery configs and statuses, and
  `src/diagnostics.rs` the ESP32 diagnostics. They mirror the payloads, so
  update them when the payloads change. `printable` escapes the control
  characters in text from the broker, so that it can't add lines to the report.
- `src/topic.rs`: the topics, for both modes.
- `src/check.rs`: the checks, without MQTT. `src/check/boards.rs` has the ESP32
  section, `src/check/tests.rs` the tests.
- `src/record.rs`: the record mode's state, without MQTT. Has unit tests.
- `src/cli.rs`: the CLI. clap validates the arguments, and then they become the
  check's or the record mode's options: clap's derive can't have the check's
  options beside the subcommand.
- `src/mqtt.rs`: the broker connection, which subscribes and reconnects.
  `src/main.rs` has the check's and the recorder's loops on it.
- `src/time.rs`: the durations, and the longest gap.
- `src/fixtures.rs`: payloads for the unit tests.

Commands: `cargo test -p ruuvi2mqtt-checker`; `make lint` and `make test` cover
it.

## Air simulator (`air-simulator/`)

ESP32 firmware, not a workspace member. It shares the firmware's
`.cargo/config.toml` and `rust-toolchain.toml` through symlinks, pins the same
crate versions, and finds its board with the firmware's `boards.sh`,
`devices/devices.conf` and `devices/<name>.mk`. `air-simulator/README.md` lists
its advertising sets.

- `src/lib.rs`: the format 6 and E1 encoders and the advertising data. Has unit
  tests against the specs' test vectors and ruuvi-sensor-protocol.
- `src/main.rs`: the advertising sets, through esp32-nimble and NimBLE.
- `src/led.rs`: the LED's slow breath. Its curve is in `lib.rs`.
- `src/antenna.rs`: the XIAO ESP32-C6's RF switch, as in the firmware.

Commands: `make -C air-simulator lint|test|build|flash|log CHIP=<chip>`.

## What CI covers

The version check (`script/check-version`), Prettier for the Markdown files, fmt
and clippy (pedantic) for the app, the firmware and the checker, zizmor for the
workflows, the trycmd CLI tests, ESP32 host-side unit tests for the pure-Rust
modules, the checker's unit tests, the Linux binary for six targets, and release
firmware builds for ESP32-C5, ESP32-C6 and ESP32-S3. For the Air simulator, only
fmt, and clippy and the unit tests on the host.

Not covered: no MQTT broker, no Bluetooth hardware, and no ESP32 runtime.
Runtime behavior in `mqtt.rs`, `ruuvi/listener.rs`, `wifi.rs`, and `ble.rs`, as
well as YAML config parsing beyond the trycmd cases, is verified manually and on
the test bed with the checker.

## Writing style

Be concise. Prefer short sentences that say one thing, in docs, code comments,
commit messages, and replies. Comments explain why, not what the code already
says. Commit messages state what changed and why in a few plain sentences. Leave
out summaries, restatements, and hedging.

Prettier wraps the Markdown files at 80 columns, except CHANGELOG.md, whose
entries stay on one line. `make fmt-docs` runs it, and `make lint` too.

Commit messages have no trailers, such as `Co-Authored-By`.
