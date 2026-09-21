use assert_cmd::Command;
use serde_json::json;
use std::path::PathBuf;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn report_json() -> String {
    std::fs::read_to_string(fixtures().join("report-dead-code.json")).unwrap()
}

/// A dead_code answer for the fixture's one finding, keyed for a
/// single-finding batch (slot 0).
fn ok_answer_body() -> serde_json::Value {
    json!({
        "model": "jev-1.13.0",
        "answers": {
            "s0__framework_invoked": { "noul": 0.9 },
            "s0__test_only": { "noul": 0.05 },
            "s0__external_api": { "noul": 0.05 },
            "s0__resolver_missed_a_call": { "noul": 0.05 },
            "s0__explanation": {
                "choice": "framework_invoked",
                "probabilities": {},
                "confidence": 0.9
            }
        },
        "usage": { "input_tokens": 10, "output_tokens": 0 }
    })
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
    assert_eq!(
        v["findings"].as_array().unwrap().len(),
        2,
        "the report still goes out"
    );
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

/// C1: a wrong key must be unmistakable. Before the fix the tool printed
/// only "1 request(s) failed", identical to what a timeout would print.
#[tokio::test]
async fn an_auth_failure_names_authentication_not_just_a_count() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    Command::cargo_bin("stratify-jev")
        .unwrap()
        .env("TYPESAFE_API_KEY", "obviously-not-a-real-key")
        .args([
            "--root",
            fixtures().join("sample-repo").to_str().unwrap(),
            "--base-url",
            &server.uri(),
        ])
        .write_stdin(report_json())
        .assert()
        .success()
        .stderr(predicates::str::contains("invalid or missing API key"));
}

/// C2: a `--root` that exists but does not match the report it is judging
/// must warn, not silently judge empty state.
#[tokio::test]
async fn a_root_mismatched_with_the_report_warns_about_it() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok_answer_body()))
        .mount(&server)
        .await;

    // An empty, otherwise-valid directory: it exists, so RepoContext::new
    // succeeds, but it holds none of the files the report names.
    let wrong_root = tempfile::tempdir().unwrap();
    // A judged finding is cached, and the default --cache-dir is relative
    // to the current directory (M2). Point it at a tempdir too, so this
    // test does not leave a stray .stratify/ next to the crate's sources.
    let cache_dir = tempfile::tempdir().unwrap();

    Command::cargo_bin("stratify-jev")
        .unwrap()
        .env("TYPESAFE_API_KEY", "test-key")
        .args([
            "--root",
            wrong_root.path().to_str().unwrap(),
            "--base-url",
            &server.uri(),
            "--cache-dir",
            cache_dir.path().to_str().unwrap(),
        ])
        .write_stdin(report_json())
        .assert()
        .success()
        .stderr(predicates::str::contains("is --root correct"));
}

/// I2: `--show-dismissed` only changes what is displayed. It must not
/// change whether `--fail-on` trips.
#[test]
fn show_dismissed_does_not_change_the_exit_code() {
    let body = r#"{"schema_version":1,"findings":[{
        "rule":"dead_code","severity":"warning",
        "message":"possibly unused function `x`",
        "span":{"file":"src/lib.rs","start_byte":0,"end_byte":1,"start_line":1},
        "confidence":"unknown"
    }]}"#;

    let without = Command::cargo_bin("stratify-jev")
        .unwrap()
        .env_remove("TYPESAFE_API_KEY")
        .args([
            "--root",
            fixtures().join("sample-repo").to_str().unwrap(),
            "--fail-on",
            "warning",
        ])
        .write_stdin(body)
        .assert();
    let with_show = Command::cargo_bin("stratify-jev")
        .unwrap()
        .env_remove("TYPESAFE_API_KEY")
        .args([
            "--root",
            fixtures().join("sample-repo").to_str().unwrap(),
            "--fail-on",
            "warning",
            "--show-dismissed",
        ])
        .write_stdin(body)
        .assert();

    assert_eq!(
        without.get_output().status.code(),
        with_show.get_output().status.code(),
        "adding --show-dismissed must not change the exit code"
    );
}

/// M6: dry-run must print the token estimate the spec asks for, not just a
/// request count.
#[test]
fn dry_run_reports_a_token_estimate_too() {
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
        .stdout(predicates::str::contains("1 request"))
        .stdout(predicates::str::contains("token"));
}

/// M2: `--cache-dir` must resolve against the current directory, not
/// `--root`, or the cache lands as untracked files inside whatever repo is
/// being analysed.
#[tokio::test]
async fn the_cache_lands_under_the_current_directory_not_root() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok_answer_body()))
        .mount(&server)
        .await;

    let cwd = tempfile::tempdir().unwrap();
    Command::cargo_bin("stratify-jev")
        .unwrap()
        .current_dir(cwd.path())
        .env("TYPESAFE_API_KEY", "test-key")
        .args([
            "--root",
            fixtures().join("sample-repo").to_str().unwrap(),
            "--base-url",
            &server.uri(),
        ])
        .write_stdin(report_json())
        .assert()
        .success();

    assert!(
        cwd.path().join(".stratify/jev-cache").exists(),
        "cache must land under the current directory"
    );
    assert!(
        !fixtures().join("sample-repo/.stratify").exists(),
        "cache must not land under --root"
    );
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
