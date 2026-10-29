# ruuvi2mqtt-esp32

ESP32 firmware that scans for Ruuvi BLE advertisements and publishes JSON
readings over Wi-Fi to an MQTT broker. Topic and payload format matches the
Linux `ruuvi2mqtt` app.

By default, the firmware scans BLE all the time and stays connected to Wi-Fi
and MQTT, for a board on mains power. A cycle is one scan of
`BLE_SCAN_DURATION` (60 s). As the next scan runs, the board publishes each
tag heard in the cycle once, with its latest reading.

On a battery, the deep-sleep modes save energy with a scan-publish-sleep
cycle. Each cycle scans BLE and connects Wi-Fi in parallel, publishes all
unique readings over MQTT, then enters deep sleep (CPU, radios, and RAM
powered off). On wake the chip reboots and reconnects. With the defaults (5s
scan, 60s sleep) the active time is ~7s per ~67s cycle, reducing average
current draw to roughly a tenth of always-on operation.

Wi-Fi and BLE share one 2.4 GHz radio, so the scan gets only part of the
airtime while Wi-Fi connects. `BLE_SCAN_MODE=sequential` scans first and
then starts Wi-Fi. That gives the scan the whole radio, but adds the Wi-Fi
and MQTT connect time to the awake time.

To further reduce active time, the firmware caches the Wi-Fi channel and BSSID
in NVS flash after the first connection. On subsequent boots it connects
directly without scanning all channels, saving several seconds per cycle. A
static IP can also be configured to skip DHCP.

Unlike the Linux app, the ESP32 does **not** subscribe to the readings for
cross-gateway throttle coordination — each cycle simply publishes every sensor
it heard during the scan window. When Linux instances hear the same tags and
their `throttle` is below the ESP32's cycle (`BLE_SCAN_DURATION` when
scanning continuously, otherwise `BLE_SLEEP_DURATION` plus the awake time),
those tags get two messages per cycle.
That is harmless. To avoid it, set the `throttle` above the cycle; the Linux
instances then fill in only when the ESP32 misses a tag.

Each cycle also publishes [diagnostics](#diagnostics). Scanning
continuously, the board also serves a [status page](#status-page) for
finding its spot.

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
| ESP32 | Xtensa | `esp32` |
| ESP32-C3 | RISC-V | `esp32c3` |
| ESP32-C5 | RISC-V | `esp32c5` |
| ESP32-C6 | RISC-V | `esp32c6` (default) |
| ESP32-S3 | Xtensa | `esp32s3` |

Each chip builds in its own target directory, `target/<chip>/`, unless
`CARGO_TARGET_DIR` is set. esp-idf-sys doesn't rebuild ESP-IDF when its
config in `Cargo.toml` changes, so `make build` and `make lint` clean its
build then.

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
| `WIFI_BAND` | — | *(both)* | Dual-band chips only (ESP32-C5): `2.4` or `5` GHz. On the C5, a full scan of 2.4 GHz takes about 2.5 s, of both bands about 13 s |
| `DEVICE_HOSTNAME` | — | `ruuvi2mqtt-esp32` | Hostname (DHCP + default MQTT client ID) |
| `MQTT_PORT` | `mqtt.port` | `1883` / `8883` | MQTT broker port (default depends on TLS) |
| `MQTT_TLS` | `mqtt.tls` | `false` | Set to `true` to enable TLS (`mqtts://`) |
| `MQTT_CA_FILE` | `mqtt.ca_file` | — | Path to a PEM CA cert, embedded at compile time |
| `MQTT_TLS_INSECURE` | `mqtt.tls_insecure` | `false` | Set to `true` to skip cert hostname check |
| `MQTT_CLIENT_ID` | `mqtt.client_id` | *hostname* | MQTT client ID, also sent as `publisher` in each reading |
| `MQTT_USER` | `mqtt.user` | *(anonymous)* | MQTT username |
| `MQTT_PASSWORD` | `mqtt.password` | *(anonymous)* | MQTT password |
| `MQTT_BASE_TOPIC` | `mqtt.base_topic` | `ruuvi2mqtt` | Topic prefix |
| `BLE_SCAN_MODE` | — | `continuous` | `continuous` scans all the time. `parallel` scans while Wi-Fi connects, then deep sleeps. `sequential` scans first, then starts Wi-Fi, and deep sleeps |
| `BLE_SCAN_DURATION` | — | `60` / `5` | BLE scan duration (seconds). Continuous: the cycle, `60`. Otherwise `5`: set it close to your Wi-Fi + MQTT connect time (see boot log), min 3s |
| `BLE_SLEEP_DURATION` | — | `60` | Deep sleep between cycles (seconds). Continuous: the wait awake before a restart after a failure |
| `USB_STAY_AWAKE` | — | `true` | `false` always deep sleeps. See [Build & flash](#build--flash) |
| `ANTENNA` | — | *(unset)* | Seeed XIAO ESP32-C6 only, and needed there: `internal` or `external` antenna. See [Antenna](#antenna) |
| `LED_MODE` | — | `on` with `LED_GPIO` | `on`, `errors` or `off`. Without `LED_GPIO`, the LED is left alone. See [LED](#led) |
| `LED_TYPE` | — | `ws2812` | `ws2812` or `gpio` |
| `LED_ACTIVE_LOW` | — | `false` | `true` for a `gpio` LED that lights when its pin is low |
| `LED_GPIO` | — | — | LED pin number. Required with `LED_MODE` |
| `LED_COLOR` | — | `000020` | WS2812 color as hex `RRGGBB`. Low values keep it dim |
| `LOG_LEVEL` | — | `info` | Log level for app code (library code stays at info) |

These can be set in the environment, in `.envrc` (with direnv), or in
per-device configuration files (see below).

#### LED

Scanning continuously, `LED_MODE=on` breathes slowly and dimly every 6 s,
to show that the board runs. In the deep-sleep modes, it lights dimly while
the board is awake.

On a failure, the LED blinks twice every 3 s instead, at half brightness
and in red on a WS2812. The blinks go on while Wi-Fi is down, and until a
cycle has published its readings and scanned without errors. In the
deep-sleep modes, a failed cycle sleeps right away, so the blinks show in
the next wake. `errors` keeps the LED dark except for the blinks, for a
room where a glow would bother.

#### Known board LED configurations

| Board | `LED_TYPE` | `LED_GPIO` |
|---|---|---|
| Beetle ESP32 C6 Mini (DFRobot DFR1117) | `gpio` | `15` |
| ESP32-C5-DevKitC-1 | `ws2812` | `27` |
| ESP32-C6-DevKitC-1 | `ws2812` | `8` |
| ESP32-CAM | `ws2812` | `33` |
| ESP32-S3 AI Camera (DFRobot DFR1154) | `gpio` | `3` |
| Seeed XIAO-ESP32-C6 | `gpio`, `LED_ACTIVE_LOW=true` | `15` |

#### Antenna

The Seeed XIAO ESP32-C6 has an RF switch between its ceramic antenna and a
U.FL connector ([Seeed wiki][xiao-c6]). `ANTENNA` selects one. The switch
is powered off in deep sleep, since it draws more than the chip does.

Without `ANTENNA`, the firmware doesn't touch the switch. It then gets no
power, and the XIAO hears little on Wi-Fi and BLE, so set `ANTENNA` on
every XIAO. Other boards use its pins for other things, so the build fails
if `ANTENNA` is set for any chip but the ESP32-C6.

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

**Register a device** by attaching it and running:

```sh
make register DEVICE=kitchen
```

This records the board's MAC address in `devices/devices.conf` and creates a
`devices/kitchen.mk` file from the template. Edit the `.mk` file to
set device-specific overrides (static IP, hostname, chip type, etc.). With
several boards attached, choose one with `ESPFLASH_PORT=<port>`.

**List the attached boards** with `make boards`:

```
PORT                     USB    MAC               DEVICE
/dev/cu.usbmodem1101     native aa:bb:cc:dd:ee:f0 kitchen
/dev/cu.usbserial-210    uart   aa:bb:cc:dd:ee:f1 garage
```

`native` is the chip's own USB port, `uart` a USB-UART bridge. A board on
its own port reports its MAC as the USB serial number. For a board behind a
bridge, `make register` records the bridge's serial number. Registered
boards are found without touching them. An unknown board behind a bridge is
identified with `espflash board-info`, which resets it. That happens in
`make boards`, and when looking for a board registered without the serial
number. Run `make register` again to add it.

**Auto-detection:** when you run `make flash` (or `build`, `lint`,
`monitor`, `log`) without `DEVICE`, the Makefile identifies the attached
board and loads its `.mk` file. With several boards attached, it lists
them and asks for `DEVICE`. A target that talks to the board finds its
port from `DEVICE`, and never uses another board.

```sh
make flash DEVICE=kitchen                 # flash the board named kitchen
make flash DEVICE=                        # skip auto-detection, use env vars only
make flash ESPFLASH_PORT=/dev/cu.usbserial-210   # the board on this port
```

The Makefile waits up to `WAIT` seconds (default 75) for a sleeping board
to wake.

> Device config files are git-ignored — only the examples are committed.

---

## Build & flash

```sh
make flash               # build and flash (first build is slow — ESP-IDF is compiled from source)
make flash monitor       # build, flash, and open the serial monitor
make build               # build only
make monitor             # open the interactive serial monitor
make log SECS=70         # print the serial output for 70 s, without prompts
```

All but `make monitor` run without prompts, so scripts and agents can use
them.

In the deep-sleep modes, deep sleep turns off the USB port built into the
ESP32-C3, -C5, -C6, and -S3. While a computer is connected to that port,
the board waits awake between cycles and restarts instead, so it can be
flashed and monitored at any time. A charger or power bank doesn't count. A
port behind a USB-UART bridge, as on ESP32 boards and the DevKitC's UART
port, stays up in deep sleep.
`USB_STAY_AWAKE=false` deep sleeps anyway, for example to measure the sleep
current.

All available targets:

```sh
make help
```

## Status page

Scanning continuously, the board serves a page on
`http://{DEVICE_HOSTNAME}.local/` (mDNS), or on its IP address. It answers
one question: is this a good spot for the board? Put the board there, open
the page on a phone, and it updates every 2 s. When the board stops
answering, the top says so after 6 s.

The top says *Good*, *Fair* or *Poor spot* and lists what isn't good:

- **Wi-Fi:** the access point's RSSI. Good from −67 dBm, fair to −75 dBm,
  poor below that. A drop since boot makes it fair at best.
- **Publishing:** fair until the first cycle, poor while the last cycle
  failed, and fair while any cycle since boot did.
- **The target tags:** tap a tag to make it one that the spot should cover.
  The phone remembers them. A tag is graded by its mean RSSI over the last
  minute (good from −80 dBm, fair to −90 dBm), and by its reports a minute
  (good from 10, fair from 3). One not heard is poor. For the first 20 s
  after boot, the rates are left out.

Without targets, only Wi-Fi and publishing count. Each tag shows its mean
RSSI, the minute's range and its reports a minute. The names come from the
Home Assistant discovery configs that the Linux app keeps retained on the
broker, so tags it doesn't know show their MAC.

`/api/status` has the same as JSON, for scripts. `?tags=<mac>,<mac>` sets
the targets, with the MACs in the topic format.

```json
{
  "hostname": "ruuvi2mqtt-kitchen",
  "firmware": "ruuvi2mqtt-esp32 1.6.1-dev (4518da5)",
  "antenna": "internal",
  "uptime_s": 754,
  "window_s": 60,
  "measuring": false,
  "verdict": "fair",
  "problems": [{ "grade": "fair", "text": "Bedroom: -86 dBm" }],
  "wifi": { "rssi": -61, "channel": 6, "drops": 0, "grade": "good" },
  "publishing": { "cycles": 12, "failed": 0, "grade": "good" },
  "tags": [
    {
      "mac": "f817624e4c0b", "name": "Bedroom", "target": true,
      "reports_per_min": 38, "rssi": -86, "rssi_min": -93, "rssi_max": -81,
      "last_heard_s": 1, "grade": "fair"
    }
  ]
}
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
| `wifi_cache` | `hit`: connected with the cached channel and BSSID. `miss`: nothing cached. `stale`: the cache failed, so a full scan. When scanning continuously, from the first connection after boot |
| `free_heap` | Free heap in bytes |
| `scan_start_ms`, `scan_end_ms` | When the BLE scan started and finished |
| `wifi_ms`, `mqtt_ms` | When Wi-Fi and the broker connection were up. When scanning continuously, `0` if they were up all along |
| `cycle_ms` | When the message was built |
| `tags` | Per Ruuvi MAC: the strongest RSSI (dBm), the advertisements received, and when the first one arrived |
| `adverts_before_wifi`, `adverts_after_wifi` | Ruuvi advertisements before and after Wi-Fi connected. When scanning continuously, while Wi-Fi was down and up |
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

The status page has no login. It only reads, and shows nothing secret,
but anyone on the network can open it. The board takes an IPv6 address
from the router too, so keep the firewall blocking incoming connections,
as it does by default.

[flash-enc]: https://docs.espressif.com/projects/esp-idf/en/stable/esp32/security/flash-encryption.html
