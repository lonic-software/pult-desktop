//! Real-mode smoke tests: these call the actual `pult` binary against the
//! fixture repo checked into `tests/fixtures/repo/`, exercising the same
//! code path the Tauri commands use (`pult_bin::run_capture`), just without
//! spinning up a Tauri `AppHandle` (that needs a real window/webview, which
//! isn't available headlessly — see the README's testing notes).
//!
//! The binary path defaults to this checkout's sibling `tui` repo build
//! output, overridable via `PULT_DESKTOP_TEST_BIN` for other machines/CI. If
//! it isn't found, tests print a note and skip rather than failing the
//! whole suite — this crate doesn't vendor pult itself.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use pult_desktop_lib::pult_bin::{resolve_pick_source, run_capture};
use pult_desktop_lib::types::{DoctorReport, Listing};

fn fixture_repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/repo")
}

fn default_pult_bin() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tui/target/debug/pult")
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from("/Volumes/SSD/Documents/lonic/tui/target/debug/pult"))
}

/// Resolve the pult binary for tests, or `None` to skip (with a printed note).
fn test_pult_bin() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("PULT_DESKTOP_TEST_BIN") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
        eprintln!(
            "PULT_DESKTOP_TEST_BIN is set but not a file: {}",
            p.display()
        );
        return None;
    }
    let p = default_pult_bin();
    if p.is_file() {
        Some(p)
    } else {
        eprintln!(
            "skipping: no pult binary at {} (set PULT_DESKTOP_TEST_BIN to override)",
            p.display()
        );
        None
    }
}

/// Run `pult <args>` in the fixture repo with a fresh, isolated trust store
/// (a temp file that doesn't exist yet) so tests never touch — or depend
/// on — the developer's real `~/Library/Application Support/pult/trust.json`.
async fn run_in_fixture(
    bin: &PathBuf,
    trust_store: &Path,
    args: &[&str],
    stdin_data: Option<&str>,
) -> std::process::Output {
    // run_capture doesn't take env vars, so we shell out one level deeper
    // here using the same piping contract it implements, but with
    // PULT_TRUST_STORE set. This keeps run_capture's signature simple for
    // the (single-user, single-trust-store) app while still letting tests
    // isolate themselves.
    use tokio::io::AsyncWriteExt;
    use tokio::process::Command;

    let mut cmd = Command::new(bin);
    cmd.args(args)
        .current_dir(fixture_repo())
        .env("PULT_TRUST_STORE", trust_store)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let mut child = cmd.spawn().expect("failed to spawn pult");
    if let Some(mut stdin) = child.stdin.take() {
        if let Some(data) = stdin_data {
            stdin.write_all(data.as_bytes()).await.unwrap();
        }
    }
    child.wait_with_output().await.expect("pult didn't exit")
}

#[tokio::test]
async fn listing_parses_and_starts_untrusted() {
    let Some(bin) = test_pult_bin() else { return };
    let trust_store = tempfile::NamedTempFile::new().unwrap();
    std::fs::remove_file(trust_store.path()).ok(); // start with no trust recorded

    let output = run_in_fixture(&bin, trust_store.path(), &["--list", "--json"], None).await;
    assert!(
        output.status.success(),
        "pult --list --json failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let listing: Listing = serde_json::from_slice(&output.stdout).expect("valid Listing json");
    assert_eq!(listing.schema, 1);
    assert!(!listing.trusted, "fresh trust store should be untrusted");
    assert_eq!(listing.name, "demo-repo");

    let ids: Vec<&str> = listing.commands.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(
        ids,
        vec![
            "aws:whoami",
            "aws:deploy",
            "shell",
            "status",
            "import",
            "pipeline",
            "sleep-loop"
        ]
    );

    assert_eq!(listing.includes.len(), 1);
    assert_eq!(listing.includes[0].name.as_deref(), Some("AWS Tooling"));

    // Grouping rule inputs: a secret param, a category, an include origin.
    let import = listing.commands.iter().find(|c| c.id == "import").unwrap();
    assert_eq!(import.category.as_deref(), Some("Deploy"));
    assert!(import
        .params
        .iter()
        .any(|p| p.name == "token" && p.secret == Some(true)));

    let deploy = listing
        .commands
        .iter()
        .find(|c| c.id == "aws:deploy")
        .unwrap();
    assert_eq!(deploy.origin.as_deref(), Some("./mods/aws"));

    // `steps` — null for a string-form (or no) `run:`, populated for a
    // list-form one built from named `use:` steps (see docs/reference.md's
    // "steps" field note and "Compiled step lists emit `step` events
    // automatically").
    assert_eq!(deploy.steps, None);
    assert_eq!(import.steps, None);
    let pipeline = listing
        .commands
        .iter()
        .find(|c| c.id == "pipeline")
        .unwrap();
    assert_eq!(
        pipeline.steps,
        Some(vec!["build-step".to_string(), "release-step".to_string()])
    );
}

#[tokio::test]
async fn trust_then_doctor_reports_readiness() {
    let Some(bin) = test_pult_bin() else { return };
    let trust_store = tempfile::NamedTempFile::new().unwrap();
    std::fs::remove_file(trust_store.path()).ok();

    let trust_out = run_in_fixture(&bin, trust_store.path(), &["--trust", "--list"], None).await;
    assert!(
        trust_out.status.success(),
        "pult --trust --list failed: {}",
        String::from_utf8_lossy(&trust_out.stderr)
    );

    let doctor_out = run_in_fixture(&bin, trust_store.path(), &["doctor", "--json"], None).await;
    // doctor's own exit code is 1 whenever any check failed — that's expected
    // here (the fixture's `import` command has a deliberately-broken check),
    // so we assert on the parsed JSON, not the exit code.
    let report: DoctorReport =
        serde_json::from_slice(&doctor_out.stdout).expect("valid DoctorReport json");

    assert_eq!(report.schema, 1);
    let by_id = |id: &str| report.commands.iter().find(|c| c.id == id).unwrap();

    assert_eq!(by_id("shell").ready, Some(true));
    assert_eq!(by_id("status").ready, Some(true));
    assert_eq!(by_id("import").ready, Some(false));
    assert_eq!(by_id("import").exit_code, Some(1));
    assert_eq!(by_id("aws:whoami").ready, Some(true));
    assert_eq!(
        by_id("aws:deploy").ready,
        None,
        "no check declared -> null, not a failure"
    );
    assert_eq!(by_id("aws:deploy").exit_code, None);
}

#[tokio::test]
async fn run_streams_output_via_params_json_and_exits_zero() {
    let Some(bin) = test_pult_bin() else { return };
    let trust_store = tempfile::NamedTempFile::new().unwrap();
    std::fs::remove_file(trust_store.path()).ok();

    run_in_fixture(&bin, trust_store.path(), &["--trust", "--list"], None).await;

    let output = run_in_fixture(
        &bin,
        trust_store.path(),
        &["status", "--params-json"],
        Some("{}"),
    )
    .await;

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("all good"), "stdout was: {stdout}");
}

#[tokio::test]
async fn run_keeps_secret_values_out_of_the_running_banner() {
    let Some(bin) = test_pult_bin() else { return };
    let trust_store = tempfile::NamedTempFile::new().unwrap();
    std::fs::remove_file(trust_store.path()).ok();

    run_in_fixture(&bin, trust_store.path(), &["--trust", "--list"], None).await;

    let output = run_in_fixture(
        &bin,
        trust_store.path(),
        &["import", "--params-json"],
        Some(r#"{"token":"hunter2","note":"hi"}"#),
    )
    .await;

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // The composed command actually receives the real value…
    assert!(
        stdout.contains("importing with token hunter2 note hi"),
        "stdout was: {stdout}"
    );
    // …but pult's own "running:" banner (stderr) redacts it.
    assert!(
        !stderr.contains("hunter2"),
        "secret leaked into the running banner: {stderr}"
    );

    // Also verify run_capture (what commands.rs actually uses) behaves the
    // same way for a simple case, end to end.
    let via_run_capture = run_capture(
        &bin,
        &fixture_repo().to_string_lossy(),
        &["--version"],
        None,
    )
    .await
    .expect("run_capture should succeed");
    assert!(via_run_capture.status.success());
}

/// `resolve_pick_source` (unlike every other function under test here) never
/// invokes `pult` to do the actual work of running the source command — it
/// only calls `pult --list --json` to read the manifest and check trust,
/// then shells out itself. That inner `pult --list --json` call goes through
/// `run_capture`, which — unlike the `run_in_fixture` helper above — doesn't
/// take a custom env, so it only sees `PULT_TRUST_STORE` if it's set on this
/// test process's own environment. Everything here therefore runs in one
/// `#[tokio::test]` function, sequentially, so this test is never racing
/// another test (or itself) over that process-global mutation — every other
/// test in this file passes its own `PULT_TRUST_STORE` override directly to
/// its child process via `.env(...)`, so it's unaffected either way.
#[tokio::test]
async fn resolve_pick_source_end_to_end() {
    let Some(bin) = test_pult_bin() else { return };
    let trust_store = tempfile::NamedTempFile::new().unwrap();
    std::fs::remove_file(trust_store.path()).ok();
    let repo = fixture_repo().to_string_lossy().to_string();

    unsafe {
        std::env::set_var("PULT_TRUST_STORE", trust_store.path());
    }

    // Untrusted manifest: refused before the source command ever runs — the
    // same trust gate `pult doctor` applies to `check:`.
    let mut values = HashMap::new();
    values.insert("region".to_string(), "eu-west-1".to_string());
    let untrusted = resolve_pick_source(&bin, &repo, "aws:deploy", "target", &values).await;
    assert!(
        untrusted.is_err(),
        "untrusted repo should refuse resolution"
    );
    assert!(
        untrusted.unwrap_err().to_lowercase().contains("trust"),
        "refusal should mention trust"
    );

    // Trust the manifest, same as the app's trust flow.
    run_in_fixture(&bin, trust_store.path(), &["--trust", "--list"], None).await;

    // Success + depends_on interpolation: the fixture's `target` param
    // sources `echo target-{region}-a; echo target-{region}-b`, so the
    // resolved options must reflect the `region` value we hand in — proving
    // both that resolution works and that `{region}` was actually
    // interpolated (not just literally passed through).
    let mut values = HashMap::new();
    values.insert("region".to_string(), "us-east-1".to_string());
    let options = resolve_pick_source(&bin, &repo, "aws:deploy", "target", &values)
        .await
        .expect("resolution should succeed once trusted");
    assert_eq!(options, vec!["target-us-east-1-a", "target-us-east-1-b"]);

    // A different region value produces different options — confirms this
    // isn't a cached/static result.
    let mut values = HashMap::new();
    values.insert("region".to_string(), "eu-west-1".to_string());
    let options = resolve_pick_source(&bin, &repo, "aws:deploy", "target", &values)
        .await
        .expect("resolution should succeed for a different region too");
    assert_eq!(options, vec!["target-eu-west-1-a", "target-eu-west-1-b"]);

    unsafe {
        std::env::remove_var("PULT_TRUST_STORE");
    }
}
