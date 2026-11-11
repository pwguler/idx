//! Command surface: every documented command parses.

use assert_cmd::Command;

fn pipeline() -> Command {
    Command::cargo_bin("pipeline").unwrap()
}

#[test]
fn help_lists_every_command() {
    let out = pipeline()
        .arg("--help")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let help = String::from_utf8(out).unwrap();
    for cmd in ["fetch", "etl", "token", "calendar", "backfill"] {
        assert!(help.contains(cmd), "missing {cmd} in help:\n{help}");
    }
}

#[test]
fn fetch_requires_a_job() {
    pipeline().arg("fetch").assert().failure();
}
