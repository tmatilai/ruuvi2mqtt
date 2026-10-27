# Ruuvi Air simulator

ESP32 firmware that advertises like a Ruuvi Air, for testing ruuvi2mqtt
without one. It runs the Air's three advertising sets from one static
random address:

| Set      | Format | Advertising | PHY                      | Interval    |
|----------|--------|-------------|--------------------------|-------------|
| legacy   | 6      | legacy      | 1M                       | 211–319 ms  |
| extended | E1     | extended    | 1M primary, 2M secondary | 211–319 ms  |
| coded    | E1     | extended    | Coded                    | 800–1000 ms |

The advertising data matches the Air firmware's `src/ble_adv.c` at
[ruuvi/ruuvi.air.main](https://github.com/ruuvi/ruuvi.air.main) commit
69a3ffb, except that no set is connectable. The values are the firmware's
mock build values, with a temperature and sequence number that change in
each reading. A new reading goes out every second.

## Use

Needs the toolchain of `../ruuvi2mqtt-esp32` (`make -C ../ruuvi2mqtt-esp32
setup`) and a Bluetooth 5 chip: ESP32-C3, C5, C6 or S3.

```sh
make flash log CHIP=esp32c5
```

The board is found as in the firmware: the only attached board is looked up
in the firmware's `devices/devices.conf`, or `DEVICE=<name>` chooses one,
and its `devices/<name>.mk` sets `CHIP` and the LED. `DEVICE=` skips the
registry, and `ESPFLASH_PORT=<port>` chooses the port. `make boards` lists
the boards. The boot log shows the address, for example
`Advertising as C0:FF:EE:00:A1:12 (Ruuvi A112)`. Add it to the ruuvi2mqtt
config with `model: air`.

Compile-time environment variables:

- `AIR_MAC`: the address. It must be static random, so the first byte is
  C0–FF. Default: the chip's Bluetooth MAC with the two top bits set.
- `AIR_SETS`: a comma-separated list of the sets to run. Default:
  `legacy,extended,coded`. A chip without the Coded PHY logs a warning and
  runs the others.
- `LED_MODE`, `LED_TYPE`, `LED_GPIO`, `LED_ACTIVE_LOW` and `LED_COLOR`: as
  in the firmware, except that with `LED_MODE=on` the LED breathes slowly
  and dimly every 6 s, to show that the simulator runs. A plain LED
  (`gpio`) peaks at an eighth of full brightness.

`make test` runs the encoder's tests on the host. They check it against the
specs' test vectors and ruuvi-sensor-protocol's decoder.
