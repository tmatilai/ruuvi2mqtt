# ruuvi2mqtt-esp32

ESP32 firmware that scans for Ruuvi BLE advertisements and publishes JSON
readings over Wi-Fi to an MQTT broker. The topics and payloads are the same as
the `ruuvi2mqtt` app's.

By default, the firmware scans BLE all the time and stays connected to Wi-Fi and
MQTT, for a board on mains power. A cycle is one scan, which lasts
`BLE_SCAN_DURATION` (60 s by default). As the next scan runs, the board
publishes each tag heard in the cycle once, with its latest reading.

For a board on a battery, `BLE_SCAN_MODE=deep-sleep` runs one cycle per wake.
The board scans BLE while it connects to Wi-Fi, publishes the latest reading of
each tag it heard, and then deep sleeps with the CPU, the radios and the main
RAM powered off. On wake, the chip reboots. With the defaults, a 5 s scan and a
60 s sleep, the board is awake about 7 s of each 67 s cycle. That draws roughly
a tenth of the current of staying awake.

After the first connection, the firmware caches the Wi-Fi channel and BSSID in
NVS flash. Later boots connect without scanning all channels, which saves a few
seconds per cycle. A static IP also skips DHCP.

Scanning continuously, the board roams between the network's access points.
After a drop, it retries the access point it was on, then reconnects to the
strongest one. Below −70 dBm, it scans the network and switches to an access
point at least 8 dB stronger. A scan takes about 5 s, and BLE hears little
meanwhile. If none is stronger, it scans again every 10 minutes while the link
stays weak.

Unlike the app, the firmware doesn't throttle: each cycle publishes every tag it
heard. See [Multiple instances](../README.md#multiple-instances) for running it
beside app instances.

Each cycle also publishes [diagnostics](#diagnostics). Scanning continuously,
the board serves a [status page](#status-page) for finding a good spot for it.

The firmware does **not** publish the Home Assistant discovery configs. For Home
Assistant to know the sensors, run at least one instance of the
[`ruuvi2mqtt`](../README.md) app on the same broker. The app publishes the
configs, with the names, units and entity types, and keeps them retained.

---

## Supported chips

The default target is **ESP32-C6**. The `CHIP` Makefile variable selects another
chip:

| Chip     | Architecture | `CHIP` value        |
| -------- | ------------ | ------------------- |
| ESP32    | Xtensa       | `esp32`             |
| ESP32-C3 | RISC-V       | `esp32c3`           |
| ESP32-C5 | RISC-V       | `esp32c5`           |
| ESP32-C6 | RISC-V       | `esp32c6` (default) |
| ESP32-S3 | Xtensa       | `esp32s3`           |

Each chip builds in its own target directory, `target/<chip>/`, unless
`CARGO_TARGET_DIR` is set.

---

## Setup

Install the toolchains and tools:

```sh
make setup
```

This installs the pinned versions of `ldproxy`, `espflash`, `espup`, and the
Xtensa Rust toolchain. Renovate bumps the pins in the `Makefile`, so re-run it
after pulling.

---

## Configuration

Environment variables set the configuration at compile time.

### Required

| Variable      | App YAML equivalent | Description                                          |
| ------------- | ------------------- | ---------------------------------------------------- |
| `WIFI_SSID`   | —                   | Wi-Fi SSID                                           |
| `WIFI_PASS`   | —                   | Wi-Fi password: 8 to 63 characters, or 64 hex digits |
| `MQTT_SERVER` | `mqtt.server`       | MQTT broker hostname or IP                           |

### Optional

| Variable             | App YAML equivalent | Default              | Description                                                                                                                                                                                                                     |
| -------------------- | ------------------- | -------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `WIFI_OPEN`          | —                   | `false`              | Set to `true` for an open network, without `WIFI_PASS`. The board then joins any access point that has the SSID                                                                                                                 |
| `WIFI_IP`            | —                   | _(DHCP)_             | Static IP address (e.g. `192.168.1.50`)                                                                                                                                                                                         |
| `WIFI_GATEWAY`       | —                   | —                    | Gateway (required when `WIFI_IP` is set)                                                                                                                                                                                        |
| `WIFI_NETMASK`       | —                   | `255.255.255.0`      | Subnet mask for static IP                                                                                                                                                                                                       |
| `WIFI_DNS`           | —                   | _gateway_            | DNS server for static IP                                                                                                                                                                                                        |
| `WIFI_BAND`          | —                   | _(both)_             | Dual-band chips only (ESP32-C5): `2.4` or `5` GHz. On the C5, a full scan of 2.4 GHz takes about 2.5 s, of both bands about 13 s                                                                                                |
| `DEVICE_HOSTNAME`    | —                   | `ruuvi2mqtt-esp32`   | Hostname for DHCP and mDNS, and the default MQTT client ID                                                                                                                                                                      |
| `MQTT_PORT`          | `mqtt.port`         | `1883` / `8883`      | MQTT broker port (default depends on TLS)                                                                                                                                                                                       |
| `MQTT_TLS`           | `mqtt.tls`          | `false`              | Set to `true` to enable TLS (`mqtts://`)                                                                                                                                                                                        |
| `MQTT_CA_FILE`       | `mqtt.ca_file`      | —                    | Path to a PEM CA cert, embedded at compile time                                                                                                                                                                                 |
| `MQTT_TLS_INSECURE`  | `mqtt.tls_insecure` | `false`              | Set to `true` to skip the certificate's hostname check                                                                                                                                                                          |
| `MQTT_CLIENT_ID`     | `mqtt.client_id`    | _hostname_           | MQTT client ID, also sent as `publisher` in each reading                                                                                                                                                                        |
| `MQTT_USER`          | `mqtt.user`         | _(anonymous)_        | MQTT username                                                                                                                                                                                                                   |
| `MQTT_PASSWORD`      | `mqtt.password`     | _(no password)_      | MQTT password. Requires `MQTT_USER`                                                                                                                                                                                             |
| `MQTT_BASE_TOPIC`    | `mqtt.base_topic`   | `ruuvi2mqtt`         | Topic prefix                                                                                                                                                                                                                    |
| `BLE_SCAN_MODE`      | —                   | `continuous`         | `continuous` scans all the time. `deep-sleep` scans while Wi-Fi connects, then deep sleeps                                                                                                                                      |
| `BLE_SCAN_DURATION`  | —                   | `60` / `5`           | BLE scan length in seconds. Scanning continuously, this is the cycle. In the deep-sleep mode, set it close to the Wi-Fi and MQTT connect time in the boot log, and at least 3                                                   |
| `BLE_SLEEP_DURATION` | —                   | `60`                 | Deep sleep between cycles in seconds. Scanning continuously, how long the board waits awake after a failure before it restarts. After a cycle timeout, it waits a cycle longer and restarts only if no cycle succeeds meanwhile |
| `USB_STAY_AWAKE`     | —                   | `true`               | `false` always deep sleeps. See [Build & flash](#build--flash)                                                                                                                                                                  |
| `ANTENNA`            | —                   | _(unset)_            | Seeed XIAO ESP32-C6 only, and needed there: `internal` or `external` antenna. See [Antenna](#antenna)                                                                                                                           |
| `LED_MODE`           | —                   | `on` with `LED_GPIO` | `on`, `errors` or `off`. Without `LED_GPIO`, the LED is left alone. See [LED](#led)                                                                                                                                             |
| `LED_TYPE`           | —                   | `ws2812`             | `ws2812` or `gpio`                                                                                                                                                                                                              |
| `LED_ACTIVE_LOW`     | —                   | `false`              | `true` for a `gpio` LED that lights when its pin is low                                                                                                                                                                         |
| `LED_GPIO`           | —                   | —                    | LED pin number. Required with `LED_MODE`                                                                                                                                                                                        |
| `LED_COLOR`          | —                   | `000020`             | WS2812 color as hex `RRGGBB`. Low values keep it dim                                                                                                                                                                            |
| `LOG_LEVEL`          | —                   | `info`               | Log level for the firmware's own code. Library code logs at `info` at most                                                                                                                                                      |

These can be set in the environment, in `.envrc` (with direnv), or in per-device
configuration files (see below).

An empty `WIFI_SSID`, `WIFI_PASS`, `MQTT_SERVER`, `MQTT_USER` or `MQTT_PASSWORD`
fails the build, as it is more likely a failed secret lookup than a choice. For
the other variables, empty means the default.

#### LED

With `LED_MODE=on`, the LED breathes dimly every 6 s while the board scans
continuously, to show that it runs. In the deep-sleep mode, the LED lights dimly
while the board is awake.

On a failure, the LED blinks twice every 3 s instead, at half brightness and in
red on a WS2812. The blinks go on while Wi-Fi is down, and until a cycle has
published its readings and scanned without errors. In the deep-sleep mode, a
failed cycle sleeps right away, so the blinks show in the next wake. `errors`
keeps the LED dark except for the blinks, for rooms where a glow would disturb.

#### Known board LED configurations

| Board                                  | `LED_TYPE`                    | `LED_GPIO` |
| -------------------------------------- | ----------------------------- | ---------- |
| Beetle ESP32 C6 Mini (DFRobot DFR1117) | `gpio`                        | `15`       |
| ESP32-C5-DevKitC-1                     | `ws2812`                      | `27`       |
| ESP32-C6-DevKitC-1                     | `ws2812`                      | `8`        |
| ESP32-CAM (AI-Thinker)                 | `gpio`, `LED_ACTIVE_LOW=true` | `33`       |
| ESP32-S3 AI Camera (DFRobot DFR1154)   | `gpio`                        | `3`        |
| Seeed XIAO ESP32-C6                    | `gpio`, `LED_ACTIVE_LOW=true` | `15`       |

#### Antenna

The Seeed XIAO ESP32-C6 has an RF switch between its ceramic antenna and a U.FL
connector ([Seeed wiki][xiao-c6]). `ANTENNA` selects one. The switch is powered
off in deep sleep, since it draws more than the chip does.

Without `ANTENNA`, the firmware doesn't touch the switch. It then gets no power,
and the XIAO hears little on Wi-Fi and BLE, so set `ANTENNA` on every XIAO.
Other boards use those pins for other things, so the build fails if `ANTENNA` is
set for any chip but the ESP32-C6.

[xiao-c6]: https://wiki.seeedstudio.com/xiao_esp32c6_getting_started/

---

## Device configuration

For a single board, set the required variables in the environment and run
`make flash`. No registration is needed, as auto-detection runs only when
`devices/devices.conf` exists. For a chip other than the default:

```sh
make flash CHIP=esp32
```

### Managing multiple devices

With several boards, each one gets its own configuration file, and the Makefile
finds the attached board by its MAC address.

**Register a device** by attaching it and running:

```sh
make register DEVICE=kitchen
```

This records the board's MAC address in `devices/devices.conf` and creates a
`devices/kitchen.mk` file from the template. Edit the `.mk` file to set
device-specific overrides (static IP, hostname, chip type, etc.). With several
boards attached, choose one with `ESPFLASH_PORT=<port>`.

**List the attached boards** with `make boards`:

```
PORT                     USB    MAC               DEVICE
/dev/cu.usbmodem1101     native aa:bb:cc:dd:ee:f0 kitchen
/dev/cu.usbserial-210    uart   aa:bb:cc:dd:ee:f1 garage
```

`native` is the chip's own USB port, `uart` a USB-UART bridge. A board on its
own port reports its MAC as the USB serial number. For a board behind a bridge,
`make register` records the bridge's serial number. Registered boards are found
without touching them. An unknown board behind a bridge is identified with
`espflash board-info`, which resets it. That happens in `make boards` and in
auto-detection, and when looking for a board registered without the serial
number. Run `make register` again to record the serial number.

**Auto-detection:** when you run `make flash` (or `all`, `build`, `lint`,
`monitor`, `log`) without `DEVICE`, the Makefile identifies the attached board
and loads its `.mk` file. With several boards attached, it lists them and asks
for `DEVICE`. A target that talks to the board finds its port from `DEVICE`, and
never uses another board.

```sh
make flash DEVICE=kitchen                 # flash the board named kitchen
make flash DEVICE=                        # skip auto-detection, use env vars only
make flash ESPFLASH_PORT=/dev/cu.usbserial-210   # the board on this port
```

The Makefile waits up to `WAIT` seconds (default 75) for a sleeping board to
wake.

> Device config files are git-ignored. Only the examples are committed.

---

## Build & flash

```sh
make flash               # build and flash (the first build compiles ESP-IDF, so it is slow)
make flash monitor       # build, flash, and open the serial monitor
make build               # build only
make monitor             # open the interactive serial monitor
make log SECS=70         # print the serial output for 70 s, without prompts
```

All but `make monitor` run without prompts, so scripts and agents can use them.

In the deep-sleep mode, deep sleep turns off the USB port built into the
ESP32-C3, -C5, -C6, and -S3. While a computer is connected to that port, the
board waits awake between cycles and restarts instead, so it can be flashed and
monitored at any time. A charger or power bank doesn't count. A port behind a
USB-UART bridge, as on ESP32 boards and the DevKitC's UART port, stays up in
deep sleep. `USB_STAY_AWAKE=false` deep sleeps anyway, for example to measure
the sleep current.

All available targets:

```sh
make help
```

---

## Status page

Scanning continuously, the board serves a page on
`http://{DEVICE_HOSTNAME}.local/` (mDNS), or on its IP address. The page shows
whether the board's spot is good. Put the board there and open the page on a
phone. It updates every 2 s, and the top says when the board hasn't answered for
6 s.

The top says _Good_, _Fair_ or _Poor spot_ and lists what isn't good:

- **Wi-Fi:** the access point's RSSI. Good from −67 dBm, fair to −75 dBm, poor
  below that. A drop since boot makes it fair at best.
- **Publishing:** fair until the first cycle, poor while the last cycle failed,
  and fair while any cycle since boot did.
- **The target tags:** tap a tag to mark it as one the spot should cover. The
  phone remembers them. A tag is graded by its mean RSSI over the last minute
  (good from −80 dBm, fair to −90 dBm), and by its reports per minute (good from
  10, fair from 3). One not heard is poor. For the first 20 s after boot, the
  rates are left out, and a tag not heard yet is fair.

Without targets, only Wi-Fi and publishing count. Each tag shows its mean RSSI,
the minute's range and its reports per minute. The names come from the Home
Assistant discovery configs that the app keeps retained on the broker. A tag
without a config shows its MAC.

`/api/status` returns the same data as JSON, for scripts. `?tags=<mac>,<mac>`
sets the targets, with the MACs written as in the topics.

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
      "mac": "aabbccddee01",
      "name": "Bedroom",
      "target": true,
      "reports_per_min": 38,
      "rssi": -86,
      "rssi_min": -93,
      "rssi_max": -81,
      "last_heard_s": 1,
      "grade": "fair"
    }
  ]
}
```

---

## Diagnostics

Each cycle ends with a message on
`{MQTT_BASE_TOPIC}/diagnostics/{DEVICE_HOSTNAME}`. It shows what the board heard
and when, so a missed tag can be traced to range or to the radio.

```json
{
  "firmware": "ruuvi2mqtt-esp32 1.5.2-dev (c43f162)",
  "client_id": "ruuvi2mqtt-kitchen",
  "chip": "esp32c6",
  "cycle": 42,
  "settings": {
    "scan_mode": "deep-sleep",
    "scan_duration": 5,
    "antenna": "external"
  },
  "wifi_rssi": -61,
  "wifi_channel": 6,
  "wifi_bssid": "021122334455",
  "wifi_cache": "hit",
  "free_heap": 151000,
  "scan_start_ms": 180,
  "scan_end_ms": 5190,
  "wifi_ms": 1450,
  "mqtt_ms": 2210,
  "cycle_ms": 5230,
  "tags": {
    "aabbccddee01": { "rssi": -74, "adverts": 2, "first_ms": 1210 }
  },
  "adverts_before_wifi": 1,
  "adverts_after_wifi": 1,
  "other_adverts_before_wifi": 35,
  "other_adverts_after_wifi": 61
}
```

| Field                                                   | Description                                                                                                                                                                                           |
| ------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `firmware`                                              | Package version and git commit, with `-dirty` for local changes                                                                                                                                       |
| `client_id`                                             | `MQTT_CLIENT_ID`, the `publisher` in the readings                                                                                                                                                     |
| `chip`                                                  | ESP-IDF target                                                                                                                                                                                        |
| `cycle`                                                 | Cycles since a cold boot or a crash, this one included. A gap means cycles without diagnostics                                                                                                        |
| `settings`                                              | The build-time settings that A/B tests compare: `BLE_SCAN_MODE`, `BLE_SCAN_DURATION` and `ANTENNA` (left out when unset)                                                                              |
| `wifi_rssi`, `wifi_channel`, `wifi_bssid`               | The access point's signal (dBm), channel and BSSID, which roaming changes                                                                                                                             |
| `wifi_cache`                                            | `hit`: connected with the cached channel and BSSID. `miss`: nothing cached. `stale`: the cached values failed, so the board scanned. When scanning continuously, from the first connection after boot |
| `free_heap`                                             | Free heap in bytes                                                                                                                                                                                    |
| `scan_start_ms`, `scan_end_ms`                          | When the BLE scan started and finished                                                                                                                                                                |
| `wifi_ms`, `mqtt_ms`                                    | When Wi-Fi and the broker connection were up. When scanning continuously, `0` if they were up all along                                                                                               |
| `cycle_ms`                                              | When the message was built                                                                                                                                                                            |
| `tags`                                                  | Per Ruuvi MAC: the strongest RSSI (dBm), the advertisements received, and when the first one arrived                                                                                                  |
| `adverts_before_wifi`, `adverts_after_wifi`             | Ruuvi advertisements before and after Wi-Fi connected. When scanning continuously, while Wi-Fi was down and up                                                                                        |
| `other_adverts_before_wifi`, `other_adverts_after_wifi` | Other BLE advertisements, counted the same way. They show how much the scan heard                                                                                                                     |
| `error`                                                 | Only when the BLE scan failed                                                                                                                                                                         |

All times are in milliseconds from the start of the cycle. The test bed's
[checker](../checker/README.md) turns the counts into rates over many cycles.

---

## Security

The built firmware embeds `WIFI_PASS` and `MQTT_PASSWORD` as plaintext constants
in flash. Don't share or publish built images, and note that physical access to
a board is enough to read the credentials back with `espflash`/`esptool`.

ESP-IDF [flash encryption][flash-enc] prevents this. Enable it with
`CONFIG_SECURE_FLASH_ENC_ENABLED` in `sdkconfig.defaults`. It is off because the
first boot with it burns eFuses irreversibly and changes how the board is
flashed. Read the Espressif documentation before enabling it.

The status page has no login. It only reads, and shows nothing secret, but
anyone on the network can open it. The board takes an IPv6 address from the
router too, so keep the firewall blocking incoming connections, as it does by
default.

[flash-enc]:
  https://docs.espressif.com/projects/esp-idf/en/stable/esp32/security/flash-encryption.html
