#[test]
fn help_lists_web_command() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_nanobot-rs"))
        .arg("--help")
        .output()
        .expect("run --help");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("web"));
}

#[test]
fn gateway_help_lists_embedded_web_bind_flags() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_nanobot-rs"))
        .arg("gateway")
        .arg("--help")
        .output()
        .expect("run gateway --help");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--web-host"), "{stdout}");
    assert!(stdout.contains("--web-port"), "{stdout}");
}
