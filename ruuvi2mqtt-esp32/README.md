# ruuvi2mqtt-esp32

ESP32 firmware that scans for Ruuvi BLE advertisements and publishes JSON
readings over Wi-Fi to an MQTT broker. Topic and payload format matches the
Linux `ruuvi2mqtt` app.

The firmware operates in a scan-publish-sleep cycle to conserve energy. Each
cycle scans BLE and connects Wi-Fi in parallel, publishes all unique readings
over MQTT, then enters deep sleep (CPU, radios, and RAM powered off). On wake
the chip reboots and reconnects. With the defaults (5s scan, 60s sleep) the
active time is ~7s per ~67s cycle, reducing average current draw to roughly
a tenth of always-on operation.

To further reduce active time, the firmware caches the Wi-Fi channel and BSSID
in NVS flash after the first connection. On subsequent boots it connects
directly without scanning all channels, saving several seconds per cycle. A
static IP can also be configured to skip DHCP.

Unlike the Linux app, the ESP32 does **not** subscribe to MQTT for
cross-gateway throttle coordination — each cycle simply publishes every sensor
it heard during the scan window.

This firmware does **not** publish Home Assistant MQTT discovery config.
At least one instance of the full [`ruuvi2mqtt`](/) Linux app must
be running and connected to the same broker for HA to recognise the sensors.
The ESP32 only publishes sensor readings; the Linux app advertises the device
definitions (names, units, entity types) and keeps them retained.

---

## Supported chips

The default target is **ESP32-C6**. Other chips are supported via the `CHIP`
Makefile variable:

| Chip | Architecture | `CHIP` value |
|------|-------------|--------------|
| ESP32-C6 | RISC-V | `esp32c6` (default) |
| ESP32-C3 | RISC-V | `esp32c3` |
| ESP32 | Xtensa | `esp32` |
| ESP32-S3 | Xtensa | `esp32s3` |

---

## Setup

Install all required toolchains and tools:

```sh
make setup
```

This installs `ldproxy`, `espflash`, `espup`, and the Xtensa Rust toolchain
(if not already present).

---

## Configuration

All settings are baked in at compile-time via environment variables.

### Required

| Variable | Linux YAML equivalent | Description |
|---|---|---|
| `WIFI_SSID` | — | Wi-Fi SSID |
| `WIFI_PASS` | — | Wi-Fi password |
| `MQTT_SERVER` | `mqtt.server` | MQTT broker hostname or IP |

### Optional

| Variable | Linux YAML equivalent | Default | Description |
|---|---|---|---|
| `WIFI_IP` | — | *(DHCP)* | Static IP address (e.g. `192.168.1.50`) |
| `WIFI_GATEWAY` | — | — | Gateway (required when `WIFI_IP` is set) |
| `WIFI_NETMASK` | — | `255.255.255.0` | Subnet mask for static IP |
| `WIFI_DNS` | — | *gateway* | DNS server for static IP |
| `DEVICE_HOSTNAME` | — | `ruuvi2mqtt-esp32` | Hostname (DHCP + default MQTT client ID) |
| `MQTT_PORT` | `mqtt.port` | `1883` / `8883` | MQTT broker port (default depends on TLS) |
| `MQTT_TLS` | `mqtt.tls` | `false` | Set to `true` to enable TLS (`mqtts://`) |
| `MQTT_CA_FILE` | `mqtt.ca_file` | — | Path to a PEM CA cert, embedded at compile time |
| `MQTT_TLS_INSECURE` | `mqtt.tls_insecure` | `false` | Set to `true` to skip cert hostname check |
| `MQTT_CLIENT_ID` | `mqtt.client_id` | *hostname* | MQTT client ID |
| `MQTT_USER` | `mqtt.user` | *(anonymous)* | MQTT username |
| `MQTT_PASSWORD` | `mqtt.password` | *(anonymous)* | MQTT password |
| `MQTT_BASE_TOPIC` | `mqtt.base_topic` | `ruuvi2mqtt` | Topic prefix |
| `BLE_SCAN_DURATION` | — | `5` | BLE scan duration (seconds). Set close to your Wi-Fi + MQTT connect time (see boot log), min 3s |
| `BLE_SLEEP_DURATION` | — | `60` | Deep sleep between cycles (seconds) |
| `LED_MODE` | — | *(unset)* | `on` to light while awake, `off` to turn off at boot, or unset to do nothing |
| `LED_TYPE` | — | `ws2812` | `ws2812` or `gpio` |
| `LED_GPIO` | — | `15` | LED pin number |
| `LOG_LEVEL` | — | `info` | Log level for app code (library code stays at info) |

These can be set in the environment, in `.envrc` (with direnv), or in
per-device configuration files (see below).

#### Known board LED configurations

| Board | `LED_TYPE` | `LED_GPIO` |
|---|---|---|
| Seeed XIAO-ESP32-C6 | `ws2812` | `15` |
| Beetle ESP32 C6 Mini (DFRobot DFR1117) | `gpio` | `15` |
| ESP32-S3 AI Camera (DFRobot DFR1154) | `gpio` | `3` |
| ESP32-CAM | `ws2812` | `33` |

---

## Device configuration

For a single board, set the required variables in the environment (e.g. in
`.envrc` with direnv) and run `make flash`. No device registration is needed —
auto-detection is only active when `devices/devices.conf` exists. To target a
non-default chip:

```sh
make flash CHIP=esp32
```

### Managing multiple devices

When managing multiple boards, each device can have its own configuration file
and be auto-detected by MAC address.

**Register a device** by connecting it and running:

```sh
make register DEVICE=kitchen
```

This records the board's MAC address in `devices/devices.conf` and creates a
`devices/kitchen.mk` file from the template. Edit the `.mk` file to
set device-specific overrides (static IP, hostname, chip type, etc.).

**Auto-detection:** when you run `make flash` (or `build`, `lint`, `monitor`)
without specifying `DEVICE`, the Makefile queries the connected board's MAC
address via `espflash board-info` and looks it up in `devices/devices.conf`.
If found, the matching `.mk` file is loaded automatically.

To target a registered device explicitly, or to skip auto-detection:

```sh
make flash DEVICE=kitchen   # use a specific device config
make flash DEVICE=           # skip auto-detection, use env vars only
```

> Device config files are git-ignored — only the examples are committed.

---

## Build & flash

```sh
make flash               # build, flash, and open serial monitor (first build is slow — ESP-IDF is compiled from source)
make build               # build only
make monitor             # open serial monitor (no build/flash)
```

All available targets:

```sh
make help
```
