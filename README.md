# Ruuvi2MQTT

Ruuvi2MQTT listens for [RuuviTag](https://ruuvi.com/ruuvitag/) sensor BLE events and sends MQTT messages, especially for [Home Assistant](https://www.home-assistant.io/) with [MQTT Discovery](https://www.home-assistant.io/docs/mqtt/discovery/).

There are also many other projects for integrating RuuviTags to Home Assistant. The main reasons for this project are:

- Possibility to run multiple bridges in case one receiver can't hear all the sensors (and/or for HA).
- I wanted to have a hobby project to learn and use Rust.

The second bullet means that the documentation, configuration, and code quality might not be top-notch. Improvements welcome!

---

## Requirements

The target platforms are Linux on amd64, arm64, and arm7 (Raspberry Pi). Because of [dbus](https://docs.rs/dbus/latest/dbus/) dependency, (cross) compiling and MUSL can get complicated. MacOS is supposed to work as well.

For local development, `make setup` installs the tools for both the Linux app
and the [ESP32 firmware](./ruuvi2mqtt-esp32/). Renovate keeps the pinned tool
versions current, so re-run it after pulling.

---

## Usage

Pre-build binaries and container images can be found in <https://github.com/tmatilai/ruuvi2mqtt>.

An example configuration file can be seen in [ruuvi2mqtt.yaml.example](./ruuvi2mqtt.yaml.example).
Configuration file is by default searched from the working directory, but the path can be specified with `--config` CLI option or `CONFIG_FILE` environment variable.
`--check-config` validates the configuration file and exits, without connecting to MQTT or Bluetooth.

Example command to run in a Docker container:

```bash
docker run --name ruuvi2mqtt -d --restart=unless-stopped \
    -v /run/dbus/system_bus_socket:/run/dbus/system_bus_socket \
    -v "$PWD/ruuvi2mqtt.yaml:/ruuvi2mqtt.yaml" \
    ghcr.io/tmatilai/ruuvi2mqtt:v1.5.1 \
    --log-level=DEBUG
```

The container runs as uid 65534 (`nobody`), so the mounted configuration file must be readable by that user, e.g. `chown 65534 ruuvi2mqtt.yaml`. The host needs BlueZ 5.51 or newer for non-root D-Bus access; on older hosts add `--user 0`.

The app exits with an error when it loses Bluetooth, e.g. when the adapter goes away or BlueZ restarts.
Run it under something that restarts it, such as systemd with `Restart=on-failure` or Docker with `--restart`.

### Status

Each instance publishes a retained status message to `<base_topic>/status/<client_id>`.
On connect it is `{"state":"online","version":"<version> (<commit>)"}`.
When the connection drops, the broker replaces it with `{"state":"offline"}`.

### Multiple instances

Several instances, and the [ESP32 firmware](./ruuvi2mqtt-esp32/), can hear the same tags.
Each instance subscribes to `<base_topic>/#`, so a publish from any of them restarts the tag's `throttle` timer on all instances.
When the timer runs out, each instance holds its next reading for 0–700 ms: up to 600 ms by signal strength, plus up to 100 ms of random jitter.
The closest instance usually publishes first, and the others see its message and skip theirs.
Instances with a similar signal take turns, and now and then both publish.
Each reading has a `publisher` field with the MQTT client ID of the instance that sent it.

Use the same `throttle` on every instance.
Different values work, but the instance with the shortest one publishes every tag it hears.

An ESP32 doesn't throttle: it publishes every tag it hears once per cycle (`BLE_SLEEP_DURATION` plus the awake time, about 67 s with the defaults).
With a `throttle` below that, the tags it hears get two messages per cycle, one from the ESP32 and one from a Linux instance.
That is harmless, though Home Assistant records a new state when `publisher` changes, as it is an attribute.
To avoid it, set `throttle` above the ESP32's cycle.
The ESP32 then publishes the tags it hears, and the Linux instances fill in when it misses one.
