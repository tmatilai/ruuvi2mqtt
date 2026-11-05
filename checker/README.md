# ruuvi2mqtt-checker

Checks the ruuvi2mqtt app and the ESP32 firmware on a test bed. The test bed
deploys a commit to a few app instances and ESP32 boards near real RuuviTags.
They publish to a broker of their own. The checker lives in this repo because it
must change when the payloads change.

The checker subscribes to `#` on that broker for `--duration` (1 hour), then
checks that:

- Each instance (`--instance <client_id>`) was online at the start and stayed
  online.
- Each instance runs `--commit`, without local changes.
- Each instance has its Home Assistant discovery config.
- Each instance publishes its diagnostics, and never goes `--max-gap` without
  them. The report shows how each instance hears each tag.
- Each tag (`--tag <MAC>`) and Ruuvi Air (`--air <MAC>`) has its first reading
  within `--first-reading` (5 minutes), and never goes `--max-gap` (10 minutes)
  without one.
- A device given as `<MAC>/<GAP>` gets twice its `GAP` for both, if that is
  longer. The `GAP` is its longest gap before the watch, from the recorder, so a
  device that comes rarely doesn't fail.
- The readings have values in the sensors' ranges. A tag's readings have a
  battery and no air quality values. An Air's have a calibration flag and no
  battery or TX power. Devices not given are checked as tags.
- Each reading's `publisher` is one of the instances or one of the boards.
- At most `--max-duplicates` (10 %) of a device's readings are duplicates, which
  come less than 90 % of `--throttle` after the previous one. Two instances with
  a similar signal sometimes both publish. The boards don't throttle, so their
  readings don't count.
- Each tag has all its Home Assistant discovery configs, and none of another
  model's.
- The instances' and the ESP32 boards' diagnostics are valid.
- Each board (`--board <client_id>`) runs `--commit`, without local changes, in
  every cycle. Flash the boards before the watch starts.
- Each board that scans continuously was online at the start and stayed online.
- Each board has its Home Assistant discovery config.
- Each board publishes its diagnostics about once per `--board-cycle` (60 s, the
  firmware's default; 67 s for the deep-sleep defaults), within 10 %, and never
  goes `--max-gap` without them.

The report also counts each publisher's readings of an Air that have PM1, PM4
and PM10, which only data format E1 sends. The count doesn't fail the check: an
ESP32, and an instance without a Bluetooth 5 adapter, get only format 6.

## ESP32 report

The checker reports every board that sends diagnostics, with or without
`--board`. A board is known by the `client_id` in its diagnostics, the
`publisher` of its readings. The report shows each board's settings, so that the
variants of an A/B test can be compared, and figures from its
[diagnostics](../ruuvi2mqtt-esp32/README.md#diagnostics). The hit rate is the
share of the expected tags that the board heard in a cycle, over all cycles.

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

The password is the first line of stdin. `--user` needs `--ca-file`, so that the
password isn't sent in plain text.

The report goes to stdout. Its last line is the summary: `ok: ...` or
`FAIL: ...`. Progress and problems go to stderr as they happen. Text from the
broker, such as a status or a tag's name, has its control characters escaped, so
that a message can't add lines to the report.

The exit status is 0 if all checks pass, 1 if one fails, and 2 on an error, such
as no connection to the broker.

## Recorder

`ruuvi2mqtt-checker record` subscribes to `#` all the time. It records how often
each tag comes, so that the runs know which tags to expect and each one's `GAP`.

```sh
printf '%s\n' "$PASSWORD" | target/release/ruuvi2mqtt-checker record \
    --server broker.example --ca-file ca.crt --user checker --state state.json
```

Every minute it writes the state file, through a temporary file. Per device, by
the topic's ID:

- `name`: from the valid discovery configs.
- `last`: the last reading's time.
- `max_gap`: the longest time in seconds without a reading in the last hour,
  until now.
- `publishers`: the number of readings in the last hour, by publisher.
- `battery` and `battery_low`: from the last reading.
- `readings`: the last hour's readings, `[<time>, <publisher>]`, and at least
  the last one.

A device enters the file with its first reading, named from the discovery
configs that may have come long before, and stays for a day after its last
reading. The times are Unix seconds. `written` is when it wrote the file,
`since` when the recording started, and `window` the hour in seconds. The gaps
count only from `since`.

After a restart, it continues from the file. A break longer than 2 minutes, with
the recorder stopped or disconnected, moves `since` to the reconnect, so that
the break doesn't count as a gap.

Its client ID is `ruuvi2mqtt-recorder`, so that a checker on the same broker
doesn't disconnect it. It exits only on an error, with status 1.

## Site table

`ruuvi2mqtt-checker site` shows how well each instance and board hears each tag,
on any broker. It listens for `--duration` (10 minutes), then prints the table,
or fails if no publisher sent diagnostics.

```sh
printf '%s\n' "$PASSWORD" | target/release/ruuvi2mqtt-checker site \
    --server broker.example --ca-file ca.crt --user viewer
```

```text
Over 10:00, the diagnostics of 2 instances and 1 board.

Problems:
  ! Attic: no publisher hears it well
  ! c6-board: restarted once

           host1     host2  c6-board
        instance  instance     board
Attic   !-93 40%         -         -
Cellar  -70 100%    ? 100%  ~-81 80%
```

A row is a tag with a discovery config, and a column a publisher that sent
diagnostics. A cell has the tag's RSSI, `?` if the adapter doesn't report it,
and the share of the publisher's minutes (instance) or cycles (board) that heard
the tag. The RSSI is the mean of the strongest per minute or cycle, so it reads
stronger than on a board's status page, though graded with the same thresholds.
The share is good from 90 % and fair from 50 %. `~` marks a fair cell, `!` a
poor one.

The problems come first, worst first:

- a tag that no publisher hears, or none better than poor;
- a tag that only instances hear well, when there are boards: a spot for one;
- a tag that only one publisher hears well;
- a board that restarted, on a fair or poor Wi-Fi signal, or with other firmware
  than most boards.

An offline publisher and a low battery are left to Home Assistant's alerts.
