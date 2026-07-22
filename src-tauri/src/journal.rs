//! Reader for pult's run journal (schema 1) — see `docs/run-journal.md` for
//! the full protocol spec. **The desktop app never writes into a run
//! directory**: pult itself is the single writer, regardless of who spawned
//! it or who's watching; this module only discovers, parses and tails what
//! pult already wrote.
//!
//! `state_dir`/`repo_key`/`writer_alive` mirror the pult repo's own
//! `src/journal.rs` byte-for-byte in semantics (same env var, same platform
//! defaults, same hash), so a repo's journal resolves to the same directory
//! whether pult or this app computes the path. `RunMeta` deserializes
//! tolerantly (no `deny_unknown_fields`) — schema 1 is additive-only, the
//! same convention `crate::types` already follows for pult's listing/doctor
//! surfaces.
//!
//! Every public entry point that touches the filesystem is a thin wrapper
//! resolving `state_dir()` (the only place this module reads
//! `PULT_STATE_DIR`) and delegating to an `_at(state, …)` sibling that takes
//! the state dir explicitly. Unit tests exercise the `_at` siblings directly
//! with tempdirs, rather than mutating process-global env vars — which would
//! race across the parallel test threads Rust runs by default.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter};

use crate::types::RunEvent;

/// Poll cadence while tailing a live run's `events.jsonl` — no fs-notify
/// dependency, just a cheap fallback poll (matches pult's own CLI tailer,
/// `tui`'s `src/runs.rs::FOLLOW_POLL`).
const FOLLOW_POLL: Duration = Duration::from_millis(150);

/// How long `tail_run` waits for pult to create a run's journal directory
/// before giving up. Covers the spawn-to-journal-init race (pult creates the
/// run dir shortly after the process starts) and a too-old pult binary that
/// doesn't understand `--run-id` at all — either way, nothing will ever
/// appear past this point.
const JOURNAL_APPEAR_TIMEOUT: Duration = Duration::from_secs(5);
const JOURNAL_APPEAR_POLL: Duration = Duration::from_millis(100);

/// How long a stopped run gets after `SIGTERM` before this app escalates to
/// `SIGKILL` — same grace period the old in-process stop flow used.
const STOP_GRACE: Duration = Duration::from_secs(3);

/// `meta.json` — one run's identity, audit record and outcome, as pult's
/// writer documents it (schema 1). Every field pult promises is here;
/// `pgid` is the one pult itself omits from the JSON entirely on Windows
/// (`skip_serializing_if`), hence the `#[serde(default)]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunMeta {
    pub schema: u32,
    pub run_id: String,
    #[serde(default)]
    pub repo_dir: Option<PathBuf>,
    #[serde(default)]
    pub manifest: Option<PathBuf>,
    pub command_id: String,
    pub command_title: String,
    #[serde(default)]
    pub params: HashMap<String, String>,
    #[serde(default = "default_origin")]
    pub origin: String,
    #[serde(default)]
    pub interactive: bool,
    #[serde(default)]
    pub pult_version: String,
    /// Pid of the *journaling pult process* — the writer to probe for
    /// crash detection, never the child it ran.
    pub pid: u32,
    /// Pult's process group (absent on Windows) — the stop capability.
    #[serde(default)]
    pub pgid: Option<i32>,
    pub started_at: String,
    pub status: Status,
    #[serde(default)]
    pub exit_code: Option<i32>,
    #[serde(default)]
    pub ended_at: Option<String>,
}

fn default_origin() -> String {
    "cli".to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Running,
    Exited,
    Stopped,
}

/// One entry in `list_runs`'s result — a display-ready summary, not
/// `RunMeta` verbatim, so the frontend never has to know the journal's
/// on-disk shape. `status` folds in the reader-derived `"crashed"` state
/// (`meta.json` itself only ever says running/exited/stopped — a crashed
/// writer can't record its own crash).
#[derive(Debug, Clone, Serialize)]
pub struct RunSummary {
    pub run_id: String,
    pub command_id: String,
    pub command_title: String,
    /// `"running" | "exited" | "stopped" | "crashed"`.
    pub status: String,
    pub exit_code: Option<i32>,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub origin: String,
    pub interactive: bool,
}

/// Whether `s` is safe to use as a run directory name — mirrors pult's own
/// `valid_run_id` (1-64 ASCII alphanumeric/`-` characters). This is the one
/// caller-supplied string that reaches a path join in this module, so every
/// entry point that takes a `run_id` routes through this first.
fn valid_run_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// The per-user state dir journals live under: `PULT_STATE_DIR` wins;
/// otherwise `~/.local/state/pult` (Linux, via XDG), `~/Library/Application
/// Support/pult/state` (macOS), `%LOCALAPPDATA%\pult\state` (Windows).
/// `None` means no journal can be resolved (no home dir at all, or
/// `PULT_STATE_DIR` explicitly set empty) — callers treat that as "no runs",
/// not an error.
pub fn state_dir() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("PULT_STATE_DIR") {
        if p.is_empty() {
            return None;
        }
        return Some(PathBuf::from(p));
    }
    #[cfg(target_os = "macos")]
    {
        dirs::data_dir().map(|d| d.join("pult").join("state"))
    }
    #[cfg(windows)]
    {
        dirs::data_local_dir().map(|d| d.join("pult").join("state"))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        dirs::state_dir()
            .map(|d| d.join("pult"))
            .or_else(|| dirs::home_dir().map(|h| h.join(".local/state/pult")))
    }
}

/// First 16 hex chars of SHA-256 of the canonical repo dir — matches pult's
/// own `repo_key` exactly, so both sides agree on the directory a repo's
/// journal lives under without either being a source of truth for the other.
pub fn repo_key(canonical_dir: &Path) -> String {
    let digest = Sha256::digest(canonical_dir.to_string_lossy().as_bytes());
    let mut key = String::with_capacity(16);
    for byte in &digest[..8] {
        key.push_str(&format!("{byte:02x}"));
    }
    key
}

/// This repo's `runs/` root under an explicit state dir — the seam tests use
/// directly, without touching `PULT_STATE_DIR`.
fn runs_root_at(state: &Path, repo_path: &str) -> PathBuf {
    let canonical = std::fs::canonicalize(repo_path).unwrap_or_else(|_| PathBuf::from(repo_path));
    state.join("repos").join(repo_key(&canonical)).join("runs")
}

/// One run's directory under an explicit state dir, given a caller-supplied
/// run id — the seam every reader here must route a `run_id` through rather
/// than joining directly (an unvalidated id could otherwise escape the
/// journal's own directory tree via `../` or an absolute path).
fn run_dir_for_at(state: &Path, repo_path: &str, run_id: &str) -> Result<PathBuf, String> {
    if !valid_run_id(run_id) {
        return Err(format!("`{run_id}` isn't a valid run id"));
    }
    Ok(runs_root_at(state, repo_path).join(run_id))
}

fn run_dir_for(repo_path: &str, run_id: &str) -> Result<PathBuf, String> {
    let state =
        state_dir().ok_or_else(|| "No run journal is available on this system".to_string())?;
    run_dir_for_at(&state, repo_path, run_id)
}

/// Read one run dir's `meta.json`. `None` for anything unreadable or
/// unparseable — readers skip, never error (forward compatibility, and
/// tolerance of a torn/mid-write meta file, though `meta.json` itself is
/// only ever rewritten via an atomic tmp+rename by the writer).
fn read_meta(run_dir: &Path) -> Option<RunMeta> {
    let raw = std::fs::read_to_string(run_dir.join("meta.json")).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Whether the journaling pult process (`meta.json`'s `pid` — the writer,
/// never its child) is still alive: `kill(pid, 0)`, an existence probe that
/// sends no signal. `EPERM` still means "exists" (owned by another user).
/// Non-unix has no cheap liveness probe (mirrors pult's own writer, which
/// disables journaling entirely there) and just says "alive" — the least
/// wrong answer when the question can't actually be asked.
#[cfg(unix)]
pub fn writer_alive(pid: u32) -> bool {
    unsafe extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    let pid = pid as i32;
    if pid <= 0 {
        return false;
    }
    unsafe { kill(pid, 0) == 0 || last_errno_is_eperm() }
}

#[cfg(unix)]
fn last_errno_is_eperm() -> bool {
    std::io::Error::last_os_error().raw_os_error() == Some(1) // EPERM
}

#[cfg(not(unix))]
pub fn writer_alive(_pid: u32) -> bool {
    true
}

fn to_summary(meta: RunMeta) -> RunSummary {
    let crashed = meta.status == Status::Running && !writer_alive(meta.pid);
    let status = if crashed {
        "crashed"
    } else {
        match meta.status {
            Status::Running => "running",
            Status::Exited => "exited",
            Status::Stopped => "stopped",
        }
    };
    RunSummary {
        run_id: meta.run_id,
        command_id: meta.command_id,
        command_title: meta.command_title,
        status: status.to_string(),
        exit_code: meta.exit_code,
        started_at: meta.started_at,
        ended_at: meta.ended_at,
        origin: meta.origin,
        interactive: meta.interactive,
    }
}

/// Every journaled run under `root`, newest first, tolerating unreadable/
/// foreign entries (no `meta.json`, unparseable JSON) — a garbage directory
/// must never break the listing. An absent `root` yields an empty list.
fn list_runs_in(root: &Path) -> Vec<RunSummary> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut runs: Vec<RunSummary> = entries
        .flatten()
        .filter_map(|e| read_meta(&e.path()))
        .map(to_summary)
        .collect();
    // RFC3339 UTC (the writer's `started_at` format) sorts lexically —
    // newest first. Run ids are UUIDv7 (also time-ordered) but `started_at`
    // is what the spec calls out as preferred.
    runs.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    runs
}

/// Every journaled run for `repo_path`, newest first. `None` state dir (no
/// home directory resolvable at all) or no `runs/` yet both yield an empty
/// list, never an error.
pub fn list_runs(repo_path: &str) -> Vec<RunSummary> {
    let Some(state) = state_dir() else {
        return Vec::new();
    };
    list_runs_in(&runs_root_at(&state, repo_path))
}

// --- Stopping a run ---------------------------------------------------

/// Stop a journaled run by its `pgid` — works identically for a run this app
/// never spawned, since the capability lives in the journal, not in any
/// in-process registry. `SIGTERM` to the whole group, a grace period, then
/// `SIGKILL` if the writer is still alive.
#[cfg(unix)]
pub async fn stop_run(repo_path: &str, run_id: &str) -> Result<(), String> {
    let run_dir = run_dir_for(repo_path, run_id)?;
    let meta = read_meta(&run_dir)
        .ok_or_else(|| "That run has already finished or was never journaled.".to_string())?;
    let pgid = meta.pgid.ok_or_else(|| {
        "This run has no recorded process group to stop (older pult binary?).".to_string()
    })?;

    signal_group(pgid, SIGTERM);

    let deadline = tokio::time::Instant::now() + STOP_GRACE;
    loop {
        if !writer_alive(meta.pid) {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    if writer_alive(meta.pid) {
        signal_group(pgid, SIGKILL);
    }
    Ok(())
}

#[cfg(not(unix))]
pub async fn stop_run(_repo_path: &str, _run_id: &str) -> Result<(), String> {
    Err("Stopping a run isn't supported on this platform yet.".to_string())
}

#[cfg(unix)]
const SIGTERM: i32 = 15;
#[cfg(unix)]
const SIGKILL: i32 = 9;

/// Send `sig` to a journaled process group — a negative pid targets the
/// whole group, so every child the run spawned is signaled too. Best
/// effort: `ESRCH` (group already gone) is ignored, since that's exactly
/// the state a stop wants to reach.
#[cfg(unix)]
fn signal_group(pgid: i32, sig: i32) {
    unsafe extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    unsafe {
        kill(-pgid, sig);
    }
}

// --- Tailing a run's events.jsonl ---------------------------------------

/// Map one `events.jsonl` line to the frontend's `RunEvent` wire shape.
/// `None` for an unparseable line or a `kind` this reader doesn't know
/// about — both are skipped, never errored (the same forward-compatibility
/// rule the wire protocol itself uses). Journaled `exit` events never carry
/// a `crashed` flag (crash is reader-derived, never journaled), so every
/// event mapped here sets it to `false`; only a *synthesized* exit (see
/// `synthesize_exit`) may set it `true`.
fn map_event_line(line: &str, run_id: &str) -> Option<RunEvent> {
    let doc: serde_json::Value = serde_json::from_str(line).ok()?;
    match doc.get("kind")?.as_str()? {
        "line" => Some(RunEvent::Line {
            run_id: run_id.to_string(),
            stream: doc.get("stream")?.as_str()?.to_string(),
            text: doc.get("text")?.as_str()?.to_string(),
        }),
        "step" => Some(RunEvent::Step {
            run_id: run_id.to_string(),
            k: doc.get("k")?.as_u64()? as u32,
            n: doc.get("n")?.as_u64()? as u32,
            name: doc.get("name")?.as_str()?.to_string(),
        }),
        "progress" => Some(RunEvent::Progress {
            run_id: run_id.to_string(),
            pct: doc.get("pct").and_then(|v| v.as_u64()).map(|v| v as u8),
            text: doc
                .get("text")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
        }),
        "status" => Some(RunEvent::Status {
            run_id: run_id.to_string(),
            text: doc.get("text")?.as_str()?.to_string(),
        }),
        "exit" => Some(RunEvent::Exit {
            run_id: run_id.to_string(),
            code: doc.get("code").and_then(|v| v.as_i64()).map(|v| v as i32),
            stopped: doc
                .get("stopped")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            crashed: false,
        }),
        _ => None,
    }
}

/// One pass over whatever has been appended to `file` since `*offset`,
/// mapping each complete record to a `RunEvent`. A torn final line (no
/// trailing newline yet — a crash mid-append) is "not yet written": rewind
/// to its start and pick it up whole on a later pass, matching the spec's
/// reader rule exactly.
fn drain_events(
    file: &mut std::fs::File,
    offset: &mut u64,
    run_id: &str,
) -> std::io::Result<Vec<RunEvent>> {
    file.seek(SeekFrom::Start(*offset))?;
    let mut reader = BufReader::new(&*file);
    let mut events = Vec::new();
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            break;
        }
        if !line.ends_with('\n') {
            break; // torn final line — not yet written, rewind next pass
        }
        *offset += n as u64;
        if let Some(event) = map_event_line(line.trim_end(), run_id) {
            events.push(event);
        }
    }
    Ok(events)
}

/// Drive one run's events file until the run is observed to be over: drain
/// what's there, and — as long as `observe` reports the run still running
/// with a live writer — sleep and repeat. `observe`/`sleep` are injected
/// (rather than hardcoded to `read_meta`/`std::thread::sleep`) so a test can
/// make the writer's exit-then-meta-flip race land deterministically,
/// mirroring pult's own CLI tailer (`tui`'s `src/runs.rs::follow_events`
/// test) exactly: **one more drain pass always happens after observing a
/// terminal state**, because the writer appends the `exit` event *then*
/// flips meta — anything written in that gap (always including a
/// same-tick `exit` event) would otherwise never be read.
fn follow_events(
    file: &mut std::fs::File,
    offset: &mut u64,
    run_id: &str,
    mut emit: impl FnMut(RunEvent),
    mut observe: impl FnMut() -> Option<RunMeta>,
    mut sleep: impl FnMut(),
) -> std::io::Result<()> {
    loop {
        for event in drain_events(file, offset, run_id)? {
            emit(event);
        }
        match observe() {
            Some(meta) if meta.status == Status::Running && writer_alive(meta.pid) => {
                sleep();
            }
            _ => {
                for event in drain_events(file, offset, run_id)? {
                    emit(event);
                }
                return Ok(());
            }
        }
    }
}

/// Build the terminal `Exit` event for a run whose events file ended
/// without one — either a crash (`meta` still says running but the writer
/// is dead) or a legitimate terminal meta whose `exit` line simply hasn't
/// been observed (shouldn't happen once `follow_events`'s final drain is
/// honored, but this is the honest fallback either way). `meta: None` means
/// even `meta.json` itself is gone/unreadable at this point — treated as a
/// crash too, since nothing else describes that state.
fn synthesize_exit(run_id: &str, meta: Option<&RunMeta>) -> RunEvent {
    match meta {
        Some(m) => {
            let crashed = m.status == Status::Running && !writer_alive(m.pid);
            RunEvent::Exit {
                run_id: run_id.to_string(),
                code: m.exit_code,
                stopped: m.status == Status::Stopped,
                crashed,
            }
        }
        None => RunEvent::Exit {
            run_id: run_id.to_string(),
            code: None,
            stopped: false,
            crashed: true,
        },
    }
}

/// Tail one run dir's `events.jsonl` to completion — backlog first (from
/// offset 0, so the frontend rebuilds full history), then live-follows a
/// running writer, and always ends in exactly one terminal `Exit` emission
/// (journaled if present, synthesized otherwise — see `synthesize_exit`).
/// Assumes `run_dir` and its `events.jsonl` already exist; callers waiting
/// out the spawn race go through `wait_for_run_dir_at` first.
fn tail_existing_run(run_dir: &Path, run_id: &str, mut emit: impl FnMut(RunEvent)) {
    let mut file = match std::fs::File::open(run_dir.join("events.jsonl")) {
        Ok(f) => f,
        Err(_) => {
            emit(synthesize_exit(run_id, read_meta(run_dir).as_ref()));
            return;
        }
    };
    let mut offset: u64 = 0;
    let mut saw_exit = false;
    let result = follow_events(
        &mut file,
        &mut offset,
        run_id,
        |event| {
            if matches!(event, RunEvent::Exit { .. }) {
                saw_exit = true;
            }
            emit(event);
        },
        || read_meta(run_dir),
        || std::thread::sleep(FOLLOW_POLL),
    );
    if result.is_err() {
        // An I/O error mid-tail (file vanished, permissions changed) is as
        // terminal as any other "nothing more is coming" state.
        saw_exit = false;
    }
    if !saw_exit {
        emit(synthesize_exit(run_id, read_meta(run_dir).as_ref()));
    }
}

/// Wait for pult to create `run_id`'s journal directory under an explicit
/// state dir, bounded by `timeout` (polling every `poll`). The seam tests
/// use directly; `wait_for_run_dir` below is the real-env-reading wrapper.
fn wait_for_run_dir_at(
    state: &Path,
    repo_path: &str,
    run_id: &str,
    timeout: Duration,
    poll: Duration,
) -> Option<PathBuf> {
    let run_dir = run_dir_for_at(state, repo_path, run_id).ok()?;
    let events_path = run_dir.join("events.jsonl");
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if events_path.exists() {
            return Some(run_dir);
        }
        if std::time::Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(poll);
    }
}

/// Emit the "pult never journaled this run" fallback: an explanatory error
/// line plus a failed `Exit`, for when a run's journal directory never
/// appears at all (too-old pult binary without `--run-id` support, or no
/// journal state dir resolvable in the first place).
fn emit_never_journaled(run_id: &str, mut emit: impl FnMut(RunEvent)) {
    emit(RunEvent::Line {
        run_id: run_id.to_string(),
        stream: "stderr".to_string(),
        text: "pult never journaled this run — check that pult is installed and supports \
               --run-id (older versions don't journal)"
            .to_string(),
    });
    emit(RunEvent::Exit {
        run_id: run_id.to_string(),
        code: None,
        stopped: false,
        crashed: false,
    });
}

/// The blocking core of a tail, seamed on an explicit state dir: waits out
/// the spawn race, then tails to completion (`tail_existing_run`), or — if
/// pult never journaled this run at all — emits the "never journaled"
/// fallback. Directly unit-testable with tempdir fixtures; `tail_run_blocking`
/// below is the real-env-reading wrapper `tail_run` actually calls.
fn tail_run_blocking_at(
    state: &Path,
    repo_path: &str,
    run_id: &str,
    timeout: Duration,
    poll: Duration,
    emit: impl FnMut(RunEvent),
) {
    match wait_for_run_dir_at(state, repo_path, run_id, timeout, poll) {
        Some(run_dir) => tail_existing_run(&run_dir, run_id, emit),
        None => emit_never_journaled(run_id, emit),
    }
}

/// Every step here is synchronous std I/O plus `std::thread::sleep`,
/// deliberately not async, so it can run on a `spawn_blocking` thread (see
/// `tail_run` below) without ever blocking the async executor.
fn tail_run_blocking(repo_path: &str, run_id: &str, emit: impl FnMut(RunEvent)) {
    match state_dir() {
        Some(state) => tail_run_blocking_at(
            &state,
            repo_path,
            run_id,
            JOURNAL_APPEAR_TIMEOUT,
            JOURNAL_APPEAR_POLL,
            emit,
        ),
        None => emit_never_journaled(run_id, emit),
    }
}

/// Registry of run ids currently being tailed, so hydration, a fresh spawn
/// and a poll can never double-tail the same run — `tail_run` on an
/// already-tailed id is a no-op. A tail removes its own entry the moment it
/// reaches its terminal emission.
#[derive(Clone, Default)]
pub struct TailRegistry(std::sync::Arc<std::sync::Mutex<std::collections::HashSet<String>>>);

impl TailRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Claim `run_id` for tailing; `true` if this call actually claimed it
    /// (nobody else was already tailing it).
    fn claim(&self, run_id: &str) -> bool {
        self.0.lock().unwrap().insert(run_id.to_string())
    }

    fn release(&self, run_id: &str) {
        self.0.lock().unwrap().remove(run_id);
    }
}

/// Start tailing `run_id`'s journal in `repo_path`, emitting mapped events
/// on the `pult://run-output` channel as a background task. A no-op if
/// `run_id` is already being tailed (see `TailRegistry`). Returns
/// immediately; the tail's outcome only ever reaches the frontend through
/// its terminal `Exit` emission.
///
/// Deliberately spawns via `tauri::async_runtime` rather than bare
/// `tokio::spawn`/`tokio::task::spawn_blocking`: this function is itself
/// synchronous (not `async fn`), and it's called from both an async command
/// (`run_command`, already polled inside the Tokio runtime — an ambient
/// reactor exists there) and a *sync* command (`tail_run`, which Tauri
/// dispatches on a plain thread with no ambient Tokio runtime at all — bare
/// `tokio::spawn` there panics with "there is no reactor running"). Tauri's
/// async-runtime handle is a process-global singleton set up once at
/// startup and usable from any thread, sync or async, so routing through it
/// here — in the one primitive every caller goes through — means no call
/// site, present or future, can get this wrong.
///
/// Generic over `R: tauri::Runtime` (rather than the `Wry`-defaulted
/// `AppHandle` alias) purely so
/// `tests::tail_run_does_not_panic_when_called_outside_any_tokio_runtime`
/// below can drive this with `tauri::test::mock_app`'s
/// `AppHandle<MockRuntime>` — every real call site still passes a plain
/// `AppHandle` (`= AppHandle<Wry>`) and `R` is inferred as `Wry` there, so
/// this is a no-op change for production behavior.
pub fn tail_run<R: tauri::Runtime>(
    app: AppHandle<R>,
    tails: TailRegistry,
    repo_path: String,
    run_id: String,
) {
    if !tails.claim(&run_id) {
        return;
    }
    tauri::async_runtime::spawn(async move {
        let emit_app = app.clone();
        let blocking_repo_path = repo_path.clone();
        let blocking_run_id = run_id.clone();
        let _ = tauri::async_runtime::spawn_blocking(move || {
            tail_run_blocking(&blocking_repo_path, &blocking_run_id, |event| {
                let _ = emit_app.emit("pult://run-output", event);
            });
        })
        .await;
        tails.release(&run_id);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_meta(run_dir: &Path, meta: &RunMeta) {
        std::fs::create_dir_all(run_dir).unwrap();
        std::fs::write(
            run_dir.join("meta.json"),
            serde_json::to_string_pretty(meta).unwrap(),
        )
        .unwrap();
    }

    fn sample_meta(run_id: &str, status: Status) -> RunMeta {
        RunMeta {
            schema: 1,
            run_id: run_id.to_string(),
            repo_dir: Some(PathBuf::from("/tmp/repo")),
            manifest: Some(PathBuf::from("/tmp/repo/pult.yaml")),
            command_id: "deploy".to_string(),
            command_title: "Deploy".to_string(),
            params: HashMap::new(),
            origin: "cli".to_string(),
            interactive: false,
            pult_version: "0.4.0".to_string(),
            pid: std::process::id(),
            pgid: Some(std::process::id() as i32),
            started_at: "2026-07-17T13:05:12.412Z".to_string(),
            status,
            exit_code: None,
            ended_at: None,
        }
    }

    /// A pid far above any real pid_max, so `kill(pid, 0)` reliably fails
    /// with `ESRCH` rather than racing a real process on the test machine.
    const DEAD_PID: u32 = 4_000_000_000;

    #[test]
    fn repo_key_matches_pult_and_is_stable_hex() {
        let key = repo_key(Path::new("/tmp/demo"));
        assert_eq!(key.len(), 16);
        assert!(key.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(key, repo_key(Path::new("/tmp/demo")));
        assert_ne!(key, repo_key(Path::new("/tmp/demo2")));
        // Golden value: sha256("/tmp/demo")'s first 16 hex chars, computed
        // independently — pins this reader to the exact same hash pult's
        // writer uses, not just "some stable hash".
        let digest = Sha256::digest(b"/tmp/demo");
        let expected: String = digest[..8].iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(key, expected);
    }

    #[test]
    fn valid_run_id_accepts_uuids_and_dashed_ids_rejects_traversal() {
        assert!(valid_run_id("550e8400-e29b-41d4-a716-446655440000"));
        assert!(valid_run_id("desktop-supplied-id"));
        assert!(!valid_run_id("../x"));
        assert!(!valid_run_id("/tmp/evil"));
        assert!(!valid_run_id("a/b"));
        assert!(!valid_run_id(""));
        assert!(!valid_run_id(&"x".repeat(65)));
    }

    #[test]
    fn run_dir_for_at_rejects_path_escaping_ids() {
        let state = tempfile::tempdir().unwrap();
        for bad in ["../x", "/tmp/evil", "a/b", ""] {
            assert!(
                run_dir_for_at(state.path(), "/some/repo", bad).is_err(),
                "id {bad:?} should be rejected"
            );
        }
    }

    #[test]
    fn writer_alive_is_false_for_a_pid_far_above_any_real_process() {
        assert!(!writer_alive(DEAD_PID));
    }

    #[test]
    fn writer_alive_is_true_for_this_test_process() {
        assert!(writer_alive(std::process::id()));
    }

    #[test]
    fn meta_parses_tolerating_unknown_fields() {
        let json = serde_json::json!({
            "schema": 1,
            "run_id": "r1",
            "repo_dir": "/tmp/repo",
            "manifest": "/tmp/repo/pult.yaml",
            "command_id": "deploy",
            "command_title": "Deploy",
            "params": {},
            "origin": "cli",
            "interactive": false,
            "pult_version": "0.4.0",
            "pid": 123,
            "pgid": 123,
            "started_at": "2026-07-17T13:05:12.412Z",
            "status": "running",
            "exit_code": null,
            "ended_at": null,
            "a_future_field_this_reader_has_never_heard_of": "surprise",
        });
        let meta: RunMeta = serde_json::from_value(json).expect("unknown fields must be tolerated");
        assert_eq!(meta.run_id, "r1");
        assert_eq!(meta.status, Status::Running);
        assert_eq!(meta.pgid, Some(123));
    }

    #[test]
    fn meta_parses_without_pgid_field_present_at_all() {
        // Windows-written meta: `pgid` is skipped entirely, not `null`.
        let json = serde_json::json!({
            "schema": 1,
            "run_id": "r1",
            "command_id": "deploy",
            "command_title": "Deploy",
            "pid": 123,
            "started_at": "2026-07-17T13:05:12.412Z",
            "status": "exited",
            "exit_code": 0,
        });
        let meta: RunMeta = serde_json::from_value(json).unwrap();
        assert_eq!(meta.pgid, None);
        assert_eq!(meta.origin, "cli", "origin must default when absent");
    }

    #[test]
    fn to_summary_derives_crashed_from_dead_writer() {
        let meta = sample_meta("r1", Status::Running);
        let mut dead = meta.clone();
        dead.pid = DEAD_PID;
        let summary = to_summary(dead);
        assert_eq!(summary.status, "crashed");

        let alive = to_summary(meta);
        assert_eq!(alive.status, "running");
    }

    #[test]
    fn to_summary_maps_every_terminal_status() {
        assert_eq!(
            to_summary(sample_meta("r1", Status::Exited)).status,
            "exited"
        );
        assert_eq!(
            to_summary(sample_meta("r1", Status::Stopped)).status,
            "stopped"
        );
    }

    #[test]
    fn list_runs_in_sorts_newest_first_and_skips_unreadable_dirs() {
        let state = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        let root = runs_root_at(state.path(), &repo.path().to_string_lossy());

        let mut older = sample_meta("older", Status::Exited);
        older.started_at = "2026-01-01T00:00:00.000Z".to_string();
        write_meta(&root.join("older"), &older);

        let mut newer = sample_meta("newer", Status::Running);
        newer.started_at = "2026-06-01T00:00:00.000Z".to_string();
        write_meta(&root.join("newer"), &newer);

        // Garbage entries a reader must tolerate, not error on.
        std::fs::create_dir_all(root.join("no-meta-at-all")).unwrap();
        let bad_dir = root.join("unparseable-meta");
        std::fs::create_dir_all(&bad_dir).unwrap();
        std::fs::write(bad_dir.join("meta.json"), "not json").unwrap();

        let runs = list_runs_in(&root);

        assert_eq!(runs.len(), 2, "garbage dirs must be skipped: {runs:?}");
        assert_eq!(runs[0].run_id, "newer");
        assert_eq!(runs[1].run_id, "older");
    }

    #[test]
    fn list_runs_in_is_empty_for_a_root_that_does_not_exist() {
        let state = tempfile::tempdir().unwrap();
        let missing_root = state.path().join("never-created");
        assert!(list_runs_in(&missing_root).is_empty());
    }

    fn append_line(path: &Path, json: &str) {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        writeln!(f, "{json}").unwrap();
    }

    #[test]
    fn drain_events_maps_every_kind_and_skips_unknown() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.jsonl");
        append_line(
            &path,
            r#"{"ts":1,"kind":"line","stream":"stdout","text":"hi"}"#,
        );
        append_line(
            &path,
            r#"{"ts":2,"kind":"step","k":1,"n":2,"name":"build"}"#,
        );
        append_line(&path, r#"{"ts":3,"kind":"progress","pct":50,"text":null}"#);
        append_line(
            &path,
            r#"{"ts":4,"kind":"progress","pct":null,"text":"thinking"}"#,
        );
        append_line(&path, r#"{"ts":5,"kind":"status","text":"verifying"}"#);
        append_line(&path, r#"{"ts":6,"kind":"hologram"}"#);
        append_line(&path, r#"{"ts":7,"kind":"exit","code":0,"stopped":false}"#);

        let mut file = std::fs::File::open(&path).unwrap();
        let mut offset = 0u64;
        let events = drain_events(&mut file, &mut offset, "r1").unwrap();

        assert_eq!(
            events.len(),
            6,
            "the unknown `hologram` kind must be skipped: {events:?}"
        );
        assert!(
            matches!(&events[0], RunEvent::Line { text, stream, .. } if text == "hi" && stream == "stdout")
        );
        assert!(matches!(&events[1], RunEvent::Step { k: 1, n: 2, name, .. } if name == "build"));
        assert!(matches!(
            &events[2],
            RunEvent::Progress {
                pct: Some(50),
                text: None,
                ..
            }
        ));
        assert!(matches!(
            &events[3],
            RunEvent::Progress { pct: None, text: Some(t), .. } if t == "thinking"
        ));
        assert!(matches!(&events[4], RunEvent::Status { text, .. } if text == "verifying"));
        assert!(matches!(
            &events[5],
            RunEvent::Exit {
                code: Some(0),
                stopped: false,
                crashed: false,
                ..
            }
        ));
    }

    #[test]
    fn drain_events_rewinds_a_torn_final_line_until_it_completes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.jsonl");
        append_line(
            &path,
            r#"{"ts":1,"kind":"line","stream":"stdout","text":"whole"}"#,
        );

        let mut file = std::fs::File::open(&path).unwrap();
        let mut offset = 0u64;

        // First pass: one complete line, then a torn one (a crash mid-write:
        // no trailing newline yet).
        {
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .unwrap();
            write!(
                f,
                r#"{{"ts":2,"kind":"line","stream":"stdout","text":"torn"#
            )
            .unwrap();
        }
        let events = drain_events(&mut file, &mut offset, "r1").unwrap();
        assert_eq!(
            events.len(),
            1,
            "the torn line must not be read yet: {events:?}"
        );
        assert!(matches!(&events[0], RunEvent::Line { text, .. } if text == "whole"));

        // The torn line completes; a later pass from the rewound offset
        // must pick it up whole, not read it twice or skip it.
        {
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .unwrap();
            writeln!(f, r#"-now-complete"}}"#).unwrap();
        }
        let events2 = drain_events(&mut file, &mut offset, "r1").unwrap();
        assert_eq!(events2.len(), 1);
        assert!(matches!(&events2[0], RunEvent::Line { text, .. } if text == "torn-now-complete"));
    }

    /// Mirrors pult's own deterministic race test (`tui`'s
    /// `src/runs.rs::follow_events_drains_once_more_after_observing_terminal_state`):
    /// the writer's `finish` appends the `exit` event *then* flips meta to a
    /// terminal status, so a reader that stops as soon as it observes "over"
    /// can miss the exit line itself when it lands in that exact gap. The
    /// injected `observe` closure below reproduces that gap on purpose,
    /// deterministically, rather than depending on real thread timing.
    #[test]
    fn follow_events_drains_once_more_after_observing_terminal_state() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.jsonl");
        append_line(
            &path,
            r#"{"ts":1,"kind":"line","stream":"stdout","text":"hello"}"#,
        );

        let mut file = std::fs::File::open(&path).unwrap();
        let mut offset = 0u64;
        let mut emitted = Vec::new();

        let meta = sample_meta("r1", Status::Exited);
        let mut appended = false;
        let path_for_closure = path.clone();
        let observe = move || {
            if !appended {
                appended = true;
                append_line(
                    &path_for_closure,
                    r#"{"ts":2,"kind":"exit","code":0,"stopped":false}"#,
                );
            }
            Some(meta.clone())
        };

        follow_events(
            &mut file,
            &mut offset,
            "r1",
            |e| emitted.push(e),
            observe,
            || panic!("must not sleep: the very first observe() already reports terminal"),
        )
        .unwrap();

        assert!(
            emitted.iter().any(|e| matches!(e, RunEvent::Exit { .. })),
            "the final drain must pick up the exit event appended during \
             the terminal observation: {emitted:?}"
        );
    }

    #[test]
    fn follow_events_polls_via_sleep_while_writer_is_alive_and_running() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.jsonl");
        std::fs::write(&path, "").unwrap();

        let mut file = std::fs::File::open(&path).unwrap();
        let mut offset = 0u64;
        let mut emitted = Vec::new();
        let mut ticks = 0;

        let running_meta = sample_meta("r1", Status::Running); // this test process's pid: alive
        let path_for_closure = path.clone();
        let observe = move || {
            ticks += 1;
            if ticks >= 3 {
                append_line(
                    &path_for_closure,
                    r#"{"ts":1,"kind":"exit","code":0,"stopped":false}"#,
                );
                let mut m = running_meta.clone();
                m.status = Status::Exited;
                return Some(m);
            }
            Some(running_meta.clone())
        };

        let mut sleeps = 0;
        follow_events(
            &mut file,
            &mut offset,
            "r1",
            |e| emitted.push(e),
            observe,
            || sleeps += 1,
        )
        .unwrap();

        assert_eq!(sleeps, 2, "should sleep once per still-running observation");
        assert!(emitted.iter().any(|e| matches!(e, RunEvent::Exit { .. })));
    }

    #[test]
    fn tail_existing_run_synthesizes_a_crash_exit_when_the_writer_is_dead() {
        let dir = tempfile::tempdir().unwrap();
        let mut meta = sample_meta("crashed-run", Status::Running);
        meta.pid = DEAD_PID; // never alive, so writer_alive() is false immediately
        write_meta(dir.path(), &meta);
        std::fs::write(dir.path().join("events.jsonl"), "").unwrap();

        let mut emitted = Vec::new();
        tail_existing_run(dir.path(), "crashed-run", |e| emitted.push(e));

        match emitted.last() {
            Some(RunEvent::Exit {
                crashed,
                code,
                stopped,
                ..
            }) => {
                assert!(
                    *crashed,
                    "a dead writer with meta still `running` must synthesize a crash"
                );
                assert_eq!(*code, None);
                assert!(!*stopped);
            }
            other => panic!("expected a synthesized Exit, got: {other:?}"),
        }
    }

    #[test]
    fn tail_existing_run_replays_backlog_then_emits_journaled_exit() {
        let dir = tempfile::tempdir().unwrap();
        let mut meta = sample_meta("finished-run", Status::Exited);
        meta.exit_code = Some(0);
        write_meta(dir.path(), &meta);
        let events_path = dir.path().join("events.jsonl");
        std::fs::write(
            &events_path,
            "{\"ts\":1,\"kind\":\"line\",\"stream\":\"stdout\",\"text\":\"a\"}\n\
             {\"ts\":2,\"kind\":\"line\",\"stream\":\"stdout\",\"text\":\"b\"}\n\
             {\"ts\":3,\"kind\":\"exit\",\"code\":0,\"stopped\":false}\n",
        )
        .unwrap();

        let mut emitted = Vec::new();
        tail_existing_run(dir.path(), "finished-run", |e| emitted.push(e));

        // Backlog replays first, in file order.
        assert!(matches!(&emitted[0], RunEvent::Line { text, .. } if text == "a"));
        assert!(matches!(&emitted[1], RunEvent::Line { text, .. } if text == "b"));
        match emitted.last() {
            Some(RunEvent::Exit {
                code: Some(0),
                stopped: false,
                crashed: false,
                ..
            }) => {}
            other => panic!("expected the journaled Exit, got: {other:?}"),
        }
        assert_eq!(
            emitted.len(),
            3,
            "no synthesized event on top of a real exit: {emitted:?}"
        );
    }

    #[test]
    fn tail_existing_run_synthesizes_exit_when_terminal_meta_has_no_exit_line() {
        // Meta already flipped to a terminal status, but the exit line
        // somehow never made it into events.jsonl (shouldn't happen once
        // follow_events's final drain is honored — this is the fallback).
        let dir = tempfile::tempdir().unwrap();
        let mut meta = sample_meta("no-exit-line", Status::Stopped);
        meta.exit_code = None;
        write_meta(dir.path(), &meta);
        std::fs::write(dir.path().join("events.jsonl"), "").unwrap();

        let mut emitted = Vec::new();
        tail_existing_run(dir.path(), "no-exit-line", |e| emitted.push(e));

        match emitted.last() {
            Some(RunEvent::Exit {
                stopped, crashed, ..
            }) => {
                assert!(*stopped);
                assert!(!*crashed);
            }
            other => panic!("expected a synthesized Exit, got: {other:?}"),
        }
    }

    #[test]
    fn wait_for_run_dir_at_times_out_when_nothing_ever_appears() {
        let state = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        let result = wait_for_run_dir_at(
            state.path(),
            &repo.path().to_string_lossy(),
            "never-appears",
            Duration::from_millis(30),
            Duration::from_millis(5),
        );
        assert!(result.is_none());
    }

    #[test]
    fn wait_for_run_dir_at_finds_a_dir_that_appears_mid_wait() {
        let state = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        let repo_path = repo.path().to_string_lossy().to_string();
        let root = runs_root_at(state.path(), &repo_path);
        let run_dir = root.join("appears-late");

        let root_for_thread = root.clone();
        let writer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            std::fs::create_dir_all(root_for_thread.join("appears-late")).unwrap();
            std::fs::write(root_for_thread.join("appears-late/events.jsonl"), "").unwrap();
        });

        let result = wait_for_run_dir_at(
            state.path(),
            &repo_path,
            "appears-late",
            Duration::from_secs(2),
            Duration::from_millis(5),
        );
        writer.join().unwrap();
        assert_eq!(result, Some(run_dir));
    }

    #[test]
    fn tail_run_blocking_at_synthesizes_a_failed_exit_when_pult_never_journals() {
        let state = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        let mut emitted = Vec::new();
        tail_run_blocking_at(
            state.path(),
            &repo.path().to_string_lossy(),
            "ghost-run",
            Duration::from_millis(30),
            Duration::from_millis(5),
            |e| emitted.push(e),
        );

        assert!(
            emitted
                .iter()
                .any(|e| matches!(e, RunEvent::Line { stream, .. } if stream == "stderr")),
            "should explain why nothing showed up: {emitted:?}"
        );
        match emitted.last() {
            Some(RunEvent::Exit {
                code: None,
                stopped: false,
                ..
            }) => {}
            other => panic!("expected a failed synthesized Exit, got: {other:?}"),
        }
    }

    #[test]
    fn tail_run_blocking_at_tails_a_run_created_after_a_short_delay() {
        // Exercises the full path: wait_for_run_dir_at succeeds once the
        // writer catches up, then tail_existing_run replays the backlog and
        // the journaled exit.
        let state = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        let repo_path = repo.path().to_string_lossy().to_string();
        let root = runs_root_at(state.path(), &repo_path);
        let run_dir = root.join("delayed-run");

        let meta = {
            let mut m = sample_meta("delayed-run", Status::Exited);
            m.exit_code = Some(0);
            m
        };
        let run_dir_for_thread = run_dir.clone();
        let writer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            write_meta(&run_dir_for_thread, &meta);
            std::fs::write(
                run_dir_for_thread.join("events.jsonl"),
                "{\"ts\":1,\"kind\":\"line\",\"stream\":\"stdout\",\"text\":\"hi\"}\n\
                 {\"ts\":2,\"kind\":\"exit\",\"code\":0,\"stopped\":false}\n",
            )
            .unwrap();
        });

        let mut emitted = Vec::new();
        tail_run_blocking_at(
            state.path(),
            &repo_path,
            "delayed-run",
            Duration::from_secs(2),
            Duration::from_millis(5),
            |e| emitted.push(e),
        );
        writer.join().unwrap();

        assert!(matches!(&emitted[0], RunEvent::Line { text, .. } if text == "hi"));
        match emitted.last() {
            Some(RunEvent::Exit {
                code: Some(0),
                crashed: false,
                ..
            }) => {}
            other => panic!("expected the journaled Exit, got: {other:?}"),
        }
    }

    #[test]
    fn tail_registry_claim_is_a_no_op_the_second_time() {
        let tails = TailRegistry::new();
        assert!(tails.claim("r1"), "first claim should succeed");
        assert!(
            !tails.claim("r1"),
            "a run already being tailed must not be claimed twice"
        );
        tails.release("r1");
        assert!(
            tails.claim("r1"),
            "after release, the run id can be claimed again"
        );
    }

    /// Regression for a field crash: `tail_run` is called from *two* Tauri
    /// command contexts — the async `run_command` (already polled inside
    /// the Tokio runtime, so an ambient reactor exists) and the *sync*
    /// `tail_run` command, which Tauri dispatches on a plain thread with no
    /// ambient Tokio runtime at all. Bare `tokio::spawn`/
    /// `tokio::task::spawn_blocking` there panicked immediately with
    /// "there is no reactor running, must be called from the context of a
    /// Tokio 1.x runtime" — switching repos in the real app (which tails
    /// hydrated history via the sync command) crashed the whole process.
    ///
    /// `tauri::test::mock_app` (the `test` feature, dev-only — see
    /// Cargo.toml) mints a real `AppHandle` without needing a window, so
    /// this reproduces the actual failure mode directly: call `tail_run`
    /// from a plain `std::thread` with no runtime anywhere in the picture,
    /// same as the sync command's real dispatch context. Revert `tail_run`
    /// to `tokio::spawn`/`tokio::task::spawn_blocking` and this goes red
    /// (the spawned thread panics); the `tauri::async_runtime` fix keeps it
    /// green, since that runtime handle is a process-global singleton
    /// usable from any thread, sync or async, runtime-context or none.
    #[test]
    fn tail_run_does_not_panic_when_called_outside_any_tokio_runtime() {
        let app = tauri::test::mock_app();
        let handle = app.handle().clone();
        let tails = TailRegistry::new();

        let joined = std::thread::spawn(move || {
            // No `#[tokio::test]`, no `Runtime::new().block_on(...)` — this
            // thread has never touched Tokio at all, matching exactly what
            // a sync `#[tauri::command]` gets dispatched onto.
            tail_run(
                handle,
                tails,
                "/some/never-opened/repo".to_string(),
                "no-runtime-thread".to_string(),
            );
        })
        .join();

        assert!(
            joined.is_ok(),
            "tail_run must not panic when called from a thread with no ambient Tokio runtime"
        );
    }

    // --- End-to-end against the real pult binary ---------------------------
    //
    // Everything above is fixture-driven and never touches a real pult
    // process. This one test spawns the actual `tui` repo's `pult` binary
    // the same way `pult_bin::spawn_run` does (detached, `--run-id`,
    // `PULT_ORIGIN=desktop`, params on stdin) and drives this module's own
    // reader functions against the journal it writes — proving the whole
    // stack end to end: backlog replay order, live-follow of a still-running
    // command, the journaled `Exit` mapping, a stop's `stopped: true`, and
    // `list_runs`'s crash detection against a group this test SIGKILLs
    // itself (bypassing pult's own graceful SIGTERM handling, so the writer
    // truly never gets to record its own outcome).
    //
    // Skips (with a printed note) rather than failing the suite if no pult
    // binary is available, matching `tests/pult_backend.rs`'s convention —
    // this crate doesn't vendor pult itself. `PULT_STATE_DIR` is the only
    // env var this test touches, and no other test in this file touches it,
    // so it's safe under Rust's default parallel test execution.
    mod real_pult_e2e {
        use super::*;
        use std::process::Stdio as StdStdio;

        fn fixture_repo() -> PathBuf {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/repo")
        }

        fn test_pult_bin() -> Option<PathBuf> {
            if let Ok(p) = std::env::var("PULT_DESKTOP_TEST_BIN") {
                let p = PathBuf::from(p);
                return p.is_file().then_some(p);
            }
            let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tui/target/debug/pult");
            let p = p.canonicalize().unwrap_or(p);
            if p.is_file() {
                Some(p)
            } else {
                eprintln!(
                    "skipping journal e2e test: no pult binary at {} \
                     (set PULT_DESKTOP_TEST_BIN to override)",
                    p.display()
                );
                None
            }
        }

        async fn trust_fixture(bin: &Path, trust_store: &Path) {
            let mut child = tokio::process::Command::new(bin)
                .args(["--trust", "--list"])
                .current_dir(fixture_repo())
                .env("PULT_TRUST_STORE", trust_store)
                .stdin(StdStdio::null())
                .stdout(StdStdio::null())
                .stderr(StdStdio::null())
                .spawn()
                .expect("failed to spawn pult --trust --list");
            let status = child.wait().await.expect("pult --trust --list didn't exit");
            assert!(status.success(), "pult --trust --list failed");
        }

        fn kill_group(pgid: i32) {
            let _ = std::process::Command::new("kill")
                .args(["-9", &format!("-{pgid}")])
                .stdout(StdStdio::null())
                .stderr(StdStdio::null())
                .status();
        }

        #[tokio::test]
        async fn journal_reader_end_to_end_against_the_real_pult_binary() {
            let Some(bin) = test_pult_bin() else { return };

            let state = tempfile::tempdir().unwrap();
            let trust_store = tempfile::NamedTempFile::new().unwrap();
            std::fs::remove_file(trust_store.path()).ok();
            let repo = fixture_repo();
            let repo_path = repo.to_string_lossy().to_string();

            unsafe {
                std::env::set_var("PULT_STATE_DIR", state.path());
                std::env::set_var("PULT_TRUST_STORE", trust_store.path());
            }
            trust_fixture(&bin, trust_store.path()).await;

            // --- 1. backlog replay + journaled Exit mapping, via `pipeline` ---
            // (steps + progress + status + two stdout lines + a clean exit —
            // exercises every mapped event kind in one finished run.)
            crate::pult_bin::spawn_run(
                &bin,
                &repo_path,
                "pipeline",
                &HashMap::new(),
                "e2e-pipeline",
            )
            .await
            .expect("spawn_run should succeed");

            let run_dir = wait_for_run_dir_at(
                state.path(),
                &repo_path,
                "e2e-pipeline",
                Duration::from_secs(5),
                Duration::from_millis(50),
            )
            .expect("pult should journal the pipeline run promptly");

            let mut pipeline_events = Vec::new();
            tail_existing_run(&run_dir, "e2e-pipeline", |e| pipeline_events.push(e));

            eprintln!("\n--- backlog replay order (pipeline) ---");
            for e in &pipeline_events {
                eprintln!("{e:?}");
            }

            // This reader must replay the file in exactly the order pult
            // wrote it (trivially true — it's a straight sequential read).
            // What's *not* guaranteed, discovered by running this against
            // the real binary: a strict global interleave between stdout
            // lines and PULT_EVENTS-sourced events. Pult reads its child's
            // stdout and its own event-channel pipe concurrently on separate
            // readers, so which one it happens to journal first is a race —
            // observed here as `release-step`'s stdout line landing in the
            // file *before* `build-step`'s own `progress` line and even
            // before `release-step`'s `step` marker. Nothing in
            // `docs/run-journal.md` promises otherwise (each record's own
            // `ts` is the only ordering primitive it documents), so this
            // isn't a reader bug or a spec violation — it's a real
            // characteristic of pult's writer worth knowing about. What IS
            // reliably ordered, and what this asserts: each stream's own
            // internal order (all `line`s in the child's actual stdout
            // order; all `step`/`progress`/`status` in the order pult's
            // event-channel reader saw them, matching the script's own
            // emission order) — and the run always ends in exactly one
            // `Exit`.
            let lines: Vec<&str> = pipeline_events
                .iter()
                .filter_map(|e| match e {
                    RunEvent::Line { text, .. } => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            assert_eq!(
                lines,
                vec!["build output", "release output"],
                "stdout's own order must be preserved: {pipeline_events:?}"
            );

            #[derive(Debug, PartialEq)]
            enum Marker {
                Step(u32),
                Progress,
                Status,
            }
            let markers: Vec<Marker> = pipeline_events
                .iter()
                .filter_map(|e| match e {
                    RunEvent::Step { k, .. } => Some(Marker::Step(*k)),
                    RunEvent::Progress { .. } => Some(Marker::Progress),
                    RunEvent::Status { .. } => Some(Marker::Status),
                    _ => None,
                })
                .collect();
            assert_eq!(
                markers,
                vec![Marker::Step(1), Marker::Progress, Marker::Step(2), Marker::Status],
                "the event-channel's own order must match the script's emission order: {pipeline_events:?}"
            );

            match pipeline_events.last() {
                Some(RunEvent::Exit {
                    code: Some(0),
                    stopped: false,
                    crashed: false,
                    ..
                }) => {}
                other => panic!("expected a clean journaled Exit, got: {other:?}"),
            }

            // --- 2. live-follow of a still-running command, then stop it ---
            crate::pult_bin::spawn_run(&bin, &repo_path, "sleep-loop", &HashMap::new(), "e2e-stop")
                .await
                .expect("spawn_run should succeed");
            let stop_run_dir = wait_for_run_dir_at(
                state.path(),
                &repo_path,
                "e2e-stop",
                Duration::from_secs(5),
                Duration::from_millis(50),
            )
            .expect("pult should journal the sleep-loop run promptly");

            let events_for_thread: std::sync::Arc<std::sync::Mutex<Vec<RunEvent>>> =
                std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
            let events_for_thread2 = events_for_thread.clone();
            let stop_run_dir2 = stop_run_dir.clone();
            let tail_thread = std::thread::spawn(move || {
                tail_existing_run(&stop_run_dir2, "e2e-stop", |e| {
                    events_for_thread2.lock().unwrap().push(e)
                });
            });

            // Give the live-follow loop a couple of poll cycles while the
            // run is genuinely still executing (`while true; do sleep 1;
            // done` never produces output, so there's nothing to backlog —
            // this specifically proves the tail is polling a live writer,
            // not just replaying a file that was already complete).
            tokio::time::sleep(Duration::from_millis(400)).await;
            stop_run(&repo_path, "e2e-stop")
                .await
                .expect("stop_run should succeed");
            tail_thread.join().expect("tail thread should not panic");

            let stop_events = events_for_thread.lock().unwrap().clone();
            eprintln!("\n--- live-follow + stop (sleep-loop) ---");
            for e in &stop_events {
                eprintln!("{e:?}");
            }
            match stop_events.last() {
                Some(RunEvent::Exit {
                    stopped: true,
                    crashed: false,
                    ..
                }) => {}
                other => panic!("expected a stopped Exit, got: {other:?}"),
            }

            // --- 3. crash detection: SIGKILL the group ourselves, bypassing
            // pult's own graceful SIGTERM handling entirely, so the writer
            // never gets to record its own outcome. ---
            crate::pult_bin::spawn_run(
                &bin,
                &repo_path,
                "sleep-loop",
                &HashMap::new(),
                "e2e-crash",
            )
            .await
            .expect("spawn_run should succeed");
            let crash_run_dir = wait_for_run_dir_at(
                state.path(),
                &repo_path,
                "e2e-crash",
                Duration::from_secs(5),
                Duration::from_millis(50),
            )
            .expect("pult should journal the sleep-loop run promptly");
            let crash_meta = read_meta(&crash_run_dir).expect("meta.json should be readable");
            let pgid = crash_meta.pgid.expect("meta should record a pgid on unix");

            kill_group(pgid);
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while writer_alive(crash_meta.pid) && std::time::Instant::now() < deadline {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            assert!(
                !writer_alive(crash_meta.pid),
                "the writer should be dead after SIGKILL"
            );

            let runs = list_runs(&repo_path);
            eprintln!("\n--- list_runs (after a SIGKILLed run) ---");
            for r in &runs {
                eprintln!("{r:?}");
            }
            let crashed = runs
                .iter()
                .find(|r| r.run_id == "e2e-crash")
                .expect("crashed run should be listed");
            assert_eq!(crashed.status, "crashed");

            unsafe {
                std::env::remove_var("PULT_STATE_DIR");
                std::env::remove_var("PULT_TRUST_STORE");
            }
        }
    }
}
