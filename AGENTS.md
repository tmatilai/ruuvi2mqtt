# ruuvi2mqtt

Two independent Rust crates that read Ruuvi BLE sensor broadcasts and publish
them to MQTT. They share no code, only the `ruuvi-sensor-protocol` dependency.
Two test bed crates: `checker/` checks the app and the ESP32 boards, and
`air-simulator/` advertises like a Ruuvi Air.

## App (repository root)

Tokio app, edition 2024. Runs continuously, throttles updates per tag, and
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
- `src/diagnostics.rs`: the instance's diagnostics, counted per minute. Has unit
  tests.
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
`ruuvi2mqtt-esp32/README.md`. `ruuvi2mqtt-esp32/boards.md` has each known
board's settings.

- `src/main.rs`: the continuous loop, the deep-sleep wake cycle, and logger
  setup.
- `src/config.rs`: compile-time config from env vars (konst).
- `src/wifi.rs`, `src/mqtt.rs`, `src/ble.rs`, `src/led.rs`: one esp-idf-svc
  subsystem each; `ble.rs` uses esp32-nimble. `src/system.rs` reads board values
  from ESP-IDF. `src/antenna.rs` drives the XIAO ESP32-C6's RF switch.
  `src/gauge.rs` reads the battery gauge over I2C. `src/gpio.rs` sets a pin as
  an output. `src/web.rs` serves the status page (`src/status.html`) and mDNS,
  in the continuous mode.
- `src/lib.rs`: the pure-Rust modules, `mac.rs` (MAC addresses), `payload.rs`
  (the reading's JSON, and an Air's format 6 versus E1), `roam.rs` (when to
  switch access points), `scan.rs` (BLE reception statistics), `diagnostics.rs`
  (the diagnostics JSON), `battery.rs` (the battery gauges' readings and the
  battery's state), `homeassistant.rs` (the board's discovery configs),
  `status.rs` (the status page's report and grades) and `led_pattern.rs` (the
  LED's breath and blinks). esp-idf-svc and esp32-nimble are espidf-only
  dependencies, so the lib's unit tests run on the host with stable Rust
  (`--target host-tuple`). The bin has `test = false`.
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
deployed there. It checks the instances' statuses, readings, discovery configs
and diagnostics, and the ESP32 boards' diagnostics. Its `record` mode runs all
the time, tells the runs which tags to expect and how often they come, and
writes the test bed's status page. Its `site` mode prints how well each
publisher hears each tag, on any broker. See `checker/README.md`.

- `src/payload.rs`: the app's readings, discovery configs, statuses and
  diagnostics, and `src/diagnostics.rs` the ESP32 diagnostics. They mirror the
  payloads, so update them when the payloads change. `printable` escapes the
  control characters in text from the broker, so that it can't add lines to the
  report.
- `src/topic.rs`: the topics, for all modes.
- `src/check.rs`: the checks, without MQTT. `src/check/boards.rs` has the ESP32
  section, `src/check/instances.rs` the instances' diagnostics,
  `src/check/tests.rs` the tests.
- `src/record.rs`: the record mode's state, without MQTT. Has unit tests.
  `src/record/page.rs` renders the status page, and `src/history.rs` keeps the
  diagnostics for it.
- `src/site.rs`: the site table, how well each publisher hears each tag, and the
  coverage problems, without MQTT. Has unit tests.
- `src/cli.rs`: the CLI. clap validates the arguments, and then they become the
  check's or a subcommand's options: clap's derive can't have the check's
  options beside the subcommand.
- `src/mqtt.rs`: the broker connection, which subscribes and reconnects.
  `src/main.rs` has the check's, the recorder's and the site's loops on it.
- `src/time.rs`: the durations, and the longest gap.
- `src/stats.rs`: counts and means, for the check and the site.
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

## Terms

Use one term per thing, in the docs, the comments and the commit messages.

- **tag**: a Ruuvi device that broadcasts readings, a RuuviTag or a Ruuvi Air.
  "RuuviTag" and "Air" name the model when it matters.
- **advertisement**: one BLE broadcast from a tag.
- **reading**: the values decoded from an advertisement, and the MQTT message
  that carries them.
- **the app**: the ruuvi2mqtt app in the repository root. An **instance** is one
  running copy of it.
- **the firmware**: `ruuvi2mqtt-esp32`. A **board** is an ESP32 that runs it.
- **publisher**: an instance or a board, known by its MQTT client ID.
- **cycle**: one scan of a board and the publish of what it heard.
- **the continuous mode** and **the deep-sleep mode**: the firmware's two
  `BLE_SCAN_MODE` values.
- **sensor** and **device**: a Home Assistant sensor and a Home Assistant
  device. A setting with another meaning keeps its name, such as `devices` and
  `DEVICE`.
- **test bed**: instances and boards near real tags, on a broker of their own,
  where the checker runs.

## Writing style

The rules cover the docs, the code comments, the commit messages and the
replies. A review checks the changed text against them, and quotes the rule for
each finding.

Be concise. Prefer short sentences that say one thing. Leave out summaries,
restatements, and hedging. Describe the current state in the present tense, not
its history. State each fact once, and link to it from elsewhere.

This repository is public. The docs, the comments and the commit messages
describe the code, never a particular installation. Examples and fixtures use
made-up values, such as `AA:BB:CC:DD:EE:01` and `broker.example`.

### Wording

- Name the thing. Write "the sample rate", not "the rate". Repeat the noun when
  "it" could mean two things.
- Call a setting, a field or an option by its own name in backticks, not by a
  description of it.
- An ongoing state takes "is": "whether the job is running", not "whether the
  job runs".
- Name the actor instead of "we", "our" or "you": "the instance", "the board".
  Instructions use the imperative.
- Write "such as" or "for example", not "e.g.". Spell out "access point", and
  write "Wi-Fi".
- Put a space between a number and its unit: "100 mA", "10 %", "−70 dBm". A
  range takes an en dash: "5–10 s".
- Use a period or a colon between clauses, not an em dash.
- An error message states each rule that can fail, so that it points at the fix.

### Docs

- New text gets its own paragraph or goes at the end of one, not into the
  middle.
- A placeholder is lowercase in angle brackets:
  `<base_topic>/status/<client_id>`.
- A code fence names its language: `sh` for commands, `text` for output.
- Headings separate the sections, without horizontal rules. The section on how
  to run something is "Usage".
- Every user-visible change adds a line to `CHANGELOG.md` in the same commit,
  under Breaking changes, Features or Fixes, in that order. A change to a
  feature that is itself unreleased needs none.

Prettier wraps the Markdown files at 80 columns, except `CHANGELOG.md`, whose
entries stay on one line. `make fmt-docs` runs it, and `make lint` too.

### Comments

- A comment explains why, not what the code already says.
- Every source file starts with a `//!` header that says what the module is for.
- A function's doc comment starts with a verb in the third person: "Returns",
  not "Return".
- A comment ends with a period, also a one-line one. clap's help text is the
  exception: it follows the CLI convention of no final period.

### Commit messages

A commit message states what changed and why in a few plain sentences. It has no
trailers, such as `Co-Authored-By`.

## Docs to keep in sync

A change to one of these also updates the others:

- A setting: the README that documents it, and its example file,
  `ruuvi2mqtt.yaml.example` or `devices/device.mk.example`. A setting that
  depends on the board also goes in `boards.md`.
- A payload: the READMEs' JSON examples and field tables, and the checker's
  mirrors.
- A source file: its `//!` header, and the module lists above.
- A heading: the links to its anchor.
- A claim that a doc changes or drops: the same claim in the other docs and in
  the comments. Search for it.
