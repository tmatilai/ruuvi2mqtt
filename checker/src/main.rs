//! Checks the ruuvi2mqtt app and the ESP32 boards on a test bed. See README.md.

mod check;
mod cli;
mod diagnostics;
#[cfg(test)]
mod fixtures;
mod mqtt;
mod payload;
mod record;
mod time;
mod topic;

use std::convert::Infallible;
use std::path::Path;
use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use tokio::time::{Instant, sleep_until};

use crate::check::{Report, Watch};
use crate::cli::{CheckOptions, Mode, RecordOptions};
use crate::mqtt::{Received, Session};
use crate::record::Recorder;

const PROGRESS_INTERVAL: Duration = Duration::from_mins(5);
const WRITE_INTERVAL: Duration = Duration::from_mins(1);

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    match cli::parse() {
        Mode::Check(options) => match check(&options).await {
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
        },
        Mode::Record(options) => {
            // It returns only on an error: systemd restarts it
            let Err(err) = record(&options).await;
            eprintln!("{err:#}");
            ExitCode::FAILURE
        }
    }
}

async fn check(options: &CheckOptions) -> Result<Report> {
    let mut session = Session::new(&options.broker, &options.client_id)?;
    let mut watch = Watch::new(options.config());
    let start = Instant::now();
    let deadline = start + options.duration;
    let mut next_progress = start + PROGRESS_INTERVAL;
    let mut connected = false;

    loop {
        let received = tokio::select! {
            () = sleep_until(deadline) => break,
            received = session.next() => received?,
        };
        let at = start.elapsed();
        match received {
            Received::Connected => {
                if connected {
                    eprintln!("+{} reconnected to the broker", time::format(at));
                }
                connected = true;
            }
            Received::Message(msg) => {
                if let Some(note) = watch.on_message(at, &msg.topic, &msg.payload, msg.retain) {
                    eprintln!("{note}");
                }
            }
            // Without a first connection, the instances can't be checked either
            Received::Lost(err) if !connected => {
                bail!("Failed to connect to {}: {err}", options.broker.server);
            }
            Received::Lost(err) => eprintln!("+{} MQTT: {err}", time::format(at)),
        }
        if Instant::now() >= next_progress {
            eprintln!("+{} {}", time::format(at), watch.progress());
            next_progress += PROGRESS_INTERVAL;
        }
    }
    Ok(watch.finish(start.elapsed()))
}

async fn record(options: &RecordOptions) -> Result<Infallible> {
    let (mut recorder, mut break_from) = load_state(options)?;
    let mut session = Session::new(&options.broker, &options.client_id)?;
    let mut connected = false;
    let mut next_write = Instant::now() + WRITE_INTERVAL;
    loop {
        tokio::select! {
            () = sleep_until(next_write) => {
                write_state(&options.state, &recorder.state(unix_now()))?;
                next_write += WRITE_INTERVAL;
            }
            received = session.next() => match received? {
                Received::Connected => {
                    eprintln!("connected to {}", options.broker.server);
                    recorder.resume(break_from, unix_now());
                    connected = true;
                }
                Received::Message(msg) => {
                    recorder.on_message(unix_now(), &msg.topic, &msg.payload, msg.retain);
                }
                Received::Lost(err) => {
                    eprintln!("MQTT: {err}");
                    if connected {
                        connected = false;
                        break_from = unix_now();
                    }
                }
            },
        }
    }
}

/// The recorder from its state file, and when the file was written.
fn load_state(options: &RecordOptions) -> Result<(Recorder, u64)> {
    let base_topic = &options.broker.base_topic;
    let path = &options.state;
    match std::fs::read_to_string(path) {
        Ok(state) => match Recorder::load(base_topic, &state) {
            Ok(loaded) => return Ok(loaded),
            Err(err) => eprintln!("starting over: {}: {err}", path.display()),
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(err).context(format!("Failed to read {}", path.display())),
    }
    let now = unix_now();
    Ok((Recorder::new(base_topic, now), now))
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
