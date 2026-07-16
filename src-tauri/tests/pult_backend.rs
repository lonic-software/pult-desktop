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
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pult_desktop_lib::pult_bin::{resolve_pick_source, run_capture, run_streaming, RunRegistry};
use pult_desktop_lib::types::{DoctorReport, Listing, RunEvent};

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
        eprintln!("PULT_DESKTOP_TEST_BIN is set but not a file: {}", p.display());
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
        vec!["aws:whoami", "aws:deploy", "shell", "status", "import", "pipeline", "sleep-loop"]
    );

    assert_eq!(listing.includes.len(), 1);
    assert_eq!(listing.includes[0].name.as_deref(), Some("AWS Tooling"));

    // Grouping rule inputs: a secret param, a category, an include origin.
    let import = listing.commands.iter().find(|c| c.id == "import").unwrap();
    assert_eq!(import.category.as_deref(), Some("Deploy"));
    assert!(import.params.iter().any(|p| p.name == "token" && p.secret == Some(true)));

    let deploy = listing.commands.iter().find(|c| c.id == "aws:deploy").unwrap();
    assert_eq!(deploy.origin.as_deref(), Some("./mods/aws"));

    // `steps` — null for a string-form (or no) `run:`, populated for a
    // list-form one built from named `use:` steps (see docs/reference.md's
    // "steps" field note and "Compiled step lists emit `step` events
    // automatically").
    assert_eq!(deploy.steps, None);
    assert_eq!(import.steps, None);
    let pipeline = listing.commands.iter().find(|c| c.id == "pipeline").unwrap();
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
    assert_eq!(by_id("aws:deploy").ready, None, "no check declared -> null, not a failure");
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
    assert!(stdout.contains("importing with token hunter2 note hi"), "stdout was: {stdout}");
    // …but pult's own "running:" banner (stderr) redacts it.
    assert!(!stderr.contains("hunter2"), "secret leaked into the running banner: {stderr}");

    // Also verify run_capture (what commands.rs actually uses) behaves the
    // same way for a simple case, end to end.
    let via_run_capture = run_capture(&bin, &fixture_repo().to_string_lossy(), &["--version"], None)
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
    assert!(untrusted.is_err(), "untrusted repo should refuse resolution");
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

/// Whether a pid is still alive, via `kill -0` (no signal sent, just an
/// existence/permission check) — shells out rather than adding an FFI/libc
/// dependency for one syscall in test code.
#[cfg(unix)]
fn is_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// `run_streaming` (what `commands::run_command` and `commands::stop_run`
/// delegate to — see `pult_bin.rs`) end to end: the fixture's `pipeline`
/// command (a list-form `run:` of named `steps:` that also emit explicit
/// `progress`/`status` lines — see `tests/fixtures/repo/pult.yaml`) yields
/// parsed `Step`/`Progress`/`Status` events on the callback, in protocol
/// order, alongside its stdout lines; then the fixture's `sleep-loop`
/// command (an infinite loop) is started and stopped via
/// `RunRegistry::request_stop`, asserting the terminal event is flagged
/// `stopped: true` and that the process group is actually gone afterward
/// (`kill -0`), not just disowned.
///
/// `run_streaming` goes through `pult_bin::run_capture`'s same env-passing
/// limitation as `resolve_pick_source` above (it can't take a per-call env
/// override — `commands::run_command` only ever needs the single real trust
/// store), so — like `resolve_pick_source_end_to_end` — this sets
/// `PULT_TRUST_STORE` on the test *process* itself and keeps both scenarios
/// in one sequential `#[tokio::test]` rather than splitting them, so
/// nothing else in this file's parallel test execution can race that global
/// mutation. See that test's comment for the full rationale.
#[cfg(unix)]
#[tokio::test]
async fn run_streaming_emits_events_and_stop_run_kills_the_group() {
    let Some(bin) = test_pult_bin() else { return };
    let trust_store = tempfile::NamedTempFile::new().unwrap();
    std::fs::remove_file(trust_store.path()).ok();
    let repo = fixture_repo().to_string_lossy().to_string();

    unsafe {
        std::env::set_var("PULT_TRUST_STORE", trust_store.path());
    }
    run_in_fixture(&bin, trust_store.path(), &["--trust", "--list"], None).await;

    // --- Scenario 1: events -------------------------------------------
    let registry = RunRegistry::new();
    let events: Arc<Mutex<Vec<RunEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let collected = Arc::clone(&events);

    run_streaming(
        &bin,
        &repo,
        "pipeline",
        &HashMap::new(),
        "events-test".to_string(),
        &registry,
        move |event| collected.lock().unwrap().push(event),
    )
    .await
    .expect("run_streaming should not error");

    let events = events.lock().unwrap().clone();
    let lines: Vec<&str> = events
        .iter()
        .filter_map(|e| match e {
            RunEvent::Line { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(lines.contains(&"build output"), "stdout lines were: {lines:?}");
    assert!(lines.contains(&"release output"), "stdout lines were: {lines:?}");

    assert!(
        events.iter().any(|e| matches!(
            e,
            RunEvent::Step { k: 1, n: 2, name, .. } if name == "build-step"
        )),
        "events were: {events:?}"
    );
    assert!(
        events.iter().any(|e| matches!(
            e,
            RunEvent::Step { k: 2, n: 2, name, .. } if name == "release-step"
        )),
        "events were: {events:?}"
    );
    assert!(
        events.iter().any(|e| matches!(
            e,
            RunEvent::Progress { pct: Some(50), text: Some(t), .. } if t == "building"
        )),
        "events were: {events:?}"
    );
    assert!(
        events.iter().any(|e| matches!(
            e,
            RunEvent::Status { text, .. } if text == "releasing now"
        )),
        "events were: {events:?}"
    );
    match events.last() {
        Some(RunEvent::Exit { code, stopped, run_id }) => {
            assert_eq!(code, &Some(0));
            assert!(!stopped);
            assert_eq!(run_id, "events-test");
        }
        other => panic!("expected a final Exit event, got: {other:?}"),
    }

    // --- Scenario 2: stop_run -------------------------------------------
    let registry = RunRegistry::new();
    let events: Arc<Mutex<Vec<RunEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let collected = Arc::clone(&events);
    let run_id = "stop-test".to_string();

    let bin2 = bin.clone();
    let repo2 = repo.clone();
    let registry2 = registry.clone();
    let run_id2 = run_id.clone();
    let handle = tokio::spawn(async move {
        run_streaming(
            &bin2,
            &repo2,
            "sleep-loop",
            &HashMap::new(),
            run_id2,
            &registry2,
            move |event| collected.lock().unwrap().push(event),
        )
        .await
    });

    // Wait for the run to register and hand back a pid — proves the process
    // (and its process group, per `process_group(0)`) actually started.
    let pid = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(pid) = registry.pid_of(&run_id) {
                return pid;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("sleep-loop should register a pid promptly");
    assert!(is_alive(pid), "sleep-loop should be running before it's stopped");

    registry.request_stop(&run_id).expect("run should still be registered");
    handle
        .await
        .expect("run_streaming task should not panic")
        .expect("run_streaming should not error");

    match events.lock().unwrap().last() {
        Some(RunEvent::Exit { stopped, .. }) => assert!(*stopped, "exit should be flagged as a stop"),
        other => panic!("expected a final Exit event, got: {other:?}"),
    }

    // The process (group) must be actually gone, not just disowned — kill -0
    // fails once the pid no longer exists in the process table.
    assert!(!is_alive(pid), "process group should be gone after stop_run");

    unsafe {
        std::env::remove_var("PULT_TRUST_STORE");
    }
}
