# Ruuvi2MQTT

Ruuvi2MQTT listens for the BLE broadcasts of
[RuuviTag](https://ruuvi.com/ruuvitag/) and [Ruuvi Air](https://ruuvi.com/air/)
sensors and publishes their readings over MQTT, especially for
[Home Assistant](https://www.home-assistant.io/) with
[MQTT Discovery](https://www.home-assistant.io/docs/mqtt/discovery/).

It has two parts:

- The app, documented here. It also publishes the Home Assistant discovery
  configs.
- The [ESP32 firmware](./ruuvi2mqtt-esp32/), which publishes the readings only.

Many other projects bring Ruuvi sensors into Home Assistant. This one runs
several receivers side by side, to cover sensors that a single receiver can't
hear.

---

## Requirements

Release builds are for Linux on amd64, arm64 and armv7 (e.g. a Raspberry Pi),
with BlueZ. It also runs on macOS, e.g. for development. macOS hides the
sender's MAC address, so there the app only matches readings whose payload
carries the full MAC, such as data format 5. Other platforms that
[btleplug](https://github.com/deviceplug/btleplug) supports are untested.

---

## Usage

Prebuilt binaries are on the
[releases page](https://github.com/tmatilai/ruuvi2mqtt/releases), and container
images at `ghcr.io/tmatilai/ruuvi2mqtt`.

[ruuvi2mqtt.yaml.example](./ruuvi2mqtt.yaml.example) is an example configuration
file. The app reads `ruuvi2mqtt.yaml` from the working directory, or the file
given with `--config` or the `CONFIG_FILE` environment variable.
`--check-config` validates the configuration file and exits, without connecting
to MQTT or Bluetooth.

Set `MQTT_USER` and `MQTT_PASSWORD` to supply MQTT credentials through the
environment. Each variable overrides its corresponding `mqtt.user` or
`mqtt.password` value in the configuration file. Unset variables leave the
file's values in use; empty variables override them with empty strings. You can
omit either setting from the file when supplying it through the environment. For
example, keep `mqtt.user` in the file and run:

```bash
export MQTT_PASSWORD='your-password'
ruuvi2mqtt
```

Example command to run in a Docker container:

```bash
docker run --name ruuvi2mqtt -d --restart=unless-stopped \
    -v /run/dbus/system_bus_socket:/run/dbus/system_bus_socket \
    -v "$PWD/ruuvi2mqtt.yaml:/ruuvi2mqtt.yaml" \
    ghcr.io/tmatilai/ruuvi2mqtt:v1.6.0 \
    --log-level=DEBUG
```

The container runs as uid 65534 (`nobody`), so the mounted configuration file
must be readable by that user, e.g. `chown 65534 ruuvi2mqtt.yaml`. The host
needs BlueZ 5.51 or newer for non-root D-Bus access; on older hosts add
`--user 0`.

The app exits with an error when it loses Bluetooth, e.g. when the adapter goes
away or BlueZ restarts. Run it under something that restarts it, such as systemd
with `Restart=on-failure` or Docker with `--restart`.

### Ruuvi Air

Ruuvi Air support is a preview, tested with a simulator but not yet with a real
Air. Set `model: air` on an Air's device to give it an Air's Home Assistant
entities instead of a RuuviTag's. When the model changes, the app removes the
old model's entities.

An Air sends two data formats. Format 6 is a legacy advertisement with PM2.5
only. E1 is a Bluetooth 5 extended advertisement with PM1, PM2.5, PM4 and PM10.
The app publishes E1, and format 6 only when no E1 has arrived from the Air in
the last minute. E1 needs a Bluetooth 5 adapter that scans for extended
advertisements, which not all do. The ESP32 firmware receives only format 6 for
now.

With several instances, any that receives only format 6 publishes too, an ESP32
included, so PM1, PM4 and PM10 may come and go. Give every app instance that
hears the Air a Bluetooth 5 adapter.

The Air doesn't broadcast its indoor air quality score, so the app computes
`iaqs` from PM2.5 and CO2 with Ruuvi's formula.

### Status

Each instance publishes a retained status message to
`<base_topic>/status/<client_id>`. On connect it is
`{"state":"online","version":"<version> (<commit>)"}`. When the connection
drops, the broker replaces it with `{"state":"offline"}`.

### Multiple instances

Several instances, and the [ESP32 firmware](./ruuvi2mqtt-esp32/), can hear the
same tags. Each instance subscribes to `<base_topic>/#`, so a publish from any
of them restarts the tag's `throttle` timer on all instances. When the timer
runs out, each instance holds its next reading for 0–700 ms: up to 600 ms by
signal strength, plus up to 100 ms of random jitter. The closest instance
usually publishes first, and the others see its message and skip theirs.
Instances with a similar signal take turns, and now and then both publish. Each
reading has a `publisher` field with the MQTT client ID of the instance that
sent it.

Use the same `throttle` on every instance. Different values work, but the
instance with the shortest one publishes every tag it hears.

An ESP32 doesn't throttle, and doesn't subscribe to the readings: it publishes
every tag it heard once per cycle. The cycle is `BLE_SCAN_DURATION` when
scanning continuously (60 s by default), and `BLE_SLEEP_DURATION` plus the awake
time in the deep-sleep mode. With a `throttle` below that, the tags it hears get
two messages per cycle, one from the ESP32 and one from an app instance. That is
harmless, but Home Assistant records a new state each time `publisher` changes,
as it is an attribute. To avoid it, set `throttle` above the ESP32's cycle. The
ESP32 then publishes the tags it hears, and the app instances fill in when it
misses one.

---

## Development

`make setup` installs the tools for the app and the ESP32 firmware. Renovate
bumps the pinned versions, so re-run it after pulling. `make lint` also formats
the Markdown files with Prettier, which needs Node.js. `make help` lists the
other targets.
