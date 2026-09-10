## _Not released yet_

### ruuvi2mqtt

- Fix panicking in a background task if BLE peripheral properties are missing.
- Rename the example configuration to `ruuvi2mqtt.yaml.example` and git-ignore local `ruuvi2mqtt*.yaml` files.
- Warn if the configuration file is readable by other users.
- Exit with an error if the BLE event stream ends (e.g. BlueZ restart), instead of running idle.
- Fix data format 3 tags logging `BDAddr not found` on every advertisement; use the advertiser address when the payload has no MAC.
- Drop sensor readings instead of queueing them when the broker is unreachable, to avoid unbounded memory growth and a flood of stale readings on reconnect.
- Only fail startup when no platform TLS certificates are found; warn about individual load errors.
- Validate `mqtt.base_topic`: reject wildcards and a leading `$`, and trim a trailing `/`.
- Add a CA certificate store to the Docker image, so `tls: true` works without `ca_file`.
- Replace the archived `serde_yaml` crate with its fork `serde_norway`.
- Run the Docker image as an unprivileged user (uid 65534); the mounted configuration file must be readable by it.

### ruuvi2mqtt-esp32

- Publish diagnostic information to MQTT.
- Deep sleep instead of boot looping after a panic or watchdog reset, which used to drain the battery.
- Fix the build script not re-running when `MQTT_CA_FILE` is set or unset, which could embed a stale CA certificate.
- Skip the cycle and sleep 4x longer after a brownout reset, instead of boot looping on a weak battery.
- Add a hard timeout for the whole cycle. The task watchdog does not catch a blocked Wi-Fi or MQTT wait.
- Wait for the MQTT connection and for publish acknowledgements before deep sleep. Readings were lost when the broker connected slower than the BLE scan.
- Report a failed BLE scan in the `error` field of the diagnostics message.
- Validate the compile-time configuration at build time (Wi-Fi credential lengths, static IP addresses, durations, LED GPIO) instead of panicking on the device.
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
- Build the container images from `scratch`, and only include a statically linked (MUSL) binary.
  The default path of the config file is changed to `/ruuvi2mqtt.yaml`.

## 1.2.0 / 2022-12-22

- Expose diagnostic information ([#124](https://github.com/tmatilai/ruuvi2mqtt/pull/124)):
    * Battery voltage
    * Low batter indicator
    * Transmitter power
    * All data in attributes for all sensors
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
