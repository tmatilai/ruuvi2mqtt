# Boards

The settings of the boards the firmware has run on. Copy a board's lines into
its [device file](README.md#device-configuration). The
[README](README.md#configuration) explains each setting.

## Adafruit ESP32-C6 Feather

```make
export LED_TYPE = ws2812
export LED_GPIO = 9
export LED_POWER_GPIO = 20
export BATTERY = max17048
export BATTERY_SDA = 19
export BATTERY_SCL = 18
export BATTERY_POWER_GPIO = 20
```

GPIO 20 powers both the LED and the battery gauge's I2C bus. The LED shares GPIO
9 with the BOOT button.

## Beetle ESP32 C6 Mini (DFRobot DFR1117)

```make
export LED_TYPE = gpio
export LED_GPIO = 15
```

## ESP32-C5-DevKitC-1

```make
CHIP = esp32c5
export LED_TYPE = ws2812
export LED_GPIO = 27
```

## ESP32-C6-DevKitC-1

```make
export LED_TYPE = ws2812
export LED_GPIO = 8
```

## ESP32-CAM (AI-Thinker)

```make
CHIP = esp32
export LED_TYPE = gpio
export LED_ACTIVE_LOW = true
export LED_GPIO = 33
```

## ESP32-S3 AI Camera (DFRobot DFR1154)

```make
CHIP = esp32s3
export LED_TYPE = gpio
export LED_GPIO = 3
```

## ESP32-S3-DevKitC-1

```make
CHIP = esp32s3
export LED_TYPE = ws2812
export LED_GPIO = 38
```

On v1.0 of the board, the LED is on GPIO 48.

## M5Stack NanoC6

```make
export LED_TYPE = ws2812
export LED_GPIO = 20
export LED_POWER_GPIO = 19
```

## M5Stack StickS3

```make
CHIP = esp32s3
export BATTERY = m5pm1
export BATTERY_SDA = 47
export BATTERY_SCL = 48
```

It has no LED that the firmware can use.

## Seeed XIAO ESP32-C6

```make
export ANTENNA = internal
export LED_TYPE = gpio
export LED_ACTIVE_LOW = true
export LED_GPIO = 15
```

`ANTENNA = external` uses the U.FL connector. Without
[`ANTENNA`](README.md#antenna), the board hears little.
