//! Checks the ruuvi2mqtt app and the ESP32 boards on a test bed. See README.md.

mod check;
mod cli;
mod diagnostics;
#[cfg(test)]
mod fixtures;
mod history;
mod mqtt;
mod payload;
mod record;
mod site;
mod stats;
mod time;
mod topic;

use std::convert::Infallible;
use std::path::Path;
use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use tokio::time::{Instant, sleep_until};

use crate::check::{Report, Watch};
use crate::cli::{CheckOptions, Mode, RecordOptions, SiteOptions};
use crate::mqtt::{Received, Session};
use crate::record::Recorder;
use crate::site::Site;

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
        Mode::Site(options) => match site(&options).await {
            Ok(table) => {
                print!("{table}");
                ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!("{err:#}");
                ExitCode::FAILURE
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

async fn site(options: &SiteOptions) -> Result<String> {
    let mut session = Session::new(&options.broker, &options.client_id)?;
    let mut site = Site::new(&options.broker.base_topic);
    let start = Instant::now();
    let deadline = start + options.duration;
    let mut connected = false;
    eprintln!("listening for {}", time::format(options.duration));
    loop {
        let received = tokio::select! {
            () = sleep_until(deadline) => break,
            received = session.next() => received?,
        };
        match received {
            Received::Connected => connected = true,
            Received::Message(msg) => site.on_message(&msg.topic, &msg.payload),
            Received::Lost(err) if !connected => {
                bail!("Failed to connect to {}: {err}", options.broker.server);
            }
            Received::Lost(err) => eprintln!("MQTT: {err}"),
        }
    }
    if site.is_empty() {
        bail!(
            "No diagnostics in {}/diagnostics/",
            options.broker.base_topic
        );
    }
    Ok(site.render(start.elapsed()))
}

async fn record(options: &RecordOptions) -> Result<Infallible> {
    let (mut recorder, mut break_from) = load_state(options)?;
    let mut session = Session::new(&options.broker, &options.client_id)?;
    let mut connected = false;
    let mut next_write = Instant::now() + WRITE_INTERVAL;
    loop {
        tokio::select! {
            () = sleep_until(next_write) => {
                let now = unix_now();
                // The page first: it updates the problems in the state. A
                // failed page doesn't stop the recording.
                if let Some(dir) = &options.page {
                    let page = recorder.page(now, &options.title, &options.expect, &read_runs(dir));
                    let written = write_file(dir, "index.html", &page.html)
                        .and_then(|()| write_file(dir, "problems.json", &page.problems.to_string()));
                    if let Err(err) = written {
                        eprintln!("{err:#}");
                    }
                }
                write_state(&options.state, &recorder.state(now))?;
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

/// The runner's `runs.json` in the page's directory, if it has written one
fn read_runs(dir: &Path) -> Result<Option<serde_json::Value>, String> {
    match std::fs::read_to_string(dir.join("runs.json")) {
        Ok(runs) => serde_json::from_str(&runs)
            .map(Some)
            .map_err(|err| format!("not JSON: {err}")),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err.to_string()),
    }
}

/// Writes a page's file through a temporary file, like the state
fn write_file(dir: &Path, name: &str, contents: &str) -> Result<()> {
    let path = dir.join(name);
    let tmp = dir.join(format!(".{name}.tmp"));
    std::fs::write(&tmp, contents)
        .and_then(|()| std::fs::rename(&tmp, &path))
        .with_context(|| format!("Failed to write {}", path.display()))
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}
