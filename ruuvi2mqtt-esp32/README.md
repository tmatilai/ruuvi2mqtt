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

The default target is **ESP32-C6** (RISC-V). Other chips (ESP32, ESP32-S3,
ESP32-C3) are supported via the `CHIP` Makefile variable — see
[Building for other chips](#building-for-other-chips).

---

## Prerequisites

Install the required host tools. Run these from **outside** this directory
(the `rust-toolchain.toml` here pins a specific channel):

```sh
cd ~
cargo install ldproxy espflash
```

> The first `cargo build` downloads and compiles ESP-IDF from source — expect
> 10–20 minutes. Subsequent builds are incremental.

---

## Configuration

All settings are baked in at compile-time via environment variables (typically
set in `.envrc` with direnv).

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
| `LOG_LEVEL` | — | `info` | Log level for app code (library code stays at info) |

---

## Build & flash

```sh
WIFI_SSID=MyNetwork \
WIFI_PASS=secret \
DEVICE_HOSTNAME=ruuvi-kitchen \
MQTT_SERVER=192.168.1.10 \
MQTT_USER=myuser \
MQTT_PASSWORD=mypassword \
cargo run --release   # builds, flashes, and opens serial monitor
```

Or via Make:

```sh
make flash
```

To flash a pre-built binary or just monitor:

```sh
espflash flash --monitor target/riscv32imac-esp-espidf/release/ruuvi2mqtt-esp32
espflash monitor
```

---

## Building for other chips

The Makefile accepts a `CHIP` variable to target a different ESP32 variant:

```sh
make build CHIP=esp32       # original ESP32 (Xtensa)
make flash CHIP=esp32s3     # ESP32-S3 (Xtensa)
make build CHIP=esp32c3     # ESP32-C3 (RISC-V)
```

Xtensa chips (ESP32, ESP32-S3) require the Espressif Rust fork. Install it
with:

```sh
cargo install espup
espup install        # downloads the 'esp' toolchain + Xtensa LLVM
```

Then load the toolchain environment (or let direnv handle it via `.envrc`):

```sh
source "$HOME/export-esp.sh"
```

---

## MQTT topics

```
ruuvi2mqtt/<macaddress>
```

Example payload:

```json
{"temperature":22.50,"humidity":45.20,"pressure":1013.25,"battery":2.950,"battery_low":false,"tx_power":4}
```

The topic and payload format are identical to the Linux `ruuvi2mqtt` app, so
both can publish to the same broker simultaneously.
