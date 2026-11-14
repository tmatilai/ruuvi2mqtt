# ruuvi2mqtt

Two independent Rust crates that read the tags' BLE advertisements and publish
their readings to MQTT. They share no code, only the `ruuvi-sensor-protocol`
dependency. Two test bed crates: `checker/` checks the app and the ESP32 boards,
and `air-simulator/` advertises like a Ruuvi Air.

## App (repository root)

Tokio app, edition 2024. Runs continuously, throttles updates per tag, and
publishes Home Assistant MQTT discovery. Released for Linux, also runs on macOS;
the optional `dbus` dependency is Linux-only.

`src/main.rs` has the event loop that ties BLE (`src/ruuvi/`) and MQTT
(`src/mqtt.rs`) together. `build.rs` sets `BUILD_VERSION`, and the ESP32
`build.rs` has a copy. `tests/cmd/` has the trycmd CLI tests: the help output,
and config checks with `--check-config`. A `<name>.in/` directory is the test's
working directory.

Commands: `make lint`, `make test`, `make local`. Cross builds use `cross`.

## ESP32 firmware (`ruuvi2mqtt-esp32/`)

Firmware on esp-idf-svc (ESP-IDF pinned in `.cargo/config.toml`). The continuous
mode, the default, scans all the time, stays connected, and publishes each
scan's readings over MQTT while the next scan runs. The deep-sleep mode runs one
cycle per wake instead: start a BLE scan thread, connect Wi-Fi, publish, deep
sleep. Configuration is compile-time via environment variables, see
`ruuvi2mqtt-esp32/README.md`. `ruuvi2mqtt-esp32/boards.md` has each known
board's settings.

The bin, `src/main.rs` and the modules it declares, runs on the board. Every
module of it uses esp-idf-svc, so an update to that touches the whole firmware.
`src/lib.rs` declares the pure-Rust modules. esp-idf-svc and esp32-nimble are
espidf-only dependencies, so the lib's unit tests run on the host with stable
Rust (`--target host-tuple`). The bin has `test = false`. Logic that needs no
ESP-IDF goes in the lib, where it can be tested.

`boards.sh` finds the attached boards and their serial ports for the Makefile,
from macOS `ioreg` or Linux udev.

Commands: `make -C ruuvi2mqtt-esp32 lint|test|build`. The firmware build needs
the ESP toolchain (`make setup`); CI builds it for ESP32-C5, ESP32-C6 and
ESP32-S3.

Attached boards: `make boards` lists them, `make flash BOARD=<name>` flashes
one, and `make log BOARD=<name> SECS=70` prints its serial output.
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

`src/payload.rs` and `src/diagnostics.rs` mirror the app's and the firmware's
payloads, so update them when the payloads change. The checks, the recorder's
state and the site table are kept apart from MQTT, so that unit tests run them.
Text from the broker goes through `printable` before it is printed, so that it
can't add lines to the report.

Commands: `cargo test -p ruuvi2mqtt-checker`; `make lint` and `make test` cover
it.

## Air simulator (`air-simulator/`)

ESP32 firmware, not a workspace member. It shares the firmware's
`.cargo/config.toml` and `rust-toolchain.toml` through symlinks, pins the same
crate versions, and finds its board with the firmware's `boards.sh`,
`boards/boards.conf` and `boards/<name>.mk`. `air-simulator/README.md` lists its
advertising sets. `src/lib.rs` has the pure-Rust part, with unit tests on the
host.

Commands: `make -C air-simulator lint|test|build|flash|log CHIP=<chip>`.

## What CI covers

The version check (`script/check-version`), Prettier for the Markdown files,
their links (remark-validate-links), the spelling (typos), fmt and clippy
(pedantic) for the app, the firmware and the checker, zizmor for the workflows,
the trycmd CLI tests, ESP32 host-side unit tests for the pure-Rust modules, the
checker's unit tests, the Linux binary for six targets, and release firmware
builds for ESP32-C5, ESP32-C6 and ESP32-S3. For the Air simulator, only fmt, and
clippy and the unit tests on the host.

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
  device. A setting with another meaning keeps its name, such as the app's
  `devices`.
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
- A changelog entry names the change, with enough context for a reader to tell
  whether it affects them. One sentence is usually enough. How the change works
  and how to migrate belong in the README or in the tool's own messages.

Prettier wraps the Markdown files at 80 columns, except `CHANGELOG.md`, whose
entries stay on one line. `make fmt-docs` runs it, and `make lint` too.

### Comments

- A comment explains why, not what the code already says.
- Every source file starts with a `//!` header that says what the module is for.
- A function's doc comment starts with a verb in the third person: "Returns",
  not "Return".
- A comment on lines of its own ends with a period, also a one-line one. clap's
  help text is the exception: it follows the CLI convention of no final period.

### Commit messages

A commit message states what changed and why in a few plain sentences. It leaves
out what was not done, unless a reader would expect it. It has no trailers, such
as `Co-Authored-By`.

## Docs to keep in sync

A change to one of these also updates the others:

- A setting: the README that documents it, and its example file,
  `ruuvi2mqtt.yaml.example` or `boards/board.mk.example`. A setting that depends
  on the board also goes in `boards.md`.
- A payload: the READMEs' JSON examples and field tables, and the checker's
  mirrors.
- A source file: its `//!` header.
- A heading: the links to its anchor.
- A claim that a doc changes or drops: the same claim in the other docs and in
  the comments. Search for it.
