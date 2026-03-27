use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

/// Create a temp home directory with the minimal structure for claudectx to run.
fn setup_home() -> TempDir {
    let home = TempDir::new().unwrap();
    fs::create_dir_all(home.path().join(".claudectx/contexts")).unwrap();
    home
}

fn claudectx(home: &TempDir) -> Command {
    let mut cmd = Command::cargo_bin("claudectx").unwrap();
    cmd.env("HOME", home.path()).env("NO_COLOR", "1");
    cmd
}

#[test]
fn cli_help() {
    let home = setup_home();
    claudectx(&home)
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("claudectx"));
}

#[test]
fn cli_version() {
    let home = setup_home();
    claudectx(&home)
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("claudectx"));
}

#[test]
fn cli_version_subcommand() {
    let home = setup_home();
    claudectx(&home)
        .arg("version")
        .assert()
        .success()
        .stdout(predicate::str::contains("claudectx"));
}

#[test]
fn cli_list_empty() {
    let home = setup_home();
    claudectx(&home)
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("No contexts"));
}

#[test]
fn cli_current_none() {
    let home = setup_home();
    claudectx(&home)
        .arg("current")
        .assert()
        .success()
        .stdout(predicate::str::contains("(none)"));
}

#[test]
fn cli_save_and_list_roundtrip() {
    let home = setup_home();

    // Create fake config files
    fs::write(home.path().join(".claude.json"), r#"{"token":"test"}"#).unwrap();

    claudectx(&home)
        .args(["save", "work"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Saved context"));

    claudectx(&home)
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("work"));
}

#[test]
fn cli_save_invalid_name() {
    let home = setup_home();
    fs::write(home.path().join(".claude.json"), r#"{"token":"test"}"#).unwrap();

    claudectx(&home)
        .args(["save", "a/b"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("must not contain"));
}

#[test]
fn cli_use_nonexistent() {
    let home = setup_home();
    claudectx(&home)
        .args(["use", "nope"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}

#[test]
fn cli_delete_nonexistent() {
    let home = setup_home();
    claudectx(&home)
        .args(["delete", "nope"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}

#[test]
fn cli_save_use_current_flow() {
    let home = setup_home();
    fs::write(home.path().join(".claude.json"), r#"{"token":"test"}"#).unwrap();

    claudectx(&home).args(["save", "work"]).assert().success();

    claudectx(&home)
        .arg("current")
        .assert()
        .success()
        .stdout(predicate::str::contains("work"));

    claudectx(&home).args(["use", "work"]).assert().success();

    claudectx(&home)
        .arg("current")
        .assert()
        .success()
        .stdout(predicate::str::contains("work"));
}

#[test]
fn cli_positional_use() {
    let home = setup_home();
    fs::write(home.path().join(".claude.json"), r#"{"token":"test"}"#).unwrap();

    claudectx(&home).args(["save", "work"]).assert().success();

    // Using positional arg (no subcommand) should act like `use`
    claudectx(&home)
        .arg("work")
        .assert()
        .success()
        .stdout(predicate::str::contains("Switched to context"));
}
