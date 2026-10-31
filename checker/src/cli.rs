//! The command line: the check by default, or the `record` subcommand.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use clap::{Args, FromArgMatches};

use crate::check::Config;
use crate::time;

pub enum Mode {
    Check(CheckOptions),
    Record(RecordOptions),
}

/// Built by hand: clap's derive can't have the check's options beside the
/// subcommand. It wants their required ones with the subcommand too, and
/// leaves an optional flatten of them unset.
fn command() -> clap::Command {
    let record = clap::Command::new("record").about(
        "Record the readings all the time, so that the runs know which tags to expect. \
         Writes the state file every minute",
    );
    CheckOptions::augment_args(clap::Command::new(env!("CARGO_PKG_NAME")))
        .about(env!("CARGO_PKG_DESCRIPTION"))
        .args_conflicts_with_subcommands(true)
        .subcommand_negates_reqs(true)
        .subcommand(RecordOptions::augment_args(record))
}

pub fn parse() -> Mode {
    try_parse_from(std::env::args_os()).unwrap_or_else(|err| err.exit())
}

fn try_parse_from<I, T>(args: I) -> Result<Mode, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let mut matches = command().try_get_matches_from(args)?;
    let mode = match matches.remove_subcommand() {
        Some((_, mut record)) => RecordOptions::from_arg_matches_mut(&mut record).map(Mode::Record),
        None => CheckOptions::from_arg_matches_mut(&mut matches).map(Mode::Check),
    };
    mode.map_err(|err| err.format(&mut command()))
}

#[derive(Debug, Args)]
pub struct Broker {
    /// MQTT broker
    #[arg(long)]
    pub server: String,
    /// Default: 8883 with TLS, else 1883
    #[arg(long)]
    pub port: Option<u16>,
    /// Connect with TLS, verifying the broker with this CA certificate (PEM)
    #[arg(long, value_name = "FILE")]
    pub ca_file: Option<PathBuf>,
    /// MQTT user. The password is the first line of stdin. Needs TLS, so
    /// that the password isn't sent in plain text
    #[arg(long, requires = "ca_file")]
    pub user: Option<String>,
    #[arg(long, default_value = "ruuvi2mqtt")]
    pub base_topic: String,
}

#[derive(Debug, Args)]
pub struct RecordOptions {
    #[command(flatten)]
    pub broker: Broker,
    /// Not the checker's, or the broker would disconnect one of them
    #[arg(long, default_value = "ruuvi2mqtt-recorder")]
    pub client_id: String,
    /// The state file, JSON. The recorder continues from it after a restart
    #[arg(long, value_name = "FILE")]
    pub state: PathBuf,
}

#[derive(Debug, Args)]
pub struct CheckOptions {
    #[command(flatten)]
    pub broker: Broker,
    #[arg(long, default_value = "ruuvi2mqtt-checker")]
    pub client_id: String,
    /// The commit under test, at least 7 hex digits
    #[arg(long, value_parser = parse_commit)]
    commit: String,
    /// An app instance's MQTT client ID (repeat for each)
    #[arg(long = "instance", value_name = "CLIENT_ID", required = true)]
    instances: Vec<String>,
    /// An ESP32 board's MQTT client ID, to check its firmware and cycle
    /// (repeat for each). Every board that sends diagnostics is reported
    #[arg(long = "board", value_name = "CLIENT_ID")]
    boards: Vec<String>,
    /// How often a board publishes: its scan duration when it scans
    /// continuously, else its deep sleep plus its awake time
    #[arg(long, default_value = "60s", value_parser = time::parse)]
    board_cycle: Duration,
    /// The MAC address of a tag that must be published (repeat for each).
    /// GAP, e.g. 6m or 382s, is its longest gap before the watch. It gets
    /// twice that for its first reading and its gaps, if longer than
    /// --first-reading and --max-gap
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
    pub duration: Duration,
    /// Every tag's first reading must come within this time
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

impl CheckOptions {
    pub fn config(&self) -> Config {
        let devices = || self.tags.iter().chain(&self.airs);
        Config {
            base_topic: self.broker.base_topic.clone(),
            commit: self.commit.clone(),
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
        }
    }
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

    const CHECK: [&str; 11] = [
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
    ];

    fn check(args: &[&str]) -> Result<CheckOptions, clap::Error> {
        match try_parse_from(CHECK.iter().chain(args))? {
            Mode::Check(options) => Ok(options),
            Mode::Record(_) => panic!("a record"),
        }
    }

    #[test]
    fn verify_cli() {
        command().debug_assert();
    }

    #[test]
    fn parses_a_check() {
        let options = check(&[]).unwrap();
        assert_eq!(options.broker.server, "broker");
        assert_eq!(options.client_id, "ruuvi2mqtt-checker");
        let config = options.config();
        assert_eq!(config.commit, "6534f11");
        assert_eq!(config.tags, ["aabbccddee01", "aabbccddee02"]);
        assert_eq!(config.airs, ["aabbccddee02"]);
        assert_eq!(
            config.gaps.into_iter().collect::<Vec<_>>(),
            [("aabbccddee01".to_string(), Duration::from_secs(382))]
        );
        assert!(try_parse_from(["checker", "--server", "broker"]).is_err());
    }

    #[test]
    fn parses_a_record() {
        let Mode::Record(options) = try_parse_from([
            "checker",
            "record",
            "--server",
            "broker",
            "--state",
            "state.json",
        ])
        .unwrap() else {
            panic!("a check");
        };
        assert_eq!(options.broker.server, "broker");
        assert_eq!(options.client_id, "ruuvi2mqtt-recorder");
        assert_eq!(options.state, PathBuf::from("state.json"));
        assert!(try_parse_from(["checker", "record", "--state", "s"]).is_err());
        assert!(
            try_parse_from([
                "checker", "--commit", "6534f11", "record", "--server", "b", "--state", "s"
            ])
            .is_err()
        );
    }

    #[test]
    fn user_needs_tls() {
        assert!(check(&["--user", "checker"]).is_err());
        assert!(check(&["--user", "checker", "--ca-file", "ca.crt"]).is_ok());
    }

    /// The test bed's runner looks for these to know what the checker takes.
    #[test]
    fn help_has_what_the_runner_looks_for() {
        let help = command().render_help().to_string();
        for s in ["MAC[/GAP]", "--air", "\n  record "] {
            assert!(help.contains(s), "{s:?} in:\n{help}");
        }
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
