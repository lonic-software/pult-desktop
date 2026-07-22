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
//!
//! **Net invariant (fix round 2's generation-fenced tail restart —
//! `TailRegistry`, below):** at most one uncancelled emitter exists per
//! `run_id` at any moment. A run's `RunRecord` on the frontend is always
//! exactly "the journal prefix delivered by the current-generation tail" —
//! never a mix of two generations' events, and never silently missing
//! everything an old, cancelled tail would have replayed (see
//! `tail_run`/`TailRegistry::claim`).

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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

/// Grace period `wait_for_run_dir_at` gives the run dir to appear after
/// `SpawnOutcomes` reports the child already dead, before concluding this is
/// a genuine pre-journal spawn failure rather than a benign race between the
/// reaper recording the outcome and pult finishing its own directory setup.
const SPAWN_FAILURE_GRACE: Duration = Duration::from_millis(300);

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

/// The error `stop_run` returns for anything that isn't a live, running
/// writer — both "no meta at all" and (fix round 2's precheck) "meta exists
/// but the run has already finished or its writer is already dead" collapse
/// to the same operator-facing message, since from the caller's point of
/// view they're indistinguishable: there is nothing left to stop.
const RUN_ALREADY_FINISHED: &str = "That run has already finished or was never journaled.";

/// Whether `meta` describes a run that's actually live and stoppable right
/// now — a `Running` status backed by a live writer. Factored out from
/// `stop_run` (fix round 2, point fix C, closing the stale-pgid signal) so
/// the precheck is unit-testable without a filesystem, a repo dir, or
/// `PULT_STATE_DIR` at all: a `pgid` is only ever meaningful while its
/// writer is the live owner of that process group — once the writer's gone,
/// the OS is free to have recycled the same pgid for an unrelated process
/// tree, so signaling it is not "harmlessly do nothing" but "maybe kill
/// something that has nothing to do with this run."
fn is_stoppable(meta: &RunMeta) -> bool {
    meta.status == Status::Running && writer_alive(meta.pid)
}

/// Stop a journaled run by its `pgid` — works identically for a run this app
/// never spawned, since the capability lives in the journal, not in any
/// in-process registry. `SIGTERM` to the whole group, a grace period, then
/// `SIGKILL` if the writer is still alive.
///
/// Precheck (fix round 2, point fix C — see `is_stoppable`): bails out with
/// the same not-running error `read_meta`'s absence already uses, rather
/// than ever calling `signal_group` against a meta that isn't both
/// `Running` and backed by a live writer.
#[cfg(unix)]
pub async fn stop_run(repo_path: &str, run_id: &str) -> Result<(), String> {
    let run_dir = run_dir_for(repo_path, run_id)?;
    let meta = read_meta(&run_dir).ok_or_else(|| RUN_ALREADY_FINISHED.to_string())?;
    if !is_stoppable(&meta) {
        return Err(RUN_ALREADY_FINISHED.to_string());
    }
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
            tail_gen: None,
        }),
        "step" => Some(RunEvent::Step {
            run_id: run_id.to_string(),
            k: doc.get("k")?.as_u64()? as u32,
            n: doc.get("n")?.as_u64()? as u32,
            name: doc.get("name")?.as_str()?.to_string(),
            tail_gen: None,
        }),
        "progress" => Some(RunEvent::Progress {
            run_id: run_id.to_string(),
            pct: doc.get("pct").and_then(|v| v.as_u64()).map(|v| v as u8),
            text: doc
                .get("text")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            tail_gen: None,
        }),
        "status" => Some(RunEvent::Status {
            run_id: run_id.to_string(),
            text: doc.get("text")?.as_str()?.to_string(),
            tail_gen: None,
        }),
        "exit" => Some(RunEvent::Exit {
            run_id: run_id.to_string(),
            code: doc.get("code").and_then(|v| v.as_i64()).map(|v| v as i32),
            stopped: doc
                .get("stopped")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            crashed: false,
            tail_gen: None,
        }),
        _ => None,
    }
}

/// One pass over whatever has been appended to `file` since `*offset`,
/// mapping each complete record to a `RunEvent`. A torn final line (no
/// trailing newline yet — a crash mid-append) is "not yet written": rewind
/// to its start and pick it up whole on a later pass, matching the spec's
/// reader rule exactly.
///
/// Reads raw bytes (`read_until(b'\n', …)`) rather than `BufRead::read_line`
/// deliberately: `read_line` validates UTF-8 as it goes, so a torn read that
/// happens to split a multibyte character mid-sequence (a crash mid-append
/// of non-ASCII text — always possible, `text` is free-form command output)
/// makes it return an error immediately, killing the tail outright instead
/// of just rewinding and trying again next pass like every other kind of
/// torn line. Checking completeness (`ends_with(b'\n')`) on the raw bytes
/// *first*, before any UTF-8 conversion, means a torn multibyte tail hits
/// the exact same "not yet written, rewind" path as a torn ASCII one; the
/// lossy conversion only happens once a full line is already in hand, and is
/// purely for `map_event_line`'s `serde_json::from_str`, which needs `&str` —
/// `unwrap_or(REPLACEMENT_CHARACTER)`-style lossiness here would only ever
/// affect a line pult itself wrote with invalid UTF-8 in the first place
/// (this reader doesn't invent the torn read, it just must not crash on it).
fn drain_events(
    file: &mut std::fs::File,
    offset: &mut u64,
    run_id: &str,
) -> std::io::Result<Vec<RunEvent>> {
    file.seek(SeekFrom::Start(*offset))?;
    let mut reader = BufReader::new(&*file);
    let mut events = Vec::new();
    let mut buf: Vec<u8> = Vec::new();
    loop {
        buf.clear();
        let n = reader.read_until(b'\n', &mut buf)?;
        if n == 0 {
            break;
        }
        if buf.last() != Some(&b'\n') {
            break; // torn final line (possibly mid-multibyte) — not yet written, rewind next pass
        }
        *offset += n as u64;
        let line = String::from_utf8_lossy(&buf);
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
///
/// `cancel` (fix round 2's generation-fenced restart — `TailRegistry`) is
/// checked between every drain pass and right after every `sleep()`: the
/// moment it flips, this returns immediately without doing the
/// observe-terminal final drain — a cancelled tail's caller
/// (`tail_existing_run`) is responsible for never treating that early
/// return as "the run ended," i.e. never synthesizing an exit on top of it.
/// Events already drained and emitted before cancellation was noticed stay
/// emitted (they're real backlog); only the *decision to keep going* is
/// what stops.
fn follow_events(
    file: &mut std::fs::File,
    offset: &mut u64,
    run_id: &str,
    mut emit: impl FnMut(RunEvent),
    mut observe: impl FnMut() -> Option<RunMeta>,
    mut sleep: impl FnMut(),
    cancel: &AtomicBool,
) -> std::io::Result<()> {
    loop {
        for event in drain_events(file, offset, run_id)? {
            emit(event);
        }
        if cancel.load(Ordering::SeqCst) {
            return Ok(());
        }
        match observe() {
            Some(meta) if meta.status == Status::Running && writer_alive(meta.pid) => {
                sleep();
                if cancel.load(Ordering::SeqCst) {
                    return Ok(());
                }
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
                tail_gen: None,
            }
        }
        None => RunEvent::Exit {
            run_id: run_id.to_string(),
            code: None,
            stopped: false,
            crashed: true,
            tail_gen: None,
        },
    }
}

/// Tail one run dir's `events.jsonl` to completion — backlog first (from
/// offset 0, so the frontend rebuilds full history), then live-follows a
/// running writer, and always ends in exactly one terminal `Exit` emission
/// (journaled if present, synthesized otherwise — see `synthesize_exit`).
/// Assumes `run_dir` and its `events.jsonl` already exist; callers waiting
/// out the spawn race go through `wait_for_run_dir_at` first.
///
/// `cancel` (fix round 2 — see `TailRegistry`): checked before doing
/// anything, and again right after `follow_events` returns, in both cases
/// short-circuiting *without* emitting a synthesized exit — a cancelled
/// tail must never emit a terminal event at all, since the replacement tail
/// that cancelled it now owns this run_id and is the only one entitled to.
fn tail_existing_run(
    run_dir: &Path,
    run_id: &str,
    cancel: &AtomicBool,
    mut emit: impl FnMut(RunEvent),
) {
    if cancel.load(Ordering::SeqCst) {
        return;
    }
    let mut file = match std::fs::File::open(run_dir.join("events.jsonl")) {
        Ok(f) => f,
        Err(_) => {
            if !cancel.load(Ordering::SeqCst) {
                emit(synthesize_exit(run_id, read_meta(run_dir).as_ref()));
            }
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
        cancel,
    );
    if result.is_err() {
        // An I/O error mid-tail (file vanished, permissions changed) is as
        // terminal as any other "nothing more is coming" state.
        saw_exit = false;
    }
    if cancel.load(Ordering::SeqCst) {
        return;
    }
    if !saw_exit {
        emit(synthesize_exit(run_id, read_meta(run_dir).as_ref()));
    }
}

/// The outcome `wait_for_run_dir_at` reaches once it stops waiting: either
/// the run dir showed up, or one of two distinct "it never will" reasons —
/// see fix round 2's `SpawnOutcomes` doc comment for why these are kept
/// separate rather than collapsed into one generic failure.
#[derive(Debug)]
enum WaitForRunDir {
    Found(PathBuf),
    /// The 5s ceiling elapsed with nothing recorded either way: too-old
    /// pult binary without `--run-id` support, or no journal state dir
    /// resolvable at all. The genuinely-unknown case — `emit_never_journaled`
    /// is still the right fallback here.
    NeverJournaled,
    /// The spawned child was already dead (per `SpawnOutcomes`) and the run
    /// dir still hadn't appeared after `SPAWN_FAILURE_GRACE` — a pre-journal
    /// spawn failure (bad command id, bad flag, …) whose real diagnostics
    /// this reader can now report instead of the generic fallback text.
    SpawnFailed {
        exit_code: Option<i32>,
        stderr_tail: Vec<String>,
    },
    /// The tail was cancelled (fix round 2 — `TailRegistry`) while still
    /// waiting for the run dir. Never surfaces any emission at all: the
    /// tail that cancelled this one owns `run_id` now.
    Cancelled,
}

/// Wait for pult to create `run_id`'s journal directory under an explicit
/// state dir, bounded by `timeout` (polling every `poll`). While waiting,
/// also probes `outcomes` for a pre-journal spawn failure (fix round 2's
/// point fix B): if the reaper already recorded that the child died and the
/// run dir still hasn't appeared `SPAWN_FAILURE_GRACE` later, this aborts
/// immediately rather than sitting out the rest of the 5s ceiling — see
/// `WaitForRunDir::SpawnFailed`. The seam tests use directly; `wait_for_run_dir`
/// below is the real-env-reading wrapper.
fn wait_for_run_dir_at(
    state: &Path,
    repo_path: &str,
    run_id: &str,
    timeout: Duration,
    poll: Duration,
    outcomes: &SpawnOutcomes,
    cancel: &AtomicBool,
) -> WaitForRunDir {
    let Ok(run_dir) = run_dir_for_at(state, repo_path, run_id) else {
        return WaitForRunDir::NeverJournaled;
    };
    let events_path = run_dir.join("events.jsonl");
    let deadline = Instant::now() + timeout;
    let mut dead_since: Option<Instant> = None;
    loop {
        if events_path.exists() {
            return WaitForRunDir::Found(run_dir);
        }
        if cancel.load(Ordering::SeqCst) {
            return WaitForRunDir::Cancelled;
        }
        match outcomes.peek(run_id) {
            Some(entry) => {
                let since = *dead_since.get_or_insert_with(Instant::now);
                if since.elapsed() >= SPAWN_FAILURE_GRACE {
                    // Prefer the freshest read (`take`, which re-peeks under
                    // the lock) but fall back to what we already have if
                    // something else consumed it in the meantime.
                    let entry = outcomes.take(run_id).unwrap_or(entry);
                    return WaitForRunDir::SpawnFailed {
                        exit_code: entry.exit_code,
                        stderr_tail: entry.stderr_tail,
                    };
                }
            }
            None => dead_since = None,
        }
        if Instant::now() >= deadline {
            return WaitForRunDir::NeverJournaled;
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
        tail_gen: None,
    });
    emit(RunEvent::Exit {
        run_id: run_id.to_string(),
        code: None,
        stopped: false,
        crashed: false,
        tail_gen: None,
    });
}

/// Emit a captured pre-journal spawn failure (fix round 2's point fix B):
/// the real stderr pult itself wrote, line by line, then a real `Exit`
/// carrying the child's actual exit code — in place of
/// `emit_never_journaled`'s generic "pult never journaled this run" text,
/// which used to be the only fallback here regardless of whether pult
/// crashed instantly with a clear diagnostic or genuinely never ran at all.
fn emit_spawn_failure(
    run_id: &str,
    exit_code: Option<i32>,
    stderr_tail: &[String],
    mut emit: impl FnMut(RunEvent),
) {
    for line in stderr_tail {
        emit(RunEvent::Line {
            run_id: run_id.to_string(),
            stream: "stderr".to_string(),
            text: line.clone(),
            tail_gen: None,
        });
    }
    emit(RunEvent::Exit {
        run_id: run_id.to_string(),
        code: exit_code,
        stopped: false,
        crashed: false,
        tail_gen: None,
    });
}

/// The blocking core of a tail, seamed on an explicit state dir: waits out
/// the spawn race, then tails to completion (`tail_existing_run`), or — if
/// pult never journaled this run at all — emits the "never journaled"
/// fallback, or — if the run dir never appeared because the spawn itself
/// failed — emits the captured real stderr/exit instead (see
/// `WaitForRunDir`). Directly unit-testable with tempdir fixtures;
/// `tail_run_blocking` below is the real-env-reading wrapper `tail_run`
/// actually calls. `cancel` is threaded all the way through so a restart mid-wait
/// (fix round 2) never lets this emit anything at all on the way out.
#[allow(clippy::too_many_arguments)]
fn tail_run_blocking_at(
    state: &Path,
    repo_path: &str,
    run_id: &str,
    timeout: Duration,
    poll: Duration,
    outcomes: &SpawnOutcomes,
    cancel: &AtomicBool,
    emit: impl FnMut(RunEvent),
) {
    match wait_for_run_dir_at(state, repo_path, run_id, timeout, poll, outcomes, cancel) {
        WaitForRunDir::Found(run_dir) => tail_existing_run(&run_dir, run_id, cancel, emit),
        WaitForRunDir::NeverJournaled => {
            if !cancel.load(Ordering::SeqCst) {
                emit_never_journaled(run_id, emit);
            }
        }
        WaitForRunDir::SpawnFailed {
            exit_code,
            stderr_tail,
        } => {
            if !cancel.load(Ordering::SeqCst) {
                emit_spawn_failure(run_id, exit_code, &stderr_tail, emit);
            }
        }
        WaitForRunDir::Cancelled => {}
    }
}

/// Every step here is synchronous std I/O plus `std::thread::sleep`,
/// deliberately not async, so it can run on a `spawn_blocking` thread (see
/// `tail_run` below) without ever blocking the async executor.
fn tail_run_blocking(
    repo_path: &str,
    run_id: &str,
    outcomes: &SpawnOutcomes,
    cancel: &AtomicBool,
    emit: impl FnMut(RunEvent),
) {
    match state_dir() {
        Some(state) => tail_run_blocking_at(
            &state,
            repo_path,
            run_id,
            JOURNAL_APPEAR_TIMEOUT,
            JOURNAL_APPEAR_POLL,
            outcomes,
            cancel,
            emit,
        ),
        None => {
            if !cancel.load(Ordering::SeqCst) {
                emit_never_journaled(run_id, emit);
            }
        }
    }
}

/// One entry in `SpawnOutcomes`: what `pult_bin::spawn_run`'s reaper
/// observed once the freshly-spawned child exited, captured before this
/// module even knows whether pult ever got as far as creating a run
/// directory at all.
#[derive(Clone)]
struct SpawnOutcomeEntry {
    exit_code: Option<i32>,
    /// Bounded to ~8KB by the reaper (`pult_bin::spawn_run`), keeping the
    /// *tail* of the child's stderr on overflow — the most recent lines are
    /// almost always the actual diagnostic (a usage error, a panic), not
    /// whatever came first.
    stderr_tail: Vec<String>,
    recorded_at: Instant,
}

/// How long a `SpawnOutcomes` entry survives if nothing ever consumes it
/// (e.g. the run dir showed up after all, racing the reaper, so
/// `wait_for_run_dir_at` never needed it) — a simple sweep-on-insert, not a
/// background timer, is enough: this is a small backstop against an
/// unbounded map, not a correctness-critical TTL.
const SPAWN_OUTCOME_TTL: Duration = Duration::from_secs(60);

/// Pre-journal spawn diagnostics (fix round 2's point fix B), keyed by
/// `run_id`: `pult_bin::spawn_run`'s reaper records `(exit_code,
/// stderr_tail)` here the moment a freshly-spawned child exits, and
/// `wait_for_run_dir_at` probes it while waiting for the run directory to
/// appear — so a spawn that fails before pult ever gets to create its
/// journal (a bad command id, a bad flag, a too-old binary that chokes on
/// `--run-id`) surfaces pult's *real* stderr and exit code instead of the
/// generic "never journaled" fallback text, and does so within a couple
/// hundred milliseconds rather than sitting out the full 5s ceiling.
/// Distinct entries never collide across concurrent runs since `run_id` is
/// unique per invocation (same key space as `TailRegistry`).
#[derive(Clone, Default)]
pub struct SpawnOutcomes(Arc<Mutex<HashMap<String, SpawnOutcomeEntry>>>);

impl SpawnOutcomes {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a freshly-reaped child's outcome, evicting anything stale
    /// first (see `SPAWN_OUTCOME_TTL`).
    pub fn record(&self, run_id: &str, exit_code: Option<i32>, stderr_tail: Vec<String>) {
        let mut map = self.0.lock().unwrap();
        sweep(&mut map);
        map.insert(
            run_id.to_string(),
            SpawnOutcomeEntry {
                exit_code,
                stderr_tail,
                recorded_at: Instant::now(),
            },
        );
    }

    /// Look at `run_id`'s outcome without consuming it — used to decide
    /// *whether* to abort the wait; the actual abort re-fetches via `take`
    /// so the entry isn't left behind once it's been acted on.
    fn peek(&self, run_id: &str) -> Option<SpawnOutcomeEntry> {
        let mut map = self.0.lock().unwrap();
        sweep(&mut map);
        map.get(run_id).cloned()
    }

    /// Consume `run_id`'s outcome, if any — evicted the moment it's used
    /// (fix round 2's "entries evicted on consumption").
    fn take(&self, run_id: &str) -> Option<SpawnOutcomeEntry> {
        let mut map = self.0.lock().unwrap();
        sweep(&mut map);
        map.remove(run_id)
    }
}

fn sweep(map: &mut HashMap<String, SpawnOutcomeEntry>) {
    map.retain(|_, entry| entry.recorded_at.elapsed() < SPAWN_OUTCOME_TTL);
}

/// One tail's claim on a `run_id`: its generation number and the flag its
/// own follow-loop checks to know it's been superseded (see
/// `TailRegistry::claim`).
struct TailHandle {
    generation: u64,
    cancel: Arc<AtomicBool>,
}

/// Registry of run ids currently being tailed, generation-fenced (fix round
/// 2, closing the eject-remount backlog-loss gap): claiming an id that's
/// already present doesn't no-op — it cancels the existing tail (sets its
/// `cancel` flag) and hands out a fresh generation + cancel flag for a brand
/// new tail from offset 0, so restarting a lost or orphaned tail (e.g. a
/// device ejected and remounted while its run kept going — see
/// `+page.svelte`'s `startTail`) always recovers the *full* backlog rather
/// than silently picking up wherever the old, now-unwatched tail happened to
/// leave off. See this module's top-level doc comment for the resulting net
/// invariant.
#[derive(Clone, Default)]
pub struct TailRegistry(Arc<Mutex<HashMap<String, TailHandle>>>);

impl TailRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Claim `run_id` for a fresh tail: if one's already registered, its
    /// cancel flag is set (so its follow-loop stops without emitting a
    /// terminal event — see `follow_events`/`tail_existing_run`) and the
    /// generation bumps by one; an absent id starts at generation 1. Always
    /// succeeds — there is no "already claimed, no-op" outcome anymore (that
    /// was fine before generation-fencing existed, but is exactly the
    /// eject-remount backlog-loss bug once it does). Returns the freshly
    /// claimed generation and its cancel flag, both of which the caller must
    /// thread through the new tail's entire lifetime.
    fn claim(&self, run_id: &str) -> (u64, Arc<AtomicBool>) {
        let mut map = self.0.lock().unwrap();
        let generation = match map.get(run_id) {
            Some(existing) => {
                existing.cancel.store(true, Ordering::SeqCst);
                existing.generation + 1
            }
            None => 1,
        };
        let cancel = Arc::new(AtomicBool::new(false));
        map.insert(
            run_id.to_string(),
            TailHandle {
                generation,
                cancel: cancel.clone(),
            },
        );
        (generation, cancel)
    }

    /// Release `run_id`'s claim — but only if the registry still holds
    /// exactly the generation this caller was given. An old, since-cancelled
    /// tail finishing its own wind-down must never delete the entry a newer
    /// tail installed in the meantime; only the current generation's own
    /// release actually clears the slot.
    fn release(&self, run_id: &str, generation: u64) {
        let mut map = self.0.lock().unwrap();
        if map.get(run_id).map(|h| h.generation) == Some(generation) {
            map.remove(run_id);
        }
    }
}

/// Start tailing `run_id`'s journal in `repo_path`, emitting mapped events
/// (each stamped with this tail's generation — see `RunEvent::stamp_tail_gen`)
/// on the `pult://run-output` channel as a background task. **Always**
/// (re)starts tailing from offset 0, even if `run_id` is already being
/// tailed — see `TailRegistry::claim`'s doc comment for why that's the fix
/// round 2 behavior change from "claiming an already-tailed id is a no-op":
/// callers that only want to avoid double-tailing a run their own frontend
/// state is already handling must check that themselves before calling this
/// (see `+page.svelte`'s `startTail`/`maybeLazyTail`) — this function's job
/// is just "make the current-generation tail for `run_id` exist," not "was
/// someone already watching."
///
/// The claim (and its immediate `TailStart` emission) happens synchronously,
/// before any background work is spawned: a caller that calls this twice in
/// a row gets the *first* call's old tail cancelled and superseded
/// deterministically, in call order, not whenever the blocking thread pool
/// happens to schedule it. Returns immediately either way; the tail's
/// outcome only ever reaches the frontend through its terminal `Exit`
/// emission (or, if superseded before then, no terminal emission at all).
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
    outcomes: SpawnOutcomes,
    repo_path: String,
    run_id: String,
) {
    let (generation, cancel) = tails.claim(&run_id);
    let _ = app.emit(
        "pult://run-output",
        RunEvent::TailStart {
            run_id: run_id.clone(),
            tail_gen: generation,
        },
    );
    tauri::async_runtime::spawn(async move {
        let emit_app = app.clone();
        let blocking_repo_path = repo_path.clone();
        let blocking_run_id = run_id.clone();
        let _ = tauri::async_runtime::spawn_blocking(move || {
            tail_run_blocking(
                &blocking_repo_path,
                &blocking_run_id,
                &outcomes,
                &cancel,
                move |event| {
                    let _ = emit_app.emit("pult://run-output", event.stamp_tail_gen(generation));
                },
            );
        })
        .await;
        tails.release(&run_id, generation);
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

    // --- stop_run's precheck (fix round 2, point fix C) ---------------------
    //
    // `is_stoppable` is the pure decision `stop_run` bails out on *before*
    // ever calling `signal_group` — by the source's own control flow,
    // `signal_group` is unreachable whenever this returns `false` (the `if
    // !is_stoppable(&meta) { return Err(...) }` guard is strictly before
    // it), so these three cases alone establish "never signals" for the
    // scenarios stop_run's own doc comment calls out (a terminal status, and
    // a `Running` status whose writer is actually dead — the stale-pgid
    // case this fix closes). Testing the pure predicate directly avoids
    // `stop_run` itself needing a real `PULT_STATE_DIR`/filesystem/async
    // runtime for what's fundamentally a synchronous decision over a
    // `RunMeta` already in hand — a case picked up again in
    // `real_pult_e2e` below, which touches `stop_run` itself against a real,
    // already-finished journaled run. Revert-check: removing the
    // `is_stoppable` guard from `stop_run` (calling `signal_group`
    // unconditionally) doesn't change these three tests' outcomes since
    // they test the predicate in isolation — the revert-check that actually
    // matters is the e2e assertion below, which goes red without the guard
    // (stop_run would return `Ok(())` for an already-finished run instead of
    // the finished error).
    #[test]
    fn is_stoppable_is_false_for_a_terminal_status() {
        assert!(!is_stoppable(&sample_meta("r1", Status::Exited)));
        assert!(!is_stoppable(&sample_meta("r1", Status::Stopped)));
    }

    #[test]
    fn is_stoppable_is_false_for_a_running_status_with_a_dead_writer() {
        // The exact stale-pgid scenario this fix closes: `meta.json` still
        // says `Running` (the writer never got to record otherwise — a
        // crash), so without this precheck `stop_run` would happily signal
        // a pgid the OS may have long since recycled for something else.
        let mut meta = sample_meta("r1", Status::Running);
        meta.pid = DEAD_PID;
        assert!(!is_stoppable(&meta));
    }

    #[test]
    fn is_stoppable_is_true_for_a_running_status_with_a_live_writer() {
        assert!(is_stoppable(&sample_meta("r1", Status::Running))); // pid: this test process, alive
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

    #[test]
    fn drain_events_rewinds_a_torn_final_line_ending_mid_multibyte_instead_of_erroring() {
        // Fix round 2's point fix D: `read_line`'s UTF-8 validation used to
        // make a torn read that happens to split a multibyte character
        // return an error immediately, killing the tail outright instead of
        // rewinding like every other kind of torn line.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.jsonl");
        append_line(
            &path,
            r#"{"ts":1,"kind":"line","stream":"stdout","text":"whole"}"#,
        );

        let mut file = std::fs::File::open(&path).unwrap();
        let mut offset = 0u64;

        {
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .unwrap();
            // A line torn mid-multibyte: a valid JSON prefix followed by
            // just the *first* byte of "é" (0xC3 0xA9 in UTF-8) — the second
            // byte, the closing quote/brace, and the newline haven't been
            // written yet, exactly the shape a crash mid-append of
            // non-ASCII text produces.
            f.write_all(br#"{"ts":2,"kind":"line","stream":"stdout","text":""#)
                .unwrap();
            f.write_all(&[0xC3]).unwrap();
        }
        let events = drain_events(&mut file, &mut offset, "r1").unwrap();
        assert_eq!(
            events.len(),
            1,
            "the torn multibyte line must not be read yet, and must not error: {events:?}"
        );
        assert!(matches!(&events[0], RunEvent::Line { text, .. } if text == "whole"));

        // Completing it (second byte + close + newline) must be picked up
        // whole on the next pass, not error, not double-read.
        {
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .unwrap();
            f.write_all(&[0xA9]).unwrap();
            writeln!(f, r#""}}"#).unwrap();
        }
        let events2 = drain_events(&mut file, &mut offset, "r1").unwrap();
        assert_eq!(events2.len(), 1);
        assert!(matches!(&events2[0], RunEvent::Line { text, .. } if text == "é"));
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
            &AtomicBool::new(false),
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
            &AtomicBool::new(false),
        )
        .unwrap();

        assert_eq!(sleeps, 2, "should sleep once per still-running observation");
        assert!(emitted.iter().any(|e| matches!(e, RunEvent::Exit { .. })));
    }

    #[test]
    fn follow_events_stops_without_a_final_drain_delay_once_cancelled() {
        // A cancelled follow must return promptly (checked between drain
        // passes and right after `sleep()`) rather than waiting out however
        // long the writer stays "running" — this is what lets a superseded
        // tail wind down instead of lingering forever.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.jsonl");
        std::fs::write(&path, "").unwrap();

        let mut file = std::fs::File::open(&path).unwrap();
        let mut offset = 0u64;
        let mut emitted = Vec::new();
        let running_meta = sample_meta("r1", Status::Running);
        let cancel = AtomicBool::new(false);
        let mut sleeps = 0;

        follow_events(
            &mut file,
            &mut offset,
            "r1",
            |e| emitted.push(e),
            move || Some(running_meta.clone()), // always still running: never terminal on its own
            || {
                sleeps += 1;
                cancel.store(true, Ordering::SeqCst);
            },
            &cancel,
        )
        .unwrap();

        assert_eq!(
            sleeps, 1,
            "must stop right after the sleep that saw cancel flip"
        );
        assert!(
            emitted.is_empty(),
            "a cancelled follow must not synthesize/observe a terminal event: {emitted:?}"
        );
    }

    #[test]
    fn tail_existing_run_synthesizes_a_crash_exit_when_the_writer_is_dead() {
        let dir = tempfile::tempdir().unwrap();
        let mut meta = sample_meta("crashed-run", Status::Running);
        meta.pid = DEAD_PID; // never alive, so writer_alive() is false immediately
        write_meta(dir.path(), &meta);
        std::fs::write(dir.path().join("events.jsonl"), "").unwrap();

        let mut emitted = Vec::new();
        tail_existing_run(dir.path(), "crashed-run", &AtomicBool::new(false), |e| {
            emitted.push(e)
        });

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
        tail_existing_run(dir.path(), "finished-run", &AtomicBool::new(false), |e| {
            emitted.push(e)
        });

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
        tail_existing_run(dir.path(), "no-exit-line", &AtomicBool::new(false), |e| {
            emitted.push(e)
        });

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
    fn tail_existing_run_never_emits_a_synthesized_exit_once_cancelled() {
        // A still-running writer whose tail gets cancelled mid-follow must
        // wind down silently — the replacement tail that cancelled it owns
        // this run_id's terminal emission now, not this one.
        let dir = tempfile::tempdir().unwrap();
        let mut meta = sample_meta("cancelled-run", Status::Running);
        meta.pid = std::process::id(); // alive, so the follow loop actually polls
        write_meta(dir.path(), &meta);
        std::fs::write(
            dir.path().join("events.jsonl"),
            "{\"ts\":1,\"kind\":\"line\",\"stream\":\"stdout\",\"text\":\"a\"}\n",
        )
        .unwrap();

        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_for_thread = cancel.clone();
        let dir_path = dir.path().to_path_buf();
        let emitted: Arc<Mutex<Vec<RunEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let emitted_for_thread = emitted.clone();
        let thread = std::thread::spawn(move || {
            tail_existing_run(&dir_path, "cancelled-run", &cancel_for_thread, |e| {
                emitted_for_thread.lock().unwrap().push(e)
            });
        });

        // Let it replay the one backlog line and settle into its poll loop,
        // then cancel it — it must return without ever emitting an Exit.
        std::thread::sleep(Duration::from_millis(50));
        cancel.store(true, Ordering::SeqCst);
        thread.join().unwrap();

        let emitted = emitted.lock().unwrap();
        assert!(
            emitted.iter().any(|e| matches!(e, RunEvent::Line { .. })),
            "the backlog line before cancellation must still have been emitted: {emitted:?}"
        );
        assert!(
            !emitted.iter().any(|e| matches!(e, RunEvent::Exit { .. })),
            "a cancelled tail must never emit its synthesized exit: {emitted:?}"
        );
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
            &SpawnOutcomes::new(),
            &AtomicBool::new(false),
        );
        assert!(matches!(result, WaitForRunDir::NeverJournaled));
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
            &SpawnOutcomes::new(),
            &AtomicBool::new(false),
        );
        writer.join().unwrap();
        match result {
            WaitForRunDir::Found(dir) => assert_eq!(dir, run_dir),
            other => panic!("expected the run dir to be found, got: {other:?}"),
        }
    }

    #[test]
    fn wait_for_run_dir_at_aborts_early_on_a_recorded_spawn_failure() {
        let state = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        let repo_path = repo.path().to_string_lossy().to_string();

        let outcomes = SpawnOutcomes::new();
        outcomes.record(
            "failed-run",
            Some(2),
            vec!["error: unrecognized subcommand 'bogus'".to_string()],
        );

        let start = Instant::now();
        let result = wait_for_run_dir_at(
            state.path(),
            &repo_path,
            "failed-run",
            Duration::from_secs(5),
            Duration::from_millis(20),
            &outcomes,
            &AtomicBool::new(false),
        );
        let elapsed = start.elapsed();

        assert!(
            elapsed < Duration::from_secs(1),
            "must abort well before the 5s ceiling once a spawn failure is recorded: {elapsed:?}"
        );
        match result {
            WaitForRunDir::SpawnFailed {
                exit_code: Some(2),
                stderr_tail,
            } => {
                assert_eq!(stderr_tail, vec!["error: unrecognized subcommand 'bogus'"]);
            }
            other => {
                panic!("expected a captured spawn failure, got a different outcome: {other:?}")
            }
        }
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
            &SpawnOutcomes::new(),
            &AtomicBool::new(false),
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
            &SpawnOutcomes::new(),
            &AtomicBool::new(false),
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
    fn tail_registry_claim_bumps_generation_and_cancels_the_previous_handle() {
        let tails = TailRegistry::new();

        let (gen1, cancel1) = tails.claim("r1");
        assert_eq!(gen1, 1, "an absent id starts at generation 1");
        assert!(!cancel1.load(Ordering::SeqCst));

        let (gen2, cancel2) = tails.claim("r1");
        assert_eq!(
            gen2, 2,
            "claiming an already-tailed id must bump the generation, not no-op"
        );
        assert!(
            cancel1.load(Ordering::SeqCst),
            "the superseded handle's cancel flag must be set"
        );
        assert!(!cancel2.load(Ordering::SeqCst));

        // A stale release (an old, already-cancelled tail winding down) must
        // not evict the entry a newer generation installed.
        tails.release("r1", gen1);
        let (gen3, _cancel3) = tails.claim("r1");
        assert_eq!(
            gen3, 3,
            "a stale release must not have cleared the registry entry"
        );

        // Only the CURRENT generation's own release actually clears the slot.
        tails.release("r1", gen3);
        let (gen4, _cancel4) = tails.claim("r1");
        assert_eq!(
            gen4, 1,
            "after the current generation's release, the id is absent again and restarts at 1"
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
        let outcomes = SpawnOutcomes::new();

        let joined = std::thread::spawn(move || {
            // No `#[tokio::test]`, no `Runtime::new().block_on(...)` — this
            // thread has never touched Tokio at all, matching exactly what
            // a sync `#[tauri::command]` gets dispatched onto.
            tail_run(
                handle,
                tails,
                outcomes,
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

    /// Test-only mirror of `tail_run`'s claim → `TailStart` → blocking-tail →
    /// release sequence, minus the Tauri/async plumbing: everything the real
    /// `tail_run` does beyond that sequence is synchronous std I/O (see
    /// `tail_run_blocking`'s doc comment), so this is directly callable from
    /// a plain `std::thread` — exactly how the real blocking-pool thread
    /// executes it — with an explicit state dir and a plain emit callback.
    /// Used by the restart/cancellation test below to drive two concurrent
    /// tails of the same run_id without needing any Tauri machinery at all.
    #[allow(clippy::too_many_arguments)]
    fn tail_run_sync_at(
        state: &Path,
        repo_path: &str,
        run_id: &str,
        registry: &TailRegistry,
        outcomes: &SpawnOutcomes,
        timeout: Duration,
        poll: Duration,
        mut emit: impl FnMut(RunEvent),
    ) {
        let (generation, cancel) = registry.claim(run_id);
        emit(RunEvent::TailStart {
            run_id: run_id.to_string(),
            tail_gen: generation,
        });
        tail_run_blocking_at(
            state,
            repo_path,
            run_id,
            timeout,
            poll,
            outcomes,
            &cancel,
            |event| emit(event.stamp_tail_gen(generation)),
        );
        registry.release(run_id, generation);
    }

    #[test]
    fn restarting_a_tail_cancels_the_old_one_and_replays_from_offset_0_with_bumped_generation() {
        let state = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        let repo_path = repo.path().to_string_lossy().to_string();
        let root = runs_root_at(state.path(), &repo_path);
        let run_dir = root.join("restart-run");
        std::fs::create_dir_all(&run_dir).unwrap();

        let mut meta = sample_meta("restart-run", Status::Running);
        meta.pid = std::process::id(); // alive for the whole test
        write_meta(&run_dir, &meta);
        std::fs::write(
            run_dir.join("events.jsonl"),
            "{\"ts\":1,\"kind\":\"line\",\"stream\":\"stdout\",\"text\":\"a\"}\n\
             {\"ts\":2,\"kind\":\"line\",\"stream\":\"stdout\",\"text\":\"b\"}\n",
        )
        .unwrap();

        let registry = TailRegistry::new();
        let outcomes = SpawnOutcomes::new();

        let first_events: Arc<Mutex<Vec<RunEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let first_events2 = first_events.clone();
        let registry1 = registry.clone();
        let outcomes1 = outcomes.clone();
        let state1 = state.path().to_path_buf();
        let repo_path1 = repo_path.clone();
        let first_thread = std::thread::spawn(move || {
            tail_run_sync_at(
                &state1,
                &repo_path1,
                "restart-run",
                &registry1,
                &outcomes1,
                Duration::from_secs(2),
                Duration::from_millis(20),
                move |e| first_events2.lock().unwrap().push(e),
            );
        });

        // Let the first tail claim generation 1, emit its leading
        // `TailStart`, and replay the two-line backlog before restarting.
        std::thread::sleep(Duration::from_millis(150));
        {
            let events = first_events.lock().unwrap();
            assert!(
                matches!(
                    events.first(),
                    Some(RunEvent::TailStart { tail_gen: 1, .. })
                ),
                "the first tail must lead with generation 1: {events:?}"
            );
            assert_eq!(
                events
                    .iter()
                    .filter(|e| matches!(e, RunEvent::Line { .. }))
                    .count(),
                2,
                "the first tail must have replayed the full backlog by now: {events:?}"
            );
        }

        // Restart: a second tail_run for the same run_id must cancel the
        // first and replay from offset 0 with a bumped generation.
        let second_events: Arc<Mutex<Vec<RunEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let second_events2 = second_events.clone();
        let registry2 = registry.clone();
        let outcomes2 = outcomes.clone();
        let state2 = state.path().to_path_buf();
        let repo_path2 = repo_path.clone();
        let second_thread = std::thread::spawn(move || {
            tail_run_sync_at(
                &state2,
                &repo_path2,
                "restart-run",
                &registry2,
                &outcomes2,
                Duration::from_secs(2),
                Duration::from_millis(20),
                move |e| second_events2.lock().unwrap().push(e),
            );
        });

        // Give the first tail time to notice its cancel flag (checked every
        // FOLLOW_POLL cycle) and the second tail time to replay the backlog
        // and settle into its own follow-loop wait.
        std::thread::sleep(Duration::from_millis(400));

        // Finish the run for real, so both threads actually terminate: the
        // second (current) tail should pick this up and emit the journaled
        // Exit.
        {
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(run_dir.join("events.jsonl"))
                .unwrap();
            writeln!(f, r#"{{"ts":3,"kind":"exit","code":0,"stopped":false}}"#).unwrap();
        }
        let mut exited = meta.clone();
        exited.status = Status::Exited;
        exited.exit_code = Some(0);
        write_meta(&run_dir, &exited);

        first_thread.join().unwrap();
        second_thread.join().unwrap();

        let first = first_events.lock().unwrap().clone();
        let second = second_events.lock().unwrap().clone();

        assert!(
            !first.iter().any(|e| matches!(e, RunEvent::Exit { .. })),
            "a cancelled tail must never emit its synthesized exit: {first:?}"
        );
        assert!(
            matches!(
                second.first(),
                Some(RunEvent::TailStart { tail_gen: 2, .. })
            ),
            "the restart must lead with a bumped generation: {second:?}"
        );
        assert_eq!(
            second
                .iter()
                .filter(|e| matches!(e, RunEvent::Line { .. }))
                .count(),
            2,
            "the restart must replay the full backlog from offset 0: {second:?}"
        );
        match second.last() {
            Some(RunEvent::Exit { code: Some(0), .. }) => {}
            other => {
                panic!("expected the current-generation tail to emit the real Exit, got: {other:?}")
            }
        }
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

            let outcomes = SpawnOutcomes::new();
            let no_cancel = AtomicBool::new(false);

            // --- 1. backlog replay + journaled Exit mapping, via `pipeline` ---
            // (steps + progress + status + two stdout lines + a clean exit —
            // exercises every mapped event kind in one finished run.)
            crate::pult_bin::spawn_run(
                &bin,
                &repo_path,
                "pipeline",
                &HashMap::new(),
                "e2e-pipeline",
                &outcomes,
            )
            .await
            .expect("spawn_run should succeed");

            let run_dir = match wait_for_run_dir_at(
                state.path(),
                &repo_path,
                "e2e-pipeline",
                Duration::from_secs(5),
                Duration::from_millis(50),
                &outcomes,
                &no_cancel,
            ) {
                WaitForRunDir::Found(dir) => dir,
                other => panic!("pult should journal the pipeline run promptly, got: {other:?}"),
            };

            let mut pipeline_events = Vec::new();
            tail_existing_run(&run_dir, "e2e-pipeline", &no_cancel, |e| {
                pipeline_events.push(e)
            });

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

            // --- 1b. stop_run's precheck (fix round 2, point fix C): the
            // pipeline run above already finished (Status::Exited) — calling
            // stop_run on it now must return the finished error, not
            // silently signal an unrelated, possibly-recycled pgid. (Can
            // only assert on the returned `Err` here, not on "nothing got
            // signaled" — `signal_group` is best-effort and swallows ESRCH,
            // so a stray signal to a process group that no longer exists
            // wouldn't be observable from here either way; see
            // `is_stoppable`'s unit tests above for the isolated precheck
            // logic this exercises end to end.)
            let stop_on_finished = stop_run(&repo_path, "e2e-pipeline").await;
            assert_eq!(
                stop_on_finished.unwrap_err(),
                RUN_ALREADY_FINISHED,
                "stop_run on an already-finished run must return the finished error"
            );

            // --- 2. live-follow of a still-running command, then stop it ---
            crate::pult_bin::spawn_run(
                &bin,
                &repo_path,
                "sleep-loop",
                &HashMap::new(),
                "e2e-stop",
                &outcomes,
            )
            .await
            .expect("spawn_run should succeed");
            let stop_run_dir = match wait_for_run_dir_at(
                state.path(),
                &repo_path,
                "e2e-stop",
                Duration::from_secs(5),
                Duration::from_millis(50),
                &outcomes,
                &no_cancel,
            ) {
                WaitForRunDir::Found(dir) => dir,
                other => panic!("pult should journal the sleep-loop run promptly, got: {other:?}"),
            };

            let events_for_thread: std::sync::Arc<std::sync::Mutex<Vec<RunEvent>>> =
                std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
            let events_for_thread2 = events_for_thread.clone();
            let stop_run_dir2 = stop_run_dir.clone();
            let tail_thread = std::thread::spawn(move || {
                tail_existing_run(&stop_run_dir2, "e2e-stop", &AtomicBool::new(false), |e| {
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
                &outcomes,
            )
            .await
            .expect("spawn_run should succeed");
            let crash_run_dir = match wait_for_run_dir_at(
                state.path(),
                &repo_path,
                "e2e-crash",
                Duration::from_secs(5),
                Duration::from_millis(50),
                &outcomes,
                &no_cancel,
            ) {
                WaitForRunDir::Found(dir) => dir,
                other => panic!("pult should journal the sleep-loop run promptly, got: {other:?}"),
            };
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

            // --- 4. pre-journal spawn-failure capture (fix round 2, point
            // fix B): spawning an unrecognized command id is a clap-level
            // parse error — pult never gets far enough to create a run
            // directory at all (confirmed by hand against this exact
            // binary/fixture: `pult nonexistent-command-xyz --run-id … `
            // exits 2 with "error: unrecognized subcommand …" on stderr,
            // nothing on stdout, no run dir). Proves the reaper's captured
            // real stderr/exit code reach the tail path — instead of
            // `emit_never_journaled`'s generic text — well under the 5s
            // ceiling. Kept in this same env-scoped test rather than a
            // second `#[tokio::test]`, since `PULT_STATE_DIR` must be set as
            // a real process env var for the *spawned pult child* to read
            // (inherited, not passed explicitly), and this file's own
            // convention (see this module's header comment) is that only
            // one test here ever touches that process-global var, to stay
            // race-free under Rust's default parallel test execution.
            let spawn_failure_outcomes = SpawnOutcomes::new();
            let start = Instant::now();
            crate::pult_bin::spawn_run(
                &bin,
                &repo_path,
                "nonexistent-command-xyz",
                &HashMap::new(),
                "e2e-spawn-failure",
                &spawn_failure_outcomes,
            )
            .await
            .expect("spawn_run itself should still succeed — pult started, it just errored fast");

            // `tail_run_blocking_at` is deliberately synchronous (see its own
            // doc comment) — real callers always run it on a genuinely
            // separate OS thread (`tauri::async_runtime::spawn_blocking` in
            // production). This test's `#[tokio::test]` runtime is
            // single-threaded by default, so calling it directly here (on
            // the same task as `spawn_run`'s `tokio::spawn`ed reaper) would
            // starve that reaper of any chance to ever run — routed through
            // `tokio::task::spawn_blocking` instead, mirroring production's
            // execution shape exactly (own thread, awaited without blocking
            // the runtime).
            let state_for_wait = state.path().to_path_buf();
            let repo_path_for_wait = repo_path.clone();
            let outcomes_for_wait = spawn_failure_outcomes.clone();
            let spawn_failure_events = tokio::task::spawn_blocking(move || {
                let mut events = Vec::new();
                tail_run_blocking_at(
                    &state_for_wait,
                    &repo_path_for_wait,
                    "e2e-spawn-failure",
                    Duration::from_secs(5),
                    Duration::from_millis(50),
                    &outcomes_for_wait,
                    &AtomicBool::new(false),
                    |e| events.push(e),
                );
                events
            })
            .await
            .expect("blocking tail task should not panic");
            let elapsed = start.elapsed();

            eprintln!("\n--- pre-journal spawn-failure capture (elapsed {elapsed:?}) ---");
            for e in &spawn_failure_events {
                eprintln!("{e:?}");
            }

            assert!(
                elapsed < Duration::from_secs(2),
                "must short-circuit well under the 5s ceiling once the child's death \
                 is observed, not sit out the full wait: {elapsed:?}"
            );

            let captured_stderr: String = spawn_failure_events
                .iter()
                .filter_map(|e| match e {
                    RunEvent::Line { stream, text, .. } if stream == "stderr" => {
                        Some(text.as_str())
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            assert!(
                captured_stderr.contains("unrecognized subcommand"),
                "should surface pult's real stderr, not the generic never-journaled \
                 text: {captured_stderr:?}"
            );

            match spawn_failure_events.last() {
                Some(RunEvent::Exit {
                    code: Some(2),
                    stopped: false,
                    crashed: false,
                    ..
                }) => {}
                other => panic!("expected pult's real exit code (2), got: {other:?}"),
            }

            // --- 6. real-journal restart e2e (fix round 2, point fix A):
            // tail a still-running slow run, then call the generation-fenced
            // tail entry point again for the SAME run_id mid-run — the
            // second tail must replay the FULL backlog (the real line pult
            // already journaled before the restart, not just whatever
            // arrives afterward), and the superseded first tail must never
            // emit a synthesized exit. This is the actual eject-remount
            // recovery scenario (`+page.svelte`'s `startTail` after
            // `handleEjectDevice` then a remount), proven here against a
            // real journaled run rather than a hand-written fixture
            // `meta.json`.
            let registry = TailRegistry::new();
            crate::pult_bin::spawn_run(
                &bin,
                &repo_path,
                "slow-drip",
                &HashMap::new(),
                "e2e-restart",
                &outcomes,
            )
            .await
            .expect("spawn_run should succeed");

            match wait_for_run_dir_at(
                state.path(),
                &repo_path,
                "e2e-restart",
                Duration::from_secs(5),
                Duration::from_millis(50),
                &outcomes,
                &no_cancel,
            ) {
                WaitForRunDir::Found(_) => {}
                other => panic!("pult should journal the slow-drip run promptly, got: {other:?}"),
            }

            let first_events: Arc<Mutex<Vec<RunEvent>>> = Arc::new(Mutex::new(Vec::new()));
            let first_events2 = first_events.clone();
            let registry1 = registry.clone();
            let outcomes1 = outcomes.clone();
            let state1 = state.path().to_path_buf();
            let repo_path1 = repo_path.clone();
            let first_thread = std::thread::spawn(move || {
                tail_run_sync_at(
                    &state1,
                    &repo_path1,
                    "e2e-restart",
                    &registry1,
                    &outcomes1,
                    Duration::from_secs(5),
                    Duration::from_millis(50),
                    move |e| first_events2.lock().unwrap().push(e),
                );
            });

            // Give the first tail time to replay the one backlog line and
            // settle into its live-follow poll — `slow-drip` loops forever
            // until stopped, so it's still genuinely running at this point.
            tokio::time::sleep(Duration::from_millis(300)).await;
            {
                let events = first_events.lock().unwrap();
                assert!(
                    events
                        .iter()
                        .any(|e| matches!(e, RunEvent::Line { text, .. } if text == "first drop")),
                    "the first tail should have replayed the backlog line by now: {events:?}"
                );
            }

            // Restart mid-run.
            let second_events: Arc<Mutex<Vec<RunEvent>>> = Arc::new(Mutex::new(Vec::new()));
            let second_events2 = second_events.clone();
            let registry2 = registry.clone();
            let outcomes2 = outcomes.clone();
            let state2 = state.path().to_path_buf();
            let repo_path2 = repo_path.clone();
            let second_thread = std::thread::spawn(move || {
                tail_run_sync_at(
                    &state2,
                    &repo_path2,
                    "e2e-restart",
                    &registry2,
                    &outcomes2,
                    Duration::from_secs(5),
                    Duration::from_millis(50),
                    move |e| second_events2.lock().unwrap().push(e),
                );
            });

            tokio::time::sleep(Duration::from_millis(300)).await;
            {
                let events = second_events.lock().unwrap();
                assert!(
                    matches!(
                        events.first(),
                        Some(RunEvent::TailStart { tail_gen: 2, .. })
                    ),
                    "the restart must lead with a bumped generation: {events:?}"
                );
                assert!(
                    events
                        .iter()
                        .any(|e| matches!(e, RunEvent::Line { text, .. } if text == "first drop")),
                    "the restart must replay the FULL backlog from offset 0 (this is the \
                     eject-remount recovery this fix enables), not just whatever arrives \
                     after the restart: {events:?}"
                );
            }

            // Stop it so both threads actually terminate (slow-drip would
            // otherwise loop forever) — the second (current) tail should
            // observe the stop.
            stop_run(&repo_path, "e2e-restart")
                .await
                .expect("stop_run should succeed");
            first_thread
                .join()
                .expect("first tail thread should not panic");
            second_thread
                .join()
                .expect("second tail thread should not panic");

            let first = first_events.lock().unwrap().clone();
            let second = second_events.lock().unwrap().clone();
            eprintln!("\n--- real-journal restart mid-run (eject-remount recovery) ---");
            eprintln!("first tail:  {first:?}");
            eprintln!("second tail: {second:?}");
            assert!(
                !first.iter().any(|e| matches!(e, RunEvent::Exit { .. })),
                "the superseded first tail must never emit a synthesized exit: {first:?}"
            );
            match second.last() {
                Some(RunEvent::Exit { stopped: true, .. }) => {}
                other => panic!(
                    "expected the current-generation tail to observe the stop, got: {other:?}"
                ),
            }

            unsafe {
                std::env::remove_var("PULT_STATE_DIR");
                std::env::remove_var("PULT_TRUST_STORE");
            }
        }
    }
}
