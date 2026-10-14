//! Checks the Linux app and the ESP32 boards on a test bed. See README.md.

mod check;
mod time;

use std::io::BufRead;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::Parser;
use rumqttc::{AsyncClient, Event, Incoming, MqttOptions, QoS, TlsConfiguration, Transport};
use tokio::time::{Instant, sleep, sleep_until};

use crate::check::{Config, Report, Watch};

/// How often to print progress to stderr
const PROGRESS_INTERVAL: Duration = Duration::from_mins(5);

#[derive(Debug, Parser)]
#[command(about, long_about = None)]
struct Options {
    /// MQTT broker
    #[arg(long)]
    server: String,
    /// Default: 8883 with TLS, else 1883
    #[arg(long)]
    port: Option<u16>,
    /// Connect with TLS, verifying the broker with this CA certificate (PEM)
    #[arg(long, value_name = "FILE")]
    ca_file: Option<PathBuf>,
    /// MQTT user. The password is the first line of stdin
    #[arg(long)]
    user: Option<String>,
    #[arg(long, default_value = "ruuvi2mqtt-checker")]
    client_id: String,
    #[arg(long, default_value = "ruuvi2mqtt")]
    base_topic: String,
    /// The commit under test, at least 7 hex digits
    #[arg(long, value_parser = parse_commit)]
    commit: String,
    /// A Linux instance's MQTT client ID (repeat for each)
    #[arg(long = "instance", value_name = "CLIENT_ID", required = true)]
    instances: Vec<String>,
    /// An ESP32 board's MQTT client ID (repeat for each). Its firmware and
    /// cycle are checked. Boards that send diagnostics are reported without
    /// it
    #[arg(long = "board", value_name = "CLIENT_ID")]
    boards: Vec<String>,
    /// How often a board publishes: its deep sleep plus its awake time
    #[arg(long, default_value = "67s", value_parser = time::parse)]
    board_cycle: Duration,
    /// The MAC address of a tag that must be published (repeat for each)
    #[arg(long = "tag", value_name = "MAC", required = true, value_parser = parse_mac)]
    tags: Vec<String>,
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
    fn config(&self) -> Config {
        Config {
            base_topic: self.base_topic.trim_end_matches('/').to_string(),
            commit: self.commit.clone(),
            instances: self.instances.clone(),
            boards: self.boards.clone(),
            board_cycle: self.board_cycle,
            tags: self.tags.clone(),
            first_reading: self.first_reading,
            max_gap: self.max_gap,
            throttle: self.throttle,
            max_duplicates: self.max_duplicates,
        }
    }

    fn mqtt_options(&self) -> Result<MqttOptions> {
        let port = self
            .port
            .unwrap_or(if self.ca_file.is_some() { 8883 } else { 1883 });
        let mut options = MqttOptions::new(&self.client_id, &self.server, port);
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
    let options = Options::parse();
    match run(&options).await {
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
    let (client, mut eventloop) = AsyncClient::new(options.mqtt_options()?, 10);
    let mut watch = Watch::new(options.config());
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
            Err(err) if !connected => bail!("Failed to connect to {}: {err}", options.server),
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

/// `F8:17:62:4E:4C:0B` to `f817624e4c0b`, as in the topics
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
        Options::command().debug_assert();
    }

    #[test]
    fn parses_macs() {
        assert_eq!(
            parse_mac("F8:17:62:4E:4C:0B").as_deref(),
            Ok("f817624e4c0b")
        );
        assert_eq!(parse_mac("f817624e4c0b").as_deref(), Ok("f817624e4c0b"));
        assert!(parse_mac("F8:17:62:4E:4C").is_err());
        assert!(parse_mac("G8:17:62:4E:4C:0B").is_err());
    }

    #[test]
    fn parses_commits() {
        assert_eq!(parse_commit("6534F11").as_deref(), Ok("6534f11"));
        assert!(parse_commit("6534f1").is_err());
        assert!(parse_commit("main").is_err());
    }
}
