# ruuvi2mqtt-checker

Checks the Linux app and the ESP32 firmware on a test bed. The test bed
deploys a commit to a few instances and boards near real RuuviTags. They
publish to a broker of their own.

The checker subscribes to `#` on that broker for a while (`--duration`,
default 1 hour). Then it checks that:

- Each instance (`--instance <client_id>`) was online at the start and
  stayed online.
- Each instance runs `--commit`, without local changes.
- Each tag (`--tag <MAC>`) and Ruuvi Air (`--air <MAC>`) has its first
  reading within `--first-reading` (5 minutes).
- No tag goes `--max-gap` (10 minutes) without a reading.
- The readings have values in the sensors' ranges. A tag's readings have
  a battery and no air quality values. An Air's have no battery and no TX
  power. Devices not given are checked as tags.
- Each reading's `publisher` is one of the instances or one of the boards.
- Few readings are duplicates (`--max-duplicates`, 10 %). A duplicate comes
  less than 90 % of `--throttle` after the previous reading. Two instances
  with a similar signal sometimes both publish. The boards' readings don't
  count, since the boards don't coordinate.
- Each tag has all its Home Assistant discovery configs, and none of
  another model's.
- The ESP32 diagnostics are valid.
- Each board (`--board <client_id>`) runs `--commit`, without local
  changes, in every cycle. Flash the boards before the watch starts.
- Each board publishes its diagnostics about once per `--board-cycle`
  (67 s, the default sleep plus the awake time), within 10 %, and never
  goes `--max-gap` without them.

## ESP32 report

The checker reports every board that sends diagnostics, with or without
`--board`. A board is known by the `client_id` in its diagnostics, the
`publisher` of its readings. The report shows each board's settings, so
that the variants of an A/B test can be compared, and figures from its
[diagnostics](../ruuvi2mqtt-esp32/README.md#diagnostics). The hit rate is
the share of cycles in which the board heard each expected tag.

These figures don't fail the check yet, since good values are still
unknown.

It lives in this repo because it must change when the payloads change.

## Usage

```sh
cargo build --release -p ruuvi2mqtt-checker
printf '%s\n' "$PASSWORD" | target/release/ruuvi2mqtt-checker \
    --server broker.example --ca-file ca.crt --user checker \
    --commit "$(git rev-parse HEAD)" \
    --instance ruuvi2mqtt_host1 --instance ruuvi2mqtt_host2 \
    --board ruuvi2mqtt-c6 \
    --tag F8:17:62:4E:4C:0B --tag CB:20:F2:CC:4F:7E --duration 15m
```

The password is the first line of stdin.

The report goes to stdout. Its last line is the summary: `ok: ...` or
`FAIL: ...`. Progress and problems go to stderr as they happen.

The exit status is 0 if all checks pass, 1 if one fails, and 2 on an error,
such as no connection to the broker.
