# ruuvi2mqtt-checker

Checks the Linux app on a test bed. The test bed deploys a commit to a few
instances near real RuuviTags. They publish to a broker of their own.

The checker subscribes to `#` on that broker for a while (`--duration`,
default 1 hour). Then it checks that:

- Each instance (`--instance <client_id>`) was online at the start and
  stayed online.
- Each instance runs `--commit`, without local changes.
- Each tag (`--tag <MAC>`) has its first reading within `--first-reading`
  (5 minutes).
- No tag goes `--max-gap` (10 minutes) without a reading.
- The readings have values in the sensors' ranges.
- Each reading's `publisher` is one of the instances.
- Few readings are duplicates (`--max-duplicates`, 10 %). A duplicate comes
  less than 90 % of `--throttle` after the previous reading. Two instances
  with a similar signal sometimes both publish.
- Each tag has all its Home Assistant discovery configs.

It lives in this repo because it must change when the payloads change.

## Usage

```sh
cargo build --release -p ruuvi2mqtt-checker
printf '%s\n' "$PASSWORD" | target/release/ruuvi2mqtt-checker \
    --server broker.example --ca-file ca.crt --user checker \
    --commit "$(git rev-parse HEAD)" \
    --instance ruuvi2mqtt_host1 --instance ruuvi2mqtt_host2 \
    --tag F8:17:62:4E:4C:0B --tag CB:20:F2:CC:4F:7E --duration 15m
```

The password is the first line of stdin.

The report goes to stdout. Its last line is the summary: `ok: ...` or
`FAIL: ...`. Progress and problems go to stderr as they happen.

The exit status is 0 if all checks pass, 1 if one fails, and 2 on an error,
such as no connection to the broker.
