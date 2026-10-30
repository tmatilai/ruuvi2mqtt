# ruuvi2mqtt-checker

Checks the ruuvi2mqtt app and the ESP32 firmware on a test bed. The test bed
deploys a commit to a few instances and boards near real RuuviTags. They publish
to a broker of their own. The checker lives in this repo because it must change
when the payloads change.

The checker subscribes to `#` on that broker for a while (`--duration`, default
1 hour). Then it checks that:

- Each instance (`--instance <client_id>`) was online at the start and stayed
  online.
- Each instance runs `--commit`, without local changes.
- Each tag (`--tag <MAC>`) and Ruuvi Air (`--air <MAC>`) has its first reading
  within `--first-reading` (5 minutes).
- No tag goes `--max-gap` (10 minutes) without a reading.
- A tag given as `<MAC>/<GAP>` gets twice its `GAP` for both, if that is longer.
  The `GAP` is its longest gap before the watch, from the recorder. A tag that
  comes rarely then doesn't fail the check.
- The readings have values in the sensors' ranges. A tag's readings have a
  battery and no air quality values. An Air's have a calibration flag and no
  battery or TX power. Devices not given are checked as tags. The report counts
  an Air's readings from each publisher that have PM1, PM4 and PM10, which only
  data format E1 sends. The count doesn't fail the check: without a Bluetooth 5
  adapter, an instance gets format 6 only.
- Each reading's `publisher` is one of the instances or one of the boards.
- Few readings are duplicates (`--max-duplicates`, 10 %). A duplicate comes less
  than 90 % of `--throttle` after the previous reading. Two instances with a
  similar signal sometimes both publish. The boards' readings don't count, since
  the boards don't coordinate.
- Each tag has all its Home Assistant discovery configs, and none of another
  model's.
- The ESP32 diagnostics are valid.
- Each board (`--board <client_id>`) runs `--commit`, without local changes, in
  every cycle. Flash the boards before the watch starts.
- Each board publishes its diagnostics about once per `--board-cycle` (60 s, the
  firmware's default; 67 s for the deep-sleep defaults), within 10 %, and never
  goes `--max-gap` without them.

## ESP32 report

The checker reports every board that sends diagnostics, with or without
`--board`. A board is known by the `client_id` in its diagnostics, the
`publisher` of its readings. The report shows each board's settings, so that the
variants of an A/B test can be compared, and figures from its
[diagnostics](../ruuvi2mqtt-esp32/README.md#diagnostics). The hit rate is the
share of cycles in which the board heard each expected tag.

These figures don't fail the check yet, since good values are still unknown.

## Usage

```sh
cargo build --release -p ruuvi2mqtt-checker
printf '%s\n' "$PASSWORD" | target/release/ruuvi2mqtt-checker \
    --server broker.example --ca-file ca.crt --user checker \
    --commit "$(git rev-parse HEAD)" \
    --instance ruuvi2mqtt_host1 --instance ruuvi2mqtt_host2 \
    --board ruuvi2mqtt-c6 \
    --tag AA:BB:CC:DD:EE:01 --tag AA:BB:CC:DD:EE:02 --duration 15m
```

The password is the first line of stdin.

The report goes to stdout. Its last line is the summary: `ok: ...` or
`FAIL: ...`. Progress and problems go to stderr as they happen.

The exit status is 0 if all checks pass, 1 if one fails, and 2 on an error, such
as no connection to the broker.

## Recorder

`ruuvi2mqtt-checker record` subscribes to `#` all the time. It is the test bed's
record of how often each tag comes, for the runs to know which tags to expect
and the `GAP` of each.

```sh
printf '%s\n' "$PASSWORD" | target/release/ruuvi2mqtt-checker record \
    --server broker.example --ca-file ca.crt --user checker --state state.json
```

Every minute it writes the state file, through a temporary file. Per device, by
the topic's ID:

- `name`: from the discovery configs.
- `last`: the last reading's time.
- `max_gap`: the longest time in seconds without a reading in the last hour,
  until now.
- `publishers`: the readings in the last hour by publisher.
- `battery` and `battery_low`: from the last reading.
- `readings`: the last hour's readings, `[<time>, <publisher>]`.

The times are Unix seconds. `written` is when it wrote the file, and `since` is
when the recording started. The gaps count only from `since`. After a restart,
it continues from the file. A break longer than 2 minutes, of the recorder or
its connection, starts the recording over.

Its client ID is `ruuvi2mqtt-recorder`, so that a checker on the same broker
doesn't disconnect it. It returns only on an error.
