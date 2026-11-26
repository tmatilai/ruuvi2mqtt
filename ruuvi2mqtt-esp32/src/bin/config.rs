//! Checks the configuration in the environment variables and writes the
//! `config` partition's contents to a file, for `make flash` and
//! `make config`. Runs on the computer, not on the board.
//!
//! Usage: `ruuvi2mqtt-config <chip> <file>`

use std::collections::BTreeMap;
use std::process::ExitCode;

use ruuvi2mqtt_esp32::config::{self, Chip, Config};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [chip, file] = args.as_slice() else {
        eprintln!("Usage: ruuvi2mqtt-config <chip> <file>");
        return ExitCode::FAILURE;
    };
    match run(chip, file) {
        Ok(()) => ExitCode::SUCCESS,
        Err(errors) => {
            for error in errors {
                eprintln!("Error: {error}");
            }
            ExitCode::FAILURE
        }
    }
}

fn run(chip_name: &str, file: &str) -> Result<(), Vec<String>> {
    let chip = Chip::find(chip_name).ok_or_else(|| vec![format!("Unknown CHIP: {chip_name}")])?;

    let mut values: BTreeMap<String, String> = config::NAMES
        .iter()
        .filter_map(|&name| Some((name.to_string(), std::env::var(name).ok()?)))
        .collect();
    values.insert("CHIP".to_string(), chip.name.to_string());
    if let Some(path) = std::env::var("MQTT_CA_FILE").ok().filter(|p| !p.is_empty()) {
        let pem = std::fs::read_to_string(&path)
            .map_err(|e| vec![format!("Failed to read MQTT_CA_FILE ({path}): {e}")])?;
        values.insert("MQTT_CA".to_string(), pem);
    }

    let config = Config::parse(&values, chip)?;
    let bytes = config::encode(&values).map_err(|e| vec![e])?;
    std::fs::write(file, bytes).map_err(|e| vec![format!("Failed to write {file}: {e}")])?;
    eprintln!(
        "Configuration of {} ({}) in {file}",
        config.device_hostname, chip.name
    );
    Ok(())
}
