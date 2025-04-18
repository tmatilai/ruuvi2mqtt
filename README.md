# Ruuvi2MQTT

Ruuvi2MQTT listens for [RuuviTag](https://ruuvi.com/ruuvitag/) sensor BLE events and sends MQTT messages, especially for [Home Assistant](https://www.home-assistant.io/) with [MQTT Discovery](https://www.home-assistant.io/docs/mqtt/discovery/).

There are also many other projects for integrating RuuviTags to Home Assistant. The main reasons for this project are:

- Possibility to run multiple bridges in case one receiver can't hear all the sensors (and/or for HA).
- I wanted to have a hobby project to learn and use Rust.

The second bullet means that the documentation, configuration, and code quality might not be top-notch. Improvements welcome!

---

## Requirements

The target platforms are Linux on amd64, arm64, and arm7 (Raspberry Pi). Because of [dbus](https://docs.rs/dbus/latest/dbus/) dependency, (cross) compiling and MUSL can get complicated. MacOS is supposed to work as well.

---

## Usage

Pre-build binaries and container images can be found in <https://github.com/tmatilai/ruuvi2mqtt>.

An example configuration file can be seen in [ruuvi2mqtt.yaml](./ruuvi2mqtt.yaml).
Configuration file is by default searched from the working directory, but the path can be specified with `--config` CLI option or `CONFIG_FILE` environment variable.

Example command to run in a Docker container:

```bash
docker run --name ruuvi2mqtt --rm \
    -v /run/dbus/system_bus_socket:/run/dbus/system_bus_socket \
    -v "$PWD/ruuvi2mqtt.yaml:/ruuvi2mqtt.yaml" \
    ghcr.io/tmatilai/ruuvi2mqtt:v1.3.4 \
    --log-level=DEBUG
```
