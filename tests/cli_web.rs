#[test]
fn help_lists_web_command() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_sidekick"))
        .arg("--help")
        .output()
        .expect("run --help");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("web"));
    assert!(stdout.contains("status"));
    assert!(stdout.contains("users"));
}

#[test]
fn gateway_help_lists_embedded_web_bind_flags() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_sidekick"))
        .arg("gateway")
        .arg("--help")
        .output()
        .expect("run gateway --help");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--web-host"), "{stdout}");
    assert!(stdout.contains("--web-port"), "{stdout}");
    assert!(!stdout.contains("--config"), "{stdout}");
    assert!(!stdout.contains("--workspace"), "{stdout}");
}

#[test]
fn agent_help_requires_user_instead_of_legacy_config_overrides() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_sidekick"))
        .arg("agent")
        .arg("--help")
        .output()
        .expect("run agent --help");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--user"), "{stdout}");
    assert!(!stdout.contains("--config"), "{stdout}");
    assert!(!stdout.contains("--workspace"), "{stdout}");
}
