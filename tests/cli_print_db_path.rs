use std::process::Command;

fn run_cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_sessions-chronicle"))
        .args(args)
        .output()
        .expect("failed to run sessions-chronicle binary")
}

#[test]
fn print_db_path_outputs_plain_default_filename() {
    let output = run_cli(&["--print-db-path"]);

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    let path = stdout.trim_end();
    assert!(!path.is_empty());
    assert!(path.ends_with("sessions.db"));
}

#[test]
fn print_db_path_uses_override_filename_with_sessions_dir() {
    let sessions_dir = tempfile::tempdir().expect("failed to create temp sessions dir");
    let sessions_dir_arg = sessions_dir
        .path()
        .to_str()
        .expect("temp sessions path should be valid utf8")
        .to_owned();

    let output = run_cli(&["--sessions-dir", &sessions_dir_arg, "--print-db-path"]);

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    let path = stdout.trim_end();
    assert!(!path.is_empty());
    assert!(path.ends_with("sessions-override.db"));
}

#[test]
fn help_describes_print_db_path_option() {
    let output = run_cli(&["--help"]);

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    assert!(stdout.contains("--print-db-path"));
    assert!(stdout.contains("Print the resolved SQLite database path and exit"));
}

#[test]
fn print_db_path_after_gapplication_option_is_handled_locally() {
    let output = run_cli(&["--gapplication-service", "--print-db-path"]);

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    assert!(stdout.trim_end().ends_with("sessions.db"));
}

#[test]
fn sessions_dir_equals_form_selects_override_database() {
    let sessions_dir = tempfile::tempdir().expect("failed to create temp sessions dir");
    let argument = format!("--sessions-dir={}", sessions_dir.path().display());
    let output = run_cli(&[&argument, "--print-db-path"]);

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    assert!(stdout.trim_end().ends_with("sessions-override.db"));
}
