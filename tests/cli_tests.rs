#[test]
fn cli_tests() {
    trycmd::TestCases::new().case("tests/cmd/*.toml");
}

#[test]
fn mqtt_credentials_from_env() {
    for (fixture, user, password, expected_user) in [
        ("check-config", None, None, "ruuvi2mqtt"),
        (
            "check-config-no-password",
            None,
            Some("env-secret"),
            "ruuvi2mqtt",
        ),
        ("check-config", Some("env-user"), None, "env-user"),
        (
            "check-config",
            Some("env-user"),
            Some("env-secret"),
            "env-user",
        ),
        ("check-config", None, Some(""), "ruuvi2mqtt"),
        (
            "check-config-env",
            Some("env-user"),
            Some("env-secret"),
            "env-user",
        ),
    ] {
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_ruuvi2mqtt"));
        command
            .args(["--check-config", "--log-level=DEBUG", "--config"])
            .arg(format!("tests/cmd/{fixture}.in/ruuvi2mqtt.yaml"))
            .env_remove("MQTT_USER")
            .env_remove("MQTT_PASSWORD")
            .env_remove("RUST_LOG");
        if let Some(user) = user {
            command.env("MQTT_USER", user);
        }
        if let Some(password) = password {
            command.env("MQTT_PASSWORD", password);
        }
        let output = command.output().unwrap();
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(output.status.success(), "{fixture}: {stderr}");
        assert!(
            stderr.contains(&format!("user: Some({expected_user:?})")),
            "{stderr}"
        );
        assert!(stderr.contains("password: Some(<REDACTED>)"), "{stderr}");
        assert!(!stderr.contains("env-secret"), "{stderr}");
    }
}

#[cfg(unix)]
#[test]
fn mqtt_env_invalid_unicode_does_not_expose_credentials() {
    use std::os::unix::ffi::OsStringExt;

    for name in ["MQTT_USER", "MQTT_PASSWORD"] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_ruuvi2mqtt"))
            .args([
                "--check-config",
                "--config",
                "tests/cmd/check-config.in/ruuvi2mqtt.yaml",
            ])
            .env_remove("MQTT_USER")
            .env_remove("MQTT_PASSWORD")
            .env(name, std::ffi::OsString::from_vec(b"secret\xff".to_vec()))
            .output()
            .unwrap();
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(!output.status.success());
        assert!(
            stderr.contains(&format!("{name} must contain valid Unicode")),
            "{stderr}"
        );
        assert!(!stderr.contains("secret"), "{stderr}");
    }
}
