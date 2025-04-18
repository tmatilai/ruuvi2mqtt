## _Not released yet_


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
