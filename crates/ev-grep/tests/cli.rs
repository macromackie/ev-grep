use std::{
    fs,
    io::Write,
    process::{Command, Stdio},
};

use anyhow::Result;
use serde_json::Value;

fn command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ev-grep"));
    for key in [
        "OPENROUTER_API_KEY",
        "TYPESAFE_API_KEY",
        "EV_GREP_PROVIDER",
        "EV_GREP_MODEL",
    ] {
        command.env_remove(key);
    }
    command
}

#[test]
fn dry_run_and_errors_form_machine_readable_streams_without_credentials() -> Result<()> {
    let dir = tempfile::tempdir()?;
    fs::write(dir.path().join("sample.rs"), "fn main() {}\n")?;
    let result = command()
        .args([
            "--dry-run",
            "--json",
            "--glob",
            "*.rs",
            "defines a function",
        ])
        .arg(dir.path())
        .output()?;
    assert!(result.status.success());
    let records: Vec<Value> = String::from_utf8(result.stdout)?
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?;
    assert_eq!(
        records.iter().filter(|r| r["type"] == "selected").count(),
        1
    );
    assert_eq!(
        records
            .last()
            .ok_or_else(|| anyhow::anyhow!("missing summary"))?["data"]["evaluated"],
        0
    );

    let result = command()
        .args(["--json", "--provider", "typesafe", "query"])
        .arg(dir.path().join("sample.rs"))
        .output()?;
    assert_eq!(result.status.code(), Some(2));
    let records: Vec<Value> = String::from_utf8(result.stdout)?
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?;
    assert!(records.iter().any(|r| {
        r["type"] == "error"
            && r["data"]["message"]
                .as_str()
                .is_some_and(|s| s.contains("TYPESAFE_API_KEY"))
    }));
    Ok(())
}

#[test]
fn query_from_stdin_and_cli_provider_override_are_unambiguous() -> Result<()> {
    let dir = tempfile::tempdir()?;
    fs::write(dir.path().join("sample.py"), "pass\n")?;
    let mut child = command()
        .env("EV_GREP_PROVIDER", "openrouter")
        .args([
            "--provider",
            "typesafe",
            "--model",
            "jev-1.13.0",
            "--dry-run",
            "--json",
            "-f",
            "-",
        ])
        .arg(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .ok_or_else(|| anyhow::anyhow!("missing stdin"))?
        .write_all(b"First line.\nSecond line.\n")?;
    let result = child.wait_with_output()?;
    assert!(result.status.success());
    let first: Value = serde_json::from_str(
        String::from_utf8(result.stdout)?
            .lines()
            .next()
            .ok_or_else(|| anyhow::anyhow!("missing begin"))?,
    )?;
    assert_eq!(first["data"]["provider"], "typesafe");
    Ok(())
}
