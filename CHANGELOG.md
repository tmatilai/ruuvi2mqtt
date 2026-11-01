## _Not released yet_

### ruuvi2mqtt-esp32

#### Features

- Publish a status with a last will when scanning continuously, like the app.

## 1.7.0 / 2026-11-01

### ruuvi2mqtt

#### Features

- Read `MQTT_USER` and `MQTT_PASSWORD` from the environment, overriding `mqtt.user` and `mqtt.password`. An empty variable is an error.
- Allow an MQTT username without a password.
- Support Ruuvi Air, as a preview. Set `model: air` on its device for its Home Assistant entities. PM1, PM4 and PM10 need a Bluetooth 5 adapter. See [Ruuvi Air](./README.md#ruuvi-air).

#### Fixes

- Fail at startup when `mqtt.password` is set without `mqtt.user`, instead of connecting anonymously.

### ruuvi2mqtt-esp32

The firmware's defaults change in this release, so read the breaking changes before flashing a board. It is still a minor release: as far as I know, nobody else runs the firmware yet. If you do, sorry for the extra work.

#### Breaking changes

- Scan continuously by default. `BLE_SCAN_MODE=continuous` stays connected to Wi-Fi and MQTT, and publishes every `BLE_SCAN_DURATION`, 60 s by default.
- Rename `BLE_SCAN_MODE=parallel` to `deep-sleep`, which a board on a battery now needs. Remove `sequential`.
- Remove the `LED_GPIO` default, 15, the Seeed XIAO ESP32-C6's pin, which may drive something else on other boards. `LED_MODE` requires `LED_GPIO`, and the XIAO's LED needs `LED_TYPE=gpio` and `LED_ACTIVE_LOW=true`.
- Default `LED_MODE` to `on`, a dim light, when `LED_GPIO` is set. `LED_MODE=off` keeps it dark.
- Fail the build on an unknown `LED_MODE` or `LED_TYPE`.
- Fail the build on an empty `WIFI_SSID` or `WIFI_PASS`. An open network needs `WIFI_OPEN=true`, and `WIFI_PASS` must be 8 to 63 characters or 64 hex digits.
- Fail the build on an empty `MQTT_SERVER`, `MQTT_USER` or `MQTT_PASSWORD`, or on `MQTT_PASSWORD` without `MQTT_USER`.

#### Features

- Serve a status page on `http://<DEVICE_HOSTNAME>.local/` when scanning continuously, to help find a good spot for a board. It grades the Wi-Fi, the publishing and the tags chosen on the page. `/api/status` returns the same data as JSON.
- Roam between access points when scanning continuously: below −70 dBm, move to one at least 8 dB stronger. The diagnostics show the BSSID.
- Publish Ruuvi Air readings, like the app, except PM1, PM4 and PM10: the firmware receives only data format 6.
- Blink the LED twice every 3 s while Wi-Fi is down or a cycle failed, and breathe it when scanning continuously. `LED_MODE=errors` shows only the blinks.
- Add `LED_COLOR` for the WS2812's color, a dim blue by default instead of full white.
- Add `LED_ACTIVE_LOW` for an LED that lights when its pin is low.
- Support the ESP32-C5: `CHIP=esp32c5`.
- Add `WIFI_BAND` to keep a dual-band chip on 2.4 or 5 GHz.
- In the deep-sleep mode, stay awake while a computer is on the chip's USB port, so flashing needs no buttons. `USB_STAY_AWAKE=false` turns this off.
- Add `make boards` to list the attached boards by their registered names. `DEVICE` also picks the board's serial port.
- Add `make log` to print the serial output for `SECS` seconds. `make flash` no longer opens the monitor.
- Build each chip in `target/<chip>/`, so one checkout can build several chips.
- Log the MQTT login: a user with or without a password, or anonymous.

#### Fixes

- Document that a Seeed XIAO ESP32-C6 needs `ANTENNA`. Without it, the RF switch gets no power and the board hears little.
- Fall back to a full Wi-Fi scan as soon as the cached access point fails, instead of after 15 s, which could use up the cycle's time.
- Fix a boot loop with `MQTT_CA_FILE`: the embedded certificate lacked the NUL byte that esp-idf-svc needs.
- Reboot when the task watchdog fires, instead of only logging a backtrace.
- Log the git commit at boot, like the diagnostics.

## 1.6.0 / 2026-10-14

### ruuvi2mqtt

#### Features

- Delay each reading by its RSSI plus random jitter, so the instance closest to a tag usually publishes it. `mqtt.throttle` now defaults to 60 seconds instead of a random 50–69; use the same value on every instance.
- Publish a retained status to `<base_topic>/status/<client_id>`: `online` with the version on connect, and `offline` as the MQTT last will.
- Add `publisher`, the MQTT client ID, to each reading.
- Add `--check-config` to validate the configuration file without connecting to MQTT or Bluetooth.
- Include the git commit in the version, when built from a git checkout.

#### Fixes

- Exit with an error when the Bluetooth adapter powers off, e.g. on a controller disconnect or BlueZ restart, so a supervisor can restart the app. It kept running without scanning, as the 1.5.0 fix never triggered on BlueZ.
- Fix duplicate and stale publishes: start the throttle timer when publishing instead of when the message comes back over MQTT, and skip the readings BlueZ cached before startup.
- Read sensor data from BlueZ manufacturer data updates instead of RSSI updates. BlueZ after 5.86 sends RSSI updates only on changes of 8 dB or more, which would miss most readings.

### ruuvi2mqtt-esp32

#### Features

- Measure BLE reception in the diagnostics: per tag the strongest RSSI, the advertisement count and when the first arrived, plus the advertisements before and after Wi-Fi connected. `tags` is now an object keyed by MAC instead of a list.
- Add `BLE_SCAN_MODE=sequential` to scan before Wi-Fi starts. Wi-Fi and BLE share one radio, so the scan gets it to itself, at the cost of a longer awake time.
- Add `publisher`, the MQTT client ID, to each reading, like the Linux app.
- Add to the diagnostics the client ID, chip, cycle count, Wi-Fi channel and cache use, the scan, Wi-Fi and MQTT times, and the git commit in `firmware`.
- Add `ANTENNA` to select the internal or external antenna on the Seeed XIAO ESP32-C6.

## 1.5.1 / 2026-09-30

- Update the dependencies

## 1.5.0 / 2026-09-11

### ruuvi2mqtt

#### Features

- Run the Docker image as an unprivileged user (uid 65534); the mounted configuration file must be readable by it.
- Add a CA certificate store to the Docker image, so `tls: true` works without `ca_file`.
- Validate `mqtt.base_topic`: reject wildcards and a leading `$`, and trim a trailing `/`.
- Warn if the configuration file is readable by other users.
- Rename the example configuration to `ruuvi2mqtt.yaml.example`.

#### Fixes

- Exit with an error if the BLE event stream ends (e.g. BlueZ restart), instead of running idle.
- Drop sensor readings instead of queueing them when the broker is unreachable, to avoid unbounded memory growth and a flood of stale readings on reconnect.
- Fix panicking in a background task if BLE peripheral properties are missing.
- Only fail startup when no platform TLS certificates are found; warn about individual load errors.
- Fix data format 3 tags logging `BDAddr not found` on every advertisement; use the advertiser address when the payload has no MAC.

### ruuvi2mqtt-esp32

#### Features

- Publish diagnostic information to MQTT, including the reason for a failed BLE scan in the `error` field.
- Validate the compile-time configuration at build time (Wi-Fi credential lengths, static IP addresses, durations, LED GPIO) instead of panicking on the device.

#### Fixes

- Deep sleep instead of boot looping after a panic, watchdog, or brownout reset, which used to drain the battery. After a brownout the sleep is 4x longer, to let a weak battery recover.
- Wait for the MQTT connection and for publish acknowledgements before deep sleep. Readings were lost when the broker connected slower than the BLE scan.
- Add a hard timeout for the whole cycle. The task watchdog does not catch a blocked Wi-Fi or MQTT wait.
- Fix the build script not re-running when `MQTT_CA_FILE` is set or unset, which could embed a stale CA certificate.
- Warn about `MQTT_TLS_INSECURE` only when TLS is enabled, and about `MQTT_CA_FILE` when it is not.
- Log failures when setting the hostname or writing the Wi-Fi cache.

## 1.4.0 / 2026-04-15

### ruuvi2mqtt

- Add TLS support for MQTT connections (`tls`, `ca_file`, and `tls_insecure` config options).
- Refactor device throttling methods.
- Fix MQTT topic subscription not being restored after reconnecting to the broker.

### ruuvi2mqtt-esp32

- Initial release: ESP32 firmware for a lightweight Ruuvi2MQTT gateway.

## 1.3.6 / 2026-04-06

- Fix BLE update handling after `btleplug` upgrade to 0.12.0.

## 1.3.5 / 2026-04-03

- Update the dependencies

## 1.3.4 / 2025-04-18

- Also build the x86_64 binaries with Cross, to ensure backwards compatibility with older Linux distributions, as the 20.04 Ubuntu GitHub runners are not available anymore.
- Update the dependencies

## 1.3.3 / 2024-09-21

- Update the dependencies

## 1.3.2 / 2023-10-29

- Update the dependencies

## 1.3.1 / 2023-03-13

- Update the dependencies
- New release for trying to get the container images to be built, too.

## 1.3.0 / 2023-02-05

- Support vendoring libdbus and building statically linked binaries ([#172](https://github.com/tmatilai/ruuvi2mqtt/pull/172)).
- Build the container images from `scratch`, and only include a statically linked (MUSL) binary. The default path of the config file is changed to `/ruuvi2mqtt.yaml`.

## 1.2.0 / 2022-12-22

- Expose diagnostic information ([#124](https://github.com/tmatilai/ruuvi2mqtt/pull/124)):
  - Battery voltage
  - Low batter indicator
  - Transmitter power
  - All data in attributes for all sensors
- Support more detailed log level configuration with the `RUST_LOG` env var ([#145](https://github.com/tmatilai/ruuvi2mqtt/pull/145)):

## 1.1.1 / 2022-10-30

- Upgrade dependencies to fix e.g. timeout issues in the MQTT connection ([#105](https://github.com/tmatilai/ruuvi2mqtt/pull/105)).

## 1.1.0 / 2022-10-29

- Add arm64 (aarch64) to build targets ([#49](https://github.com/tmatilai/ruuvi2mqtt/pull/49)).
- Add command line options for config file and log level ([#1](https://github.com/tmatilai/ruuvi2mqtt/pull/1)).
- Add device info for Home Assistant ([#63](https://github.com/tmatilai/ruuvi2mqtt/pull/63)). The MQTT paths and `unique_id`s have changed, so some cleanup on MQTT server and/or Home Assistant might be needed.
- Fix BDAddr on non-Linux platforms ([#104](https://github.com/tmatilai/ruuvi2mqtt/pull/104)). At least macOS should work now.
- Strip the release build binaries.

## 1.0.1 / 2022-02-20

- Fix container release.

## 1.0.0 / 2022-02-20

- First public release.
