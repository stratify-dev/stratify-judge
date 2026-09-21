use assert_cmd::Command;
use std::path::PathBuf;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn report_json() -> String {
    std::fs::read_to_string(fixtures().join("report-dead-code.json")).unwrap()
}

#[test]
fn without_an_api_key_the_report_passes_through_and_exits_zero() {
    let out = Command::cargo_bin("stratify-jev")
        .unwrap()
        .env_remove("TYPESAFE_API_KEY")
        .args([
            "--root",
            fixtures().join("sample-repo").to_str().unwrap(),
            "--format",
            "json",
        ])
        .write_stdin(report_json())
        .assert()
        .success();

    let body = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let input: serde_json::Value = serde_json::from_str(&report_json()).unwrap();
    // Byte-identical, not merely the right count: a pass-through that
    // corrupted a severity or a span would otherwise slip through.
    assert_eq!(v["findings"], input["findings"], "untouched");
    assert!(v["findings"][0].get("judgment").is_none());
}

/// A root we cannot read stops judgment, not the report. Verified against
/// the guarantee rather than against the exit code alone: an earlier
/// version printed a clean error and exit 2, which looks correct until you
/// notice stdout is empty and --fail-on never still failed.
#[test]
fn an_unreadable_root_still_passes_the_report_through() {
    let out = Command::cargo_bin("stratify-jev")
        .unwrap()
        .env_remove("TYPESAFE_API_KEY")
        .args(["--root", "/no/such/directory/anywhere", "--format", "json"])
        .write_stdin(report_json())
        .assert()
        .success();

    let body = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["findings"].as_array().unwrap().len(), 2, "the report still goes out");
}

#[test]
fn an_unreadable_root_respects_fail_on_never() {
    Command::cargo_bin("stratify-jev")
        .unwrap()
        .env_remove("TYPESAFE_API_KEY")
        .args([
            "--root",
            "/no/such/directory/anywhere",
            "--fail-on",
            "never",
        ])
        .write_stdin(report_json())
        .assert()
        .success()
        .stderr(predicates::str::contains("nothing was judged"));
}

#[test]
fn a_missing_key_warns_on_stderr_without_failing() {
    Command::cargo_bin("stratify-jev")
        .unwrap()
        .env_remove("TYPESAFE_API_KEY")
        .args(["--root", fixtures().join("sample-repo").to_str().unwrap()])
        .write_stdin(report_json())
        .assert()
        .success()
        .stderr(predicates::str::contains("TYPESAFE_API_KEY"));
}

#[test]
fn dry_run_reports_planned_requests_and_sends_nothing() {
    Command::cargo_bin("stratify-jev")
        .unwrap()
        .env("TYPESAFE_API_KEY", "not-a-real-key")
        .args([
            "--root",
            fixtures().join("sample-repo").to_str().unwrap(),
            "--dry-run",
        ])
        .write_stdin(report_json())
        .assert()
        .success()
        .stdout(predicates::str::contains("1 request"));
}

#[test]
fn fail_on_warning_exits_nonzero_when_a_warning_survives() {
    Command::cargo_bin("stratify-jev")
        .unwrap()
        .env_remove("TYPESAFE_API_KEY")
        .args([
            "--root",
            fixtures().join("sample-repo").to_str().unwrap(),
            "--fail-on",
            "warning",
        ])
        .write_stdin(report_json())
        .assert()
        .failure();
}

#[test]
fn an_unknown_schema_version_passes_through_with_a_warning() {
    let body = r#"{"schema_version":99,"findings":[]}"#;
    Command::cargo_bin("stratify-jev")
        .unwrap()
        .env_remove("TYPESAFE_API_KEY")
        .args(["--root", fixtures().join("sample-repo").to_str().unwrap()])
        .write_stdin(body)
        .assert()
        .success()
        .stderr(predicates::str::contains("schema_version 99"));
}
