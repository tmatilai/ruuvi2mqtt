//! Checks the ruuvi2mqtt app and the ESP32 boards on a test bed. See README.md.

mod check;
mod record;
mod time;

use std::collections::BTreeMap;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use rumqttc::{AsyncClient, Event, Incoming, MqttOptions, QoS, TlsConfiguration, Transport};
use tokio::time::{Instant, sleep, sleep_until};

use crate::check::{Config, Report, Watch};
use crate::record::Recorder;

/// How often to print progress to stderr
const PROGRESS_INTERVAL: Duration = Duration::from_mins(5);
/// How often the recorder writes its state
const WRITE_INTERVAL: Duration = Duration::from_mins(1);

#[derive(Debug, Parser)]
#[command(
    about,
    long_about = None,
    args_conflicts_with_subcommands = true,
    subcommand_negates_reqs = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    /// Empty with a command. Not an optional flatten: clap's derive leaves
    /// that None even with the options given.
    #[command(flatten)]
    options: Options,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Record the readings all the time, for the runs to know which tags to
    /// expect. Writes the state every minute
    Record(RecordOptions),
}

#[derive(Debug, Args)]
struct Broker {
    /// MQTT broker. Required, but optional for clap's derive, which would
    /// otherwise want the check's with a command
    #[arg(long, required = true)]
    server: Option<String>,
    /// Default: 8883 with TLS, else 1883
    #[arg(long)]
    port: Option<u16>,
    /// Connect with TLS, verifying the broker with this CA certificate (PEM)
    #[arg(long, value_name = "FILE")]
    ca_file: Option<PathBuf>,
    /// MQTT user. The password is the first line of stdin
    #[arg(long)]
    user: Option<String>,
    #[arg(long, default_value = "ruuvi2mqtt")]
    base_topic: String,
}

#[derive(Debug, Args)]
struct RecordOptions {
    #[command(flatten)]
    broker: Broker,
    /// Not the checker's: the broker would disconnect one of them
    #[arg(long, default_value = "ruuvi2mqtt-recorder")]
    client_id: String,
    /// The state file, JSON. The recorder continues from it after a restart
    #[arg(long, value_name = "FILE")]
    state: PathBuf,
}

#[derive(Debug, Args)]
struct Options {
    #[command(flatten)]
    broker: Broker,
    #[arg(long, default_value = "ruuvi2mqtt-checker")]
    client_id: String,
    /// The commit under test, at least 7 hex digits. Required, as `server`
    #[arg(long, required = true, value_parser = parse_commit)]
    commit: Option<String>,
    /// An app instance's MQTT client ID (repeat for each)
    #[arg(long = "instance", value_name = "CLIENT_ID", required = true)]
    instances: Vec<String>,
    /// An ESP32 board's MQTT client ID (repeat for each). Its firmware and
    /// cycle are checked. Boards that send diagnostics are reported without
    /// it
    #[arg(long = "board", value_name = "CLIENT_ID")]
    boards: Vec<String>,
    /// How often a board publishes: its scan when it scans continuously,
    /// otherwise its deep sleep plus its awake time
    #[arg(long, default_value = "60s", value_parser = time::parse)]
    board_cycle: Duration,
    /// The MAC address of a tag that must be published (repeat for each).
    /// GAP is its longest gap before the watch, e.g. 6m or 382s: it gets
    /// twice that for its first reading and its gaps, if that is longer
    #[arg(
        long = "tag",
        value_name = "MAC[/GAP]",
        required_unless_present = "airs",
        value_parser = parse_device
    )]
    tags: Vec<(String, Option<Duration>)>,
    /// The MAC address of a Ruuvi Air that must be published (repeat for
    /// each), with its GAP as for a tag
    #[arg(long = "air", value_name = "MAC[/GAP]", value_parser = parse_device)]
    airs: Vec<(String, Option<Duration>)>,
    /// How long to watch, e.g. 1h, 15m or 90s
    #[arg(long, default_value = "1h", value_parser = time::parse)]
    duration: Duration,
    /// Every tag must be published within this time
    #[arg(long, default_value = "5m", value_parser = time::parse)]
    first_reading: Duration,
    /// The longest a tag may go without a reading
    #[arg(long, default_value = "10m", value_parser = time::parse)]
    max_gap: Duration,
    /// The instances' mqtt.throttle. A reading less than 90 % of it after the
    /// previous one is a duplicate
    #[arg(long, default_value = "60s", value_parser = time::parse)]
    throttle: Duration,
    /// Percentage of a tag's readings that may be duplicates. Two instances
    /// with a similar signal sometimes both publish
    #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u8).range(0..=100))]
    max_duplicates: u8,
}

impl Options {
    fn config(&self) -> Result<Config> {
        let devices = || self.tags.iter().chain(&self.airs);
        Ok(Config {
            base_topic: self.broker.base_topic(),
            commit: self.commit.clone().context("No --commit")?,
            instances: self.instances.clone(),
            boards: self.boards.clone(),
            board_cycle: self.board_cycle,
            tags: devices().map(|(id, _)| id.clone()).collect(),
            airs: self.airs.iter().map(|(id, _)| id.clone()).collect(),
            gaps: devices()
                .filter_map(|(id, gap)| Some((id.clone(), (*gap)?)))
                .collect::<BTreeMap<_, _>>(),
            first_reading: self.first_reading,
            max_gap: self.max_gap,
            throttle: self.throttle,
            max_duplicates: self.max_duplicates,
        })
    }
}

impl Broker {
    fn base_topic(&self) -> String {
        self.base_topic.trim_end_matches('/').to_string()
    }

    fn server(&self) -> Result<&str> {
        self.server.as_deref().context("No --server")
    }

    fn mqtt_options(&self, client_id: &str) -> Result<MqttOptions> {
        let port = self
            .port
            .unwrap_or(if self.ca_file.is_some() { 8883 } else { 1883 });
        let mut options = MqttOptions::new(client_id, self.server()?, port);
        options.set_keep_alive(Duration::from_secs(15));
        if let Some(user) = &self.user {
            options.set_credentials(user, read_password()?);
        }
        if let Some(ca_file) = &self.ca_file {
            let ca = std::fs::read(ca_file)
                .with_context(|| format!("Failed to read {}", ca_file.display()))?;
            options.set_transport(Transport::Tls(TlsConfiguration::Simple {
                ca,
                alpn: None,
                client_auth: None,
            }));
        }
        Ok(options)
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    if let Some(Command::Record(options)) = &cli.command {
        // It returns only on an error: systemd restarts it
        let Err(err) = record(options).await;
        eprintln!("{err:#}");
        return ExitCode::FAILURE;
    }
    match run(&cli.options).await {
        Ok(report) => {
            print!("{}", report.text);
            println!("{}", report.summary);
            if report.failures.is_empty() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(err) => {
            println!("FAIL: {err:#}");
            ExitCode::from(2)
        }
    }
}

async fn run(options: &Options) -> Result<Report> {
    let (client, mut eventloop) =
        AsyncClient::new(options.broker.mqtt_options(&options.client_id)?, 10);
    let mut watch = Watch::new(options.config()?);
    let start = Instant::now();
    let deadline = start + options.duration;
    let mut next_progress = start + PROGRESS_INTERVAL;
    let mut connected = false;

    loop {
        let event = tokio::select! {
            () = sleep_until(deadline) => break,
            event = eventloop.poll() => event,
        };
        let at = start.elapsed();
        match event {
            Ok(Event::Incoming(Incoming::ConnAck(_))) => {
                if connected {
                    eprintln!("+{} reconnected to the broker", time::format(at));
                }
                connected = true;
                client
                    .try_subscribe("#", QoS::AtMostOnce)
                    .context("Failed to subscribe")?;
            }
            Ok(Event::Incoming(Incoming::Publish(msg))) => {
                if let Some(note) = watch.on_message(at, &msg.topic, &msg.payload, msg.retain) {
                    eprintln!("{note}");
                }
            }
            Ok(_) => {}
            // Without a first connection, the instances can't be checked either
            Err(err) if !connected => {
                bail!("Failed to connect to {}: {err}", options.broker.server()?);
            }
            Err(err) => {
                eprintln!("+{} MQTT: {err}", time::format(at));
                // The next poll reconnects
                sleep(Duration::from_secs(5)).await;
            }
        }
        if Instant::now() >= next_progress {
            eprintln!("+{} {}", time::format(at), watch.progress());
            next_progress += PROGRESS_INTERVAL;
        }
    }
    Ok(watch.finish(start.elapsed()))
}

/// The record mode: never returns, except on an error.
async fn record(options: &RecordOptions) -> Result<std::convert::Infallible> {
    let base_topic = options.broker.base_topic();
    // Until it connects, it records nothing
    let (mut recorder, mut break_from) = match std::fs::read_to_string(&options.state) {
        Ok(state) => match Recorder::load(&base_topic, &state) {
            Ok(loaded) => loaded,
            Err(err) => {
                eprintln!("starting over: {}: {err}", options.state.display());
                (Recorder::new(&base_topic, unix_now()), unix_now())
            }
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            (Recorder::new(&base_topic, unix_now()), unix_now())
        }
        Err(err) => return Err(err).context(format!("Failed to read {}", options.state.display())),
    };
    let (client, mut eventloop) =
        AsyncClient::new(options.broker.mqtt_options(&options.client_id)?, 10);
    let mut connected = false;
    let mut next_write = Instant::now() + WRITE_INTERVAL;
    loop {
        let event = tokio::select! {
            () = sleep_until(next_write) => {
                write_state(&options.state, &recorder.state(unix_now()))?;
                next_write += WRITE_INTERVAL;
                continue;
            }
            event = eventloop.poll() => event,
        };
        match event {
            Ok(Event::Incoming(Incoming::ConnAck(_))) => {
                eprintln!("connected to {}", options.broker.server()?);
                recorder.resume(break_from, unix_now());
                connected = true;
                client
                    .try_subscribe("#", QoS::AtMostOnce)
                    .context("Failed to subscribe")?;
            }
            Ok(Event::Incoming(Incoming::Publish(msg))) => {
                recorder.on_message(unix_now(), &msg.topic, &msg.payload, msg.retain);
            }
            Ok(_) => {}
            Err(err) => {
                eprintln!("MQTT: {err}");
                if connected {
                    connected = false;
                    break_from = unix_now();
                }
                // The next poll reconnects
                sleep(Duration::from_secs(5)).await;
            }
        }
    }
}

/// Writes the state through a temporary file, so that a reader never gets a
/// partial one.
fn write_state(path: &Path, state: &serde_json::Value) -> Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, state.to_string())
        .and_then(|()| std::fs::rename(&tmp, path))
        .with_context(|| format!("Failed to write {}", path.display()))
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn read_password() -> Result<String> {
    let mut password = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut password)
        .context("Failed to read the password from stdin")?;
    let password = password.trim_end_matches(['\r', '\n']);
    if password.is_empty() {
        bail!("No password on stdin");
    }
    Ok(password.to_string())
}

fn parse_commit(s: &str) -> Result<String, String> {
    if s.len() >= 7 && s.len() <= 40 && s.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(s.to_ascii_lowercase())
    } else {
        Err(format!("not a commit hash: '{s}'"))
    }
}

/// `<MAC>[/<GAP>]`
fn parse_device(s: &str) -> Result<(String, Option<Duration>), String> {
    match s.split_once('/') {
        Some((mac, gap)) => Ok((parse_mac(mac)?, Some(time::parse(gap)?))),
        None => Ok((parse_mac(s)?, None)),
    }
}

/// `AA:BB:CC:DD:EE:01` to `aabbccddee01`, as in the topics
fn parse_mac(s: &str) -> Result<String, String> {
    let hex: String = s.chars().filter(|c| *c != ':').collect();
    if hex.len() == 12 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(hex.to_ascii_lowercase())
    } else {
        Err(format!("not a MAC address: '{s}'"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_cli() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }

    #[test]
    fn parses_devices() {
        assert_eq!(
            parse_device("AA:BB:CC:DD:EE:01"),
            Ok(("aabbccddee01".into(), None))
        );
        assert_eq!(
            parse_device("AA:BB:CC:DD:EE:01/382s"),
            Ok(("aabbccddee01".into(), Some(Duration::from_secs(382))))
        );
        assert!(parse_device("AA:BB:CC:DD:EE:01/").is_err());
        assert!(parse_device("AA:BB:CC:DD:EE/1m").is_err());
    }

    #[test]
    fn parses_a_check() {
        let cli = Cli::try_parse_from([
            "checker",
            "--server",
            "broker",
            "--commit",
            "6534f11",
            "--instance",
            "host1",
            "--tag",
            "AA:BB:CC:DD:EE:01/382s",
            "--air",
            "AA:BB:CC:DD:EE:02",
        ])
        .unwrap();
        assert!(cli.command.is_none());
        assert_eq!(cli.options.broker.server().unwrap(), "broker");
        let config = cli.options.config().unwrap();
        assert_eq!(config.commit, "6534f11");
        assert_eq!(config.tags, ["aabbccddee01", "aabbccddee02"]);
        assert_eq!(config.airs, ["aabbccddee02"]);
        assert_eq!(
            config.gaps.into_iter().collect::<Vec<_>>(),
            [("aabbccddee01".to_string(), Duration::from_secs(382))]
        );
    }

    #[test]
    fn record_takes_no_check_options() {
        let cli = Cli::try_parse_from([
            "checker",
            "record",
            "--server",
            "broker",
            "--state",
            "state.json",
        ])
        .unwrap();
        assert!(matches!(cli.command, Some(Command::Record(_))));
        assert_eq!(cli.options.commit, None);
        assert!(
            Cli::try_parse_from([
                "checker", "--commit", "6534f11", "record", "--server", "b", "--state", "s"
            ])
            .is_err()
        );
        assert!(Cli::try_parse_from(["checker", "--server", "broker"]).is_err());
    }

    #[test]
    fn parses_macs() {
        assert_eq!(
            parse_mac("AA:BB:CC:DD:EE:01").as_deref(),
            Ok("aabbccddee01")
        );
        assert_eq!(parse_mac("aabbccddee01").as_deref(), Ok("aabbccddee01"));
        assert!(parse_mac("AA:BB:CC:DD:EE").is_err());
        assert!(parse_mac("GA:BB:CC:DD:EE:01").is_err());
    }

    #[test]
    fn parses_commits() {
        assert_eq!(parse_commit("6534F11").as_deref(), Ok("6534f11"));
        assert!(parse_commit("6534f1").is_err());
        assert!(parse_commit("main").is_err());
    }
}
