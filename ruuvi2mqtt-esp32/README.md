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

Wi-Fi and BLE share one 2.4 GHz radio, so the scan gets only part of the
airtime until Wi-Fi has connected. `BLE_SCAN_MODE=sequential` scans first and
then starts Wi-Fi. That gives the scan the whole radio, but adds the Wi-Fi
and MQTT connect time to the awake time.

To further reduce active time, the firmware caches the Wi-Fi channel and BSSID
in NVS flash after the first connection. On subsequent boots it connects
directly without scanning all channels, saving several seconds per cycle. A
static IP can also be configured to skip DHCP.

Unlike the Linux app, the ESP32 does **not** subscribe to MQTT for
cross-gateway throttle coordination — each cycle simply publishes every sensor
it heard during the scan window. When Linux instances hear the same tags and
their `throttle` is below the ESP32's full cycle (`BLE_SLEEP_DURATION` plus the
awake time, ~67s with the defaults), those tags get two messages per cycle.
That is harmless. To avoid it, set the `throttle` above the cycle; the Linux
instances then fill in only when the ESP32 misses a tag.

Each cycle also publishes [diagnostics](#diagnostics).

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
| ESP32-C5 | RISC-V | `esp32c5` |
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

This installs the pinned versions of `ldproxy`, `espflash`, `espup`, and the
Xtensa Rust toolchain. Renovate bumps the pins in the `Makefile`, so re-run it
after pulling.

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
| `MQTT_CLIENT_ID` | `mqtt.client_id` | *hostname* | MQTT client ID, also sent as `publisher` in each reading |
| `MQTT_USER` | `mqtt.user` | *(anonymous)* | MQTT username |
| `MQTT_PASSWORD` | `mqtt.password` | *(anonymous)* | MQTT password |
| `MQTT_BASE_TOPIC` | `mqtt.base_topic` | `ruuvi2mqtt` | Topic prefix |
| `BLE_SCAN_DURATION` | — | `5` | BLE scan duration (seconds). Set close to your Wi-Fi + MQTT connect time (see boot log), min 3s |
| `BLE_SCAN_MODE` | — | `parallel` | `parallel` scans while Wi-Fi connects. `sequential` scans first, then starts Wi-Fi |
| `BLE_SLEEP_DURATION` | — | `60` | Deep sleep between cycles (seconds) |
| `ANTENNA` | — | *(unset)* | Seeed XIAO ESP32-C6 only: `internal` or `external` antenna. See [Antenna](#antenna) |
| `LED_MODE` | — | *(unset)* | `on` to light while awake, `off` to turn off at boot, or unset to do nothing |
| `LED_TYPE` | — | `ws2812` | `ws2812` or `gpio` |
| `LED_GPIO` | — | — | LED pin number. Required with `LED_MODE` |
| `LED_COLOR` | — | `000020` | WS2812 color as hex `RRGGBB`. Low values keep it dim |
| `LOG_LEVEL` | — | `info` | Log level for app code (library code stays at info) |

These can be set in the environment, in `.envrc` (with direnv), or in
per-device configuration files (see below).

#### Known board LED configurations

| Board | `LED_TYPE` | `LED_GPIO` |
|---|---|---|
| ESP32-C5-DevKitC-1 | `ws2812` | `27` |
| Seeed XIAO-ESP32-C6 | `ws2812` | `15` |
| Beetle ESP32 C6 Mini (DFRobot DFR1117) | `gpio` | `15` |
| ESP32-S3 AI Camera (DFRobot DFR1154) | `gpio` | `3` |
| ESP32-CAM | `ws2812` | `33` |

#### Antenna

The Seeed XIAO ESP32-C6 has an RF switch between its ceramic antenna and a
U.FL connector ([Seeed wiki][xiao-c6]). `ANTENNA` selects one. The switch
is powered off in deep sleep, since it draws more than the chip does.

Without `ANTENNA`, the firmware doesn't touch the switch, and the XIAO uses
its internal antenna. Other boards use its pins for other things, so the
build fails if `ANTENNA` is set for any chip but the ESP32-C6.

[xiao-c6]: https://wiki.seeedstudio.com/xiao_esp32c6_getting_started/

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

## Diagnostics

Each cycle ends with a message on
`{MQTT_BASE_TOPIC}/diagnostics/{DEVICE_HOSTNAME}`. It shows what the board
heard and when, so a missed tag can be traced to range or to the radio.

```json
{
  "firmware": "ruuvi2mqtt-esp32 1.5.2-dev (c43f162)",
  "client_id": "ruuvi2mqtt-kitchen",
  "chip": "esp32c6",
  "cycle": 42,
  "settings": { "scan_mode": "parallel", "scan_duration": 5, "antenna": "external" },
  "wifi_rssi": -61,
  "wifi_channel": 6,
  "wifi_cache": "hit",
  "free_heap": 151000,
  "scan_start_ms": 180,
  "scan_end_ms": 5190,
  "wifi_ms": 1450,
  "mqtt_ms": 2210,
  "cycle_ms": 5230,
  "tags": {
    "f817624e4c0b": { "rssi": -74, "adverts": 2, "first_ms": 1210 }
  },
  "adverts_before_wifi": 1,
  "adverts_after_wifi": 1,
  "other_adverts_before_wifi": 35,
  "other_adverts_after_wifi": 61
}
```

| Field | Description |
|---|---|
| `firmware` | Package version and git commit, with `-dirty` for local changes |
| `client_id` | `MQTT_CLIENT_ID`, the `publisher` in the readings |
| `chip` | ESP-IDF target |
| `cycle` | Cycles since a cold boot or a crash, this one included. A gap means cycles without diagnostics |
| `settings` | The build-time settings that A/B tests compare: `BLE_SCAN_MODE`, `BLE_SCAN_DURATION` and `ANTENNA` (left out when unset) |
| `wifi_rssi`, `wifi_channel` | The access point's signal (dBm) and channel |
| `wifi_cache` | `hit`: connected with the cached channel and BSSID. `miss`: nothing cached. `stale`: the cache failed, so a full scan |
| `free_heap` | Free heap in bytes |
| `scan_start_ms`, `scan_end_ms` | When the BLE scan started and finished |
| `wifi_ms`, `mqtt_ms` | When Wi-Fi and the broker connection were up |
| `cycle_ms` | When the message was built |
| `tags` | Per Ruuvi MAC: the strongest RSSI (dBm), the advertisements received, and when the first one arrived |
| `adverts_before_wifi`, `adverts_after_wifi` | Ruuvi advertisements before and after Wi-Fi connected |
| `other_adverts_before_wifi`, `other_adverts_after_wifi` | Other BLE advertisements, the same way. They show the scan's airtime |
| `error` | Only when the BLE scan failed |

All times are in milliseconds from the start of the cycle. The test bed's
[checker](../checker/README.md) turns the counts into rates over many cycles.

## Security

The built firmware embeds `WIFI_PASS` and `MQTT_PASSWORD` as plaintext
constants in flash. Don't share or publish built images, and note that
physical access to a board is enough to read the credentials back with
`espflash`/`esptool`.

ESP-IDF [flash encryption][flash-enc] mitigates this, and can be enabled with
`CONFIG_SECURE_FLASH_ENC_ENABLED` in `sdkconfig.defaults`. It is off by design:
the first boot with it enabled burns eFuses irreversibly and changes how the
board is flashed. Read the Espressif documentation before enabling it.

[flash-enc]: https://docs.espressif.com/projects/esp-idf/en/stable/esp32/security/flash-encryption.html
