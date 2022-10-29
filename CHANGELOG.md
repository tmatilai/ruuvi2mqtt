# 1.1.1 / _Not released yet_


# 1.1.0 / 2022-10-29

- Add arm64 (aarch64) to build targets ([#49](https://github.com/tmatilai/ruuvi2mqtt/pull/49)).
- Add command line options for config file and log level ([#1](https://github.com/tmatilai/ruuvi2mqtt/pull/1)).
- Add device info for Home Assistant ([#63](https://github.com/tmatilai/ruuvi2mqtt/pull/63)). The MQTT paths and `unique_id`s have changed, so some cleanup on MQTT server and/or Home Assistant might be needed.
- Fix BDAddr on non-Linux platforms ([#104](https://github.com/tmatilai/ruuvi2mqtt/pull/104)). At least macOS should work now.
- Strip the release build binaries.

# 1.0.1 / 2022-02-20

- Fix container release.

# 1.0.0 / 2022-02-20

- First public release.
