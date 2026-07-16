//! Locating and invoking the `pult` binary.
//!
//! `pult` is resolved from (in order): a path saved in the settings store,
//! then `which pult` on the user's PATH, then a bundled sidecar binary next
//! to the app's own executable — see [`resolve_pult`]. The sidecar is a
//! checksummed pult release binary fetched per-target-triple at package
//! time by `scripts/fetch-pult-sidecar.mjs` (pinned version + checksums in
//! `src-tauri/sidecar.json`) and registered via `tauri.conf.json`'s
//! `bundle.externalBin`; see the README's "Sidecar bundling" section.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Output, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::AppHandle;
use tauri_plugin_store::StoreExt;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::oneshot;
use tokio::time::timeout;

use crate::types::{Listing, RunEvent};

const SETTINGS_STORE: &str = "settings.json";
const PULT_PATH_KEY: &str = "pultPath";

/// How long a `pick.source` shell-out gets before we give up on it. pult
/// itself doesn't bound this (it's talking to a real terminal), but a
/// resolve call blocks a form field, so it needs a ceiling.
const RESOLVE_TIMEOUT: Duration = Duration::from_secs(10);
/// Cap on stdout/stderr we'll buffer from a `pick.source` command — option
/// lists are short; this is only a backstop against a runaway/malicious
/// script, not a real limit anyone should hit.
const RESOLVE_MAX_OUTPUT_BYTES: usize = 256 * 1024;
/// Cap on the number of options we'll hand back to the UI.
const RESOLVE_MAX_OPTIONS: usize = 500;

/// Read the user-configured `pult` path override, if any, from the store.
pub fn stored_pult_path(app: &AppHandle) -> Option<String> {
    let store = app.store(SETTINGS_STORE).ok()?;
    store
        .get(PULT_PATH_KEY)
        .and_then(|v| v.as_str().map(|s| s.to_string()))
}

/// Save a user-configured `pult` path override.
pub fn set_stored_pult_path(app: &AppHandle, path: &str) -> Result<(), String> {
    let store = app
        .store(SETTINGS_STORE)
        .map_err(|e| format!("Couldn't open settings: {e}"))?;
    store.set(PULT_PATH_KEY, serde_json::json!(path));
    store
        .save()
        .map_err(|e| format!("Couldn't save settings: {e}"))
}

/// Resolve the `pult` binary to run, or the friendly error to show when it
/// can't be found — this exact string is what the "binary missing" empty
/// state in the UI displays.
///
/// Precedence: (1) the settings override above — an explicit user choice
/// always wins — (2) `which pult` on PATH, so a system install is still
/// preferred over the bundled copy, then (3) the bundled sidecar binary
/// (`resolve_pult_path` below carries the actual precedence logic, kept
/// AppHandle-free so it's unit testable). The sidecar's checksum is
/// verified once, at package time (`scripts/fetch-pult-sidecar.mjs` against
/// `src-tauri/sidecar.json`), not at runtime — by the time it's sitting
/// next to the app's executable inside a distributed bundle, re-checking it
/// on every launch would only catch tampering with the user's own install,
/// which isn't a threat model this app defends against.
pub fn resolve_pult(app: &AppHandle) -> Result<PathBuf, String> {
    resolve_pult_path(
        stored_pult_path(app),
        || which::which("pult").ok(),
        sidecar_candidate().unwrap_or_default(),
    )
}

/// The precedence logic behind [`resolve_pult`], factored out so it's
/// testable without an `AppHandle`: `configured` is the settings-store
/// override (if any), `which_pult` looks it up on PATH, and `sidecar` is
/// the candidate sidecar path (checked with `is_file`, so an empty/missing
/// path — e.g. when [`sidecar_candidate`] couldn't determine one — just
/// falls through to the final error rather than needing an `Option`).
fn resolve_pult_path(
    configured: Option<String>,
    which_pult: impl FnOnce() -> Option<PathBuf>,
    sidecar: PathBuf,
) -> Result<PathBuf, String> {
    if let Some(configured) = configured {
        let p = PathBuf::from(&configured);
        if p.is_file() {
            return Ok(p);
        }
        return Err(format!(
            "The configured pult path doesn't exist: {configured}. Fix it in Settings."
        ));
    }
    if let Some(p) = which_pult() {
        return Ok(p);
    }
    if sidecar.is_file() {
        return Ok(sidecar);
    }
    Err(
        "pult isn't installed, isn't on PATH, and no bundled copy was found. Install pult or set its path in Settings."
            .to_string(),
    )
}

/// The bundled sidecar's expected filename — Tauri's `externalBin` naming
/// convention strips the target-triple suffix and appends `.exe` on
/// Windows when it copies the resource next to the app's own executable.
fn sidecar_binary_name() -> &'static str {
    if cfg!(windows) {
        "pult.exe"
    } else {
        "pult"
    }
}

/// Where the sidecar binary would live inside `dir` — factored out from
/// [`sidecar_candidate`] so the naming convention is testable without
/// touching `current_exe`.
fn sidecar_path_in(dir: &std::path::Path) -> PathBuf {
    dir.join(sidecar_binary_name())
}

/// The bundled sidecar's candidate path: next to the running app's own
/// executable, which is where Tauri places every `externalBin` resource
/// for every bundle type we ship (`.app`/`.dmg`, NSIS, `.deb`, AppImage —
/// all place resources relative to the main executable's directory, not a
/// separate `Resources`-style folder). Returns `None` only if the OS can't
/// even tell us our own executable's path.
fn sidecar_candidate() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    Some(sidecar_path_in(dir))
}

/// Run `pult <args>` in `dir`, optionally feeding `stdin_data`, and capture
/// the whole output. Used for the non-streaming commands (listing, trust,
/// doctor, version) where we just need one parsed result.
pub async fn run_capture(
    bin: &PathBuf,
    dir: &str,
    args: &[&str],
    stdin_data: Option<&str>,
) -> Result<Output, String> {
    let mut cmd = Command::new(bin);
    cmd.args(args)
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Couldn't run pult: {e}"))?;

    if let Some(mut stdin) = child.stdin.take() {
        if let Some(data) = stdin_data {
            let _ = stdin.write_all(data.as_bytes()).await;
        }
        // Dropping `stdin` here closes the pipe so pult doesn't block
        // waiting for more input.
    }

    child
        .wait_with_output()
        .await
        .map_err(|e| format!("pult didn't exit cleanly: {e}"))
}

/// Trim and stringify a process's stderr for display.
pub fn stderr_text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).trim().to_string()
}

/// Resolve a `pick.source` param's live options.
///
/// pult 0.4 has no CLI surface for this: `--print` is explicitly
/// side-effect-free and does **not** run dynamic option sources, and there
/// is no resolve/complete subcommand (see docs/reference.md's `pult <id>
/// --print` note and the CLI summary — checked against the installed
/// `pult --version` / `pult --help` too). So — per the README's documented
/// plan — we replicate pult's documented `pick.from` semantics ourselves
/// rather than inventing our own: run the source string via `sh -c`, with
/// `{param}` strictly interpolated (only names in the param's declared
/// `depends_on`, values shell-quoted) and `{{`/`}}` escaping literal braces;
/// stdout lines (trimmed, non-empty) become options; a non-zero exit or an
/// empty result is an error.
///
/// Trust gate: a `pick.source` command is manifest-authored shell code, same
/// as a `check:` — `pult doctor` only runs those for a trusted manifest, so
/// we mirror that here. Unlike `run_command` (which hands off to `pult`
/// itself and lets *it* enforce the non-interactive-and-untrusted refusal),
/// this call never invokes `pult` to run anything, so there's no inner gate
/// to defer to — we re-check `trusted` ourselves via `pult --list --json`
/// rather than accepting a frontend-supplied flag, so a stale or spoofed
/// frontend state can't bypass it.
pub async fn resolve_pick_source(
    bin: &PathBuf,
    path: &str,
    command_id: &str,
    param_name: &str,
    values: &HashMap<String, String>,
) -> Result<Vec<String>, String> {
    let listing_output = run_capture(bin, path, &["--list", "--json"], None).await?;
    if !listing_output.status.success() {
        return Err(format!(
            "Couldn't read this repository's commands: {}",
            stderr_text(&listing_output)
        ));
    }
    let listing: Listing = serde_json::from_slice(&listing_output.stdout)
        .map_err(|e| format!("Couldn't read this repository's commands: {e}"))?;

    if !listing.trusted {
        return Err("Trust this repository before resolving live options.".to_string());
    }

    let command = listing
        .commands
        .iter()
        .find(|c| c.id == command_id)
        .ok_or_else(|| format!("Unknown command: {command_id}"))?;
    let param = command
        .params
        .iter()
        .find(|p| p.name == param_name)
        .ok_or_else(|| format!("Unknown param: {param_name}"))?;
    let source = param
        .source
        .as_deref()
        .ok_or_else(|| format!("{param_name} isn't a dynamic pick param"))?;

    let depends_on = param.depends_on.clone().unwrap_or_default();
    for dep in &depends_on {
        if !values.contains_key(dep) {
            return Err(format!("{param_name} depends on {dep}, which has no value yet"));
        }
    }

    let script = interpolate_source(source, &depends_on, values)?;
    // `run_dir` is where commands and option sources execute (differs from
    // `dir` only in user scope, per docs/reference.md's field notes); fall
    // back to the given path defensively if a future schema ever omits it.
    let cwd = if listing.run_dir.is_empty() { path } else { &listing.run_dir };

    run_pick_source(&script, cwd).await
}

/// Strict `{param}` interpolation for a `pick.source` string, matching
/// pult's documented semantics for `pick.from` / `run:` templates: only a
/// name declared in `depends_on` may be substituted (an unknown name is a
/// load error, mirroring pult's own "must be declared" rule), `{{` and `}}`
/// escape literal braces, and every substituted value is shell-quoted.
///
/// Assumption: the reference doc states the `{{`/`}}` escapes without
/// spelling out a full escaping state machine. We take the conservative
/// reading — a bare `}` that isn't closing a `{name}` placeholder and isn't
/// part of `}}` is passed through literally rather than rejected, since
/// pult itself already validated this string when the manifest loaded (a
/// `depends_on` we were handed only exists because pult's own parser
/// accepted the template), so by the time we see it here it's already
/// known-good; we're being lenient about characters we don't need to
/// reject, not re-validating pult's own grammar.
fn interpolate_source(
    template: &str,
    depends_on: &[String],
    values: &HashMap<String, String>,
) -> Result<String, String> {
    let chars: Vec<char> = template.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '{' if chars.get(i + 1) == Some(&'{') => {
                out.push('{');
                i += 2;
            }
            '{' => {
                let Some(rel) = chars[i + 1..].iter().position(|&c| c == '}') else {
                    return Err("This param's option source has an unterminated `{`".to_string());
                };
                let end = i + 1 + rel;
                let name: String = chars[i + 1..end].iter().collect();
                if !depends_on.iter().any(|d| d == &name) {
                    return Err(format!(
                        "This param's option source references {{{name}}}, which isn't in its depends_on list"
                    ));
                }
                let value = values.get(&name).cloned().unwrap_or_default();
                out.push_str(&shell_quote(&value));
                i = end + 1;
            }
            '}' if chars.get(i + 1) == Some(&'}') => {
                out.push('}');
                i += 2;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    Ok(out)
}

/// POSIX single-quote a value for `sh -c`, escaping embedded `'` as `'\''`.
/// Never logged — this is the one place a secret `depends_on` value could
/// end up in a string, and it only ever reaches the child process's argv.
fn shell_quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('\'');
    for c in value.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

/// Run an already-interpolated `pick.source` script via `sh -c` in `cwd`,
/// bounded by [`RESOLVE_TIMEOUT`] and [`RESOLVE_MAX_OUTPUT_BYTES`]. Never
/// panics: every failure mode (spawn error, timeout, non-zero exit, empty
/// output) becomes a typed `Err` string for the UI.
async fn run_pick_source(script: &str, cwd: &str) -> Result<Vec<String>, String> {
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(script)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Couldn't run this param's option source: {e}"))?;

    let mut stdout = child.stdout.take().expect("stdout was piped");
    let mut stderr = child.stderr.take().expect("stderr was piped");

    let outcome = timeout(RESOLVE_TIMEOUT, async {
        let mut out_buf = Vec::new();
        let mut err_buf = Vec::new();
        let mut capped_stdout = (&mut stdout).take(RESOLVE_MAX_OUTPUT_BYTES as u64);
        let mut capped_stderr = (&mut stderr).take(RESOLVE_MAX_OUTPUT_BYTES as u64);
        let _ = tokio::join!(
            capped_stdout.read_to_end(&mut out_buf),
            capped_stderr.read_to_end(&mut err_buf),
        );
        let status = child.wait().await;
        (status, out_buf, err_buf)
    })
    .await;

    let (status, stdout_bytes, stderr_bytes) = match outcome {
        Ok(v) => v,
        Err(_) => {
            let _ = child.start_kill();
            return Err(
                "Timed out resolving options (10s) — the repository's option source didn't finish"
                    .to_string(),
            );
        }
    };

    let status = status.map_err(|e| format!("Couldn't run this param's option source: {e}"))?;

    if !status.success() {
        let stderr = String::from_utf8_lossy(&stderr_bytes).trim().to_string();
        return Err(if stderr.is_empty() {
            "This param's option source exited with an error".to_string()
        } else {
            format!("This param's option source failed: {stderr}")
        });
    }

    let options: Vec<String> = String::from_utf8_lossy(&stdout_bytes)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .take(RESOLVE_MAX_OPTIONS)
        .collect();

    if options.is_empty() {
        return Err("This param's option source returned no options".to_string());
    }

    Ok(options)
}

/// How long a stopped run gets after `SIGTERM` before this app escalates to
/// `SIGKILL` (unix only — see [`run_streaming`]). pult itself has no opinion
/// on this; it's purely this app's own grace period for `stop_run`.
const STOP_GRACE: Duration = Duration::from_secs(3);

/// Registry of in-flight runs, shared as Tauri managed state (see
/// `src-tauri/src/lib.rs`). Maps `run_id` to a one-shot "please stop" signal
/// for the task that owns that run's child process — [`RunRegistry::request_stop`]
/// (what the `stop_run` Tauri command calls) is the only thing that ever
/// sends on it; [`run_streaming`] is the only thing that ever receives. An
/// entry is removed the moment its run ends, whether it exited on its own or
/// was stopped, so a `stop_run` call for a `run_id` that's already finished
/// (or never existed) is a clean, reportable error rather than a silent
/// no-op.
#[derive(Clone, Default)]
pub struct RunRegistry(Arc<Mutex<HashMap<String, RunHandle>>>);

struct RunHandle {
    stop_tx: oneshot::Sender<()>,
    /// The child's pid — kept for introspection/tests ([`RunRegistry::pid_of`]).
    /// The actual stop happens inside [`run_streaming`], which already has
    /// direct access to the `Child`; nothing here needs the pid back to act
    /// on it.
    pid: Option<u32>,
}

impl RunRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    fn register(&self, run_id: String, pid: Option<u32>) -> oneshot::Receiver<()> {
        let (tx, rx) = oneshot::channel();
        self.0.lock().unwrap().insert(run_id, RunHandle { stop_tx: tx, pid });
        rx
    }

    fn unregister(&self, run_id: &str) {
        self.0.lock().unwrap().remove(run_id);
    }

    /// The pid of a currently-registered run, if any. Only meaningful for
    /// introspection/tests — `run_streaming` never needs this back from the
    /// registry, it already holds the `Child` it came from.
    pub fn pid_of(&self, run_id: &str) -> Option<u32> {
        self.0.lock().unwrap().get(run_id).and_then(|h| h.pid)
    }

    /// Signal a running command to stop. Errs with a user-facing sentence if
    /// `run_id` isn't currently running — the frontend shows this verbatim.
    pub fn request_stop(&self, run_id: &str) -> Result<(), String> {
        let handle = self
            .0
            .lock()
            .unwrap()
            .remove(run_id)
            .ok_or_else(|| "That run has already finished.".to_string())?;
        // If the receiving end already dropped (the run finished in a race
        // with this call), there's nothing left to stop — not worth
        // surfacing as a distinct error, since "already finished" describes
        // exactly that outcome too.
        let _ = handle.stop_tx.send(());
        Ok(())
    }
}

/// Spawn `pult <id> --params-json`, streaming stdout/stderr — and, on unix,
/// `PULT_EVENTS` machine events — to `emit` as they arrive, then a final
/// `Exit` event once the child exits (naturally, or because `stop_run` /
/// [`RunRegistry::request_stop`] stopped it). AppHandle-free like
/// [`run_capture`]/[`resolve_pick_source`] above, so it's unit-testable
/// without a Tauri window (see this crate's `tests/pult_backend.rs`);
/// `commands::run_command` is the thin `#[tauri::command]` wrapper that
/// resolves the binary and turns `emit` into
/// `app.emit("pult://run-output", …)`.
///
/// ## The `PULT_EVENTS` channel (unix only)
///
/// pult's documented passthrough rule (the pult repo's `docs/reference.md`,
/// "Events protocol — `PULT_EVENTS`"): "if `PULT_EVENTS` is already set in
/// pult's own environment when it runs a command, pult does nothing — no
/// pipe, no translation. The var and its fd inherit through to the child as-
/// is." So this app claims the channel itself, the same "replicate pult's
/// documented behavior rather than invent new ones" approach
/// [`resolve_pick_source`] already takes for `pick.source`: create an OS
/// pipe, hand the write end to the spawned `pult` process as fd 3 (the fd
/// number pult's own internal channel uses today, per the doc's "always
/// read the number from `$PULT_EVENTS`" note — any number would work, 3
/// just avoids surprising anyone who greps for it), set `PULT_EVENTS=3` in
/// its environment, and read lines off the read end concurrently with
/// stdout/stderr, parsing each with [`crate::events::parse`]
/// (malformed/unknown lines are silently ignored, matching the protocol's
/// own stated leniency — never an error, never a crashed run).
///
/// The write end is `dup2`'d into the child during `pre_exec` (async-signal-
/// safe; runs after `fork`, before `exec`, in the child's own copy of the fd
/// table) rather than passed via `Stdio`, since neither `std` nor `tokio`
/// have a first-class "extra inherited fd" API — this mirrors pult's own
/// `src/runner.rs::run_with_own_channel`, right down to the fd-3-already-
/// free edge case (`std::io::pipe()` creates both ends close-on-exec; if the
/// write end happens to already land on fd 3, `dup2(3, 3)` is a defined
/// no-op that does *not* clear that flag, so it's cleared directly via
/// `fcntl` instead — see `unix_run::wire_up`). This process's own copy of
/// the write end is dropped in the parent right after spawning — otherwise
/// it would keep the pipe open forever from this end too, and the reader
/// would never see EOF — so EOF (and the reader task's natural exit)
/// arrives exactly when `pult` and everything it spawned have closed their
/// copies, i.e. exactly when the run is done producing events.
///
/// Windows has no equivalent of `pre_exec`/arbitrary-fd-inheritance in
/// `std::process`, so there this never sets `PULT_EVENTS` and never creates
/// a pipe: no step/progress/status events flow, but stdout/stderr streaming
/// and stopping a run both work the same as on unix.
///
/// ## Stopping a run
///
/// The spawned `pult` process is placed in its own process group (unix
/// only, `process_group(0)`) so a stop kills the whole tree it started —
/// pult's own `sh -c "<script>"` child (and anything *that* forks) inherits
/// the group unless something explicitly calls `setpgid`, which nothing
/// here does. `registry` hands back a one-shot receiver this function races
/// against `child.wait()`; once `stop_run` fires it, this sends `SIGTERM` to
/// the group, waits up to [`STOP_GRACE`] for a clean exit, and escalates to
/// `SIGKILL` if the group is still alive. Windows has no signal-a-group
/// primitive in `std`, so there `child.start_kill()` (a `TerminateProcess`
/// call, and per the task this app targets, sufficient on its own) is used
/// directly instead, with no group/escalation concept to add.
pub async fn run_streaming(
    bin: &PathBuf,
    dir: &str,
    id: &str,
    values: &HashMap<String, String>,
    run_id: String,
    registry: &RunRegistry,
    emit: impl Fn(RunEvent) + Send + Sync + 'static,
) -> Result<(), String> {
    let emit: Arc<dyn Fn(RunEvent) + Send + Sync> = Arc::new(emit);
    let payload = serde_json::to_string(values).map_err(|e| e.to_string())?;

    let mut cmd = Command::new(bin);
    cmd.arg(id)
        .arg("--params-json")
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(unix)]
    cmd.process_group(0);

    #[cfg(unix)]
    let events_pipe = unix_run::wire_up(&mut cmd)?;

    let mut child = cmd.spawn().map_err(|e| format!("Couldn't start pult: {e}"))?;

    // Drop this process's own copy of the events pipe's write end right
    // after spawning (see this function's doc comment for why): the child
    // now holds the only other copy (dup2'd in pre_exec), so once it — and
    // anything it spawned that inherited fd 3 — closes its copy, the reader
    // below sees EOF.
    #[cfg(unix)]
    let events_reader = {
        let (reader, writer) = events_pipe;
        drop(writer);
        reader
    };

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(payload.as_bytes()).await;
        // dropped here: closes stdin so pult doesn't wait for more input
    }

    let stdout = child.stdout.take().expect("stdout was piped");
    let stderr = child.stderr.take().expect("stderr was piped");

    let out_emit = Arc::clone(&emit);
    let out_run_id = run_id.clone();
    let out_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(text)) = lines.next_line().await {
            out_emit(RunEvent::Line {
                run_id: out_run_id.clone(),
                stream: "stdout".to_string(),
                text,
            });
        }
    });

    let err_emit = Arc::clone(&emit);
    let err_run_id = run_id.clone();
    let err_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(text)) = lines.next_line().await {
            err_emit(RunEvent::Line {
                run_id: err_run_id.clone(),
                stream: "stderr".to_string(),
                text,
            });
        }
    });

    // Read+parse the events pipe on a blocking task (std::io::PipeReader has
    // no async counterpart) concurrently with the stdout/stderr streams
    // above — same "one task per stream" shape, just spawn_blocking instead
    // of spawn since this reader is synchronous.
    #[cfg(unix)]
    let events_task = {
        let ev_emit = Arc::clone(&emit);
        let ev_run_id = run_id.clone();
        tokio::task::spawn_blocking(move || unix_run::read_events(events_reader, ev_run_id, ev_emit))
    };

    let pid = child.id();
    let stop_rx = registry.register(run_id.clone(), pid);

    let (status, stopped) = wait_or_stop(&mut child, pid, stop_rx).await?;
    registry.unregister(&run_id);

    let _ = out_task.await;
    let _ = err_task.await;
    #[cfg(unix)]
    let _ = events_task.await;

    emit(RunEvent::Exit {
        run_id,
        code: status.code(),
        stopped,
    });

    Ok(())
}

/// Race `child.wait()` against a stop request, escalating `SIGTERM` →
/// `SIGKILL` (unix) or re-issuing `start_kill` (elsewhere) once
/// [`STOP_GRACE`] passes without the child exiting. Returns the exit status
/// and whether this was a stop rather than a natural exit — see
/// [`run_streaming`]'s doc comment.
async fn wait_or_stop(
    child: &mut Child,
    pid: Option<u32>,
    stop_rx: oneshot::Receiver<()>,
) -> Result<(std::process::ExitStatus, bool), String> {
    tokio::select! {
        status = child.wait() => {
            Ok((status.map_err(|e| format!("pult didn't exit cleanly: {e}"))?, false))
        }
        _ = stop_rx => {
            request_termination(child, pid);
            let status = tokio::select! {
                status = child.wait() => Some(status),
                _ = tokio::time::sleep(STOP_GRACE) => None,
            };
            let status = match status {
                Some(status) => status,
                None => {
                    escalate_termination(child, pid);
                    child.wait().await
                }
            };
            Ok((status.map_err(|e| format!("pult didn't exit cleanly: {e}"))?, true))
        }
    }
}

#[cfg(unix)]
fn request_termination(_child: &mut Child, pid: Option<u32>) {
    if let Some(pid) = pid {
        unix_run::signal_group(pid, unix_run::SIGTERM);
    }
}
#[cfg(not(unix))]
fn request_termination(child: &mut Child, _pid: Option<u32>) {
    // No group/signal concept in std on Windows — TerminateProcess via
    // start_kill is the whole story here (per this app's stop_run spec:
    // "Windows: child.kill() is fine").
    let _ = child.start_kill();
}

#[cfg(unix)]
fn escalate_termination(_child: &mut Child, pid: Option<u32>) {
    if let Some(pid) = pid {
        unix_run::signal_group(pid, unix_run::SIGKILL);
    }
}
#[cfg(not(unix))]
fn escalate_termination(child: &mut Child, _pid: Option<u32>) {
    // Best-effort retry in case the first start_kill raced the process not
    // having fully started yet (tokio documents that as possible); there's
    // no stronger "are you still there" primitive to check against first.
    let _ = child.start_kill();
}

/// Unix-only machinery for [`run_streaming`]: wiring up the `PULT_EVENTS`
/// pipe and signaling a process group. See that function's doc comment for
/// the full rationale; this mirrors pult's own `src/runner.rs`'s
/// `run_with_own_channel` (fd-passing) and adds process-group signaling
/// (pult itself never needs to kill anything).
#[cfg(unix)]
mod unix_run {
    use std::ffi::c_int;
    use std::io::{BufRead, BufReader};
    use std::os::fd::AsRawFd;
    use std::sync::Arc;

    use tokio::process::Command;

    use crate::events;
    use crate::types::RunEvent;

    // Rust 2024 requires FFI declarations inside `unsafe extern` blocks; no
    // new dependency needed; the binary already links the system libc on
    // unix (pult's own `src/runner.rs` takes the same approach, for the
    // same reason: this is four constants and three functions, not enough
    // to justify pulling in the `libc` crate just for these).
    unsafe extern "C" {
        fn dup2(oldfd: c_int, newfd: c_int) -> c_int;
        fn fcntl(fd: c_int, cmd: c_int, ...) -> c_int;
        fn kill(pid: c_int, sig: c_int) -> c_int;
    }

    const F_GETFD: c_int = 1;
    const F_SETFD: c_int = 2;
    const FD_CLOEXEC: c_int = 1;
    pub(super) const SIGTERM: c_int = 15;
    pub(super) const SIGKILL: c_int = 9;

    /// The fd `pult` documents its own `PULT_EVENTS` channel using today
    /// (docs/reference.md's "Fd 3 conflicts" section) — any number would
    /// work here since we set `PULT_EVENTS` to match whatever we pick, this
    /// just avoids surprising anyone who greps for `3`.
    const EVENTS_FD: c_int = 3;

    /// Create the events pipe and wire its write end into `cmd`'s future
    /// child at fd [`EVENTS_FD`], setting `PULT_EVENTS` to match. Returns
    /// `(reader, writer)`: keep the reader, drop the writer in the parent
    /// right after `spawn()` (see `run_streaming`'s doc comment for why).
    pub(super) fn wire_up(
        cmd: &mut Command,
    ) -> Result<(std::io::PipeReader, std::io::PipeWriter), String> {
        let (reader, writer) =
            std::io::pipe().map_err(|e| format!("Couldn't create the events pipe: {e}"))?;
        let write_fd = writer.as_raw_fd();

        // SAFETY: this closure runs in the child after `fork`, before
        // `exec`, operating only on that child's own copy of the fd table —
        // dup2/fcntl are async-signal-safe, so this is sound to run between
        // fork and exec. `write_fd` stays valid for the closure's lifetime:
        // `writer` (and the fd it wraps) isn't dropped until after `spawn()`
        // returns, by which point `fork` has already copied the fd table.
        unsafe {
            cmd.pre_exec(move || {
                if write_fd == EVENTS_FD {
                    // `std::io::pipe()` creates both ends close-on-exec.
                    // `dup2(3, 3)` is a defined no-op on Linux/macOS when the
                    // write end already happens to be fd 3 — it does NOT
                    // clear FD_CLOEXEC, so without this branch fd 3 would
                    // close right at exec and the child's events writes
                    // would silently fail. Clear the flag directly instead.
                    let flags = fcntl(write_fd, F_GETFD);
                    if flags == -1 || fcntl(write_fd, F_SETFD, flags & !FD_CLOEXEC) == -1 {
                        return Err(std::io::Error::last_os_error());
                    }
                } else if dup2(write_fd, EVENTS_FD) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        cmd.env("PULT_EVENTS", EVENTS_FD.to_string());

        Ok((reader, writer))
    }

    /// Blocking read loop for the events pipe — run via `spawn_blocking`
    /// since `std::io::PipeReader` has no async counterpart. Parses each
    /// line with [`events::parse`] and calls `emit` for anything that
    /// parses; malformed/unknown lines are silently dropped, matching the
    /// protocol's own leniency (see `events`'s module doc). Returns once the
    /// write end is closed everywhere — `pult` exits and every process it
    /// spawned that inherited fd 3 has too.
    pub(super) fn read_events(
        reader: std::io::PipeReader,
        run_id: String,
        emit: Arc<dyn Fn(RunEvent) + Send + Sync>,
    ) {
        for line in BufReader::new(reader).lines() {
            let Ok(line) = line else { break };
            if let Some(event) = events::parse(&line) {
                emit(to_run_event(&run_id, event));
            }
        }
    }

    fn to_run_event(run_id: &str, event: events::PultEvent) -> RunEvent {
        match event {
            events::PultEvent::Progress { pct, text } => {
                RunEvent::Progress { run_id: run_id.to_string(), pct, text }
            }
            events::PultEvent::Status(text) => RunEvent::Status { run_id: run_id.to_string(), text },
            events::PultEvent::Step { k, n, name } => {
                RunEvent::Step { run_id: run_id.to_string(), k, n, name }
            }
        }
    }

    /// Send `sig` to the whole process group `pult` was placed in
    /// (`process_group(0)` at spawn time makes its pid the pgid too) — a
    /// negative pid targets the group rather than just the leader, so `sh
    /// -c` children (and anything *they* fork) are signaled as well.
    /// Best-effort: if the group is already gone, `kill` returns `ESRCH`,
    /// which is ignored — nothing left to signal is exactly the state a
    /// stop wants to reach.
    pub(super) fn signal_group(pid: u32, sig: c_int) {
        unsafe {
            kill(-(pid as c_int), sig);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidecar_path_in_uses_the_platform_binary_name() {
        let dir = std::path::Path::new("/some/app/dir");
        let candidate = sidecar_path_in(dir);
        assert_eq!(candidate.parent(), Some(dir));
        assert_eq!(candidate.file_name().unwrap(), sidecar_binary_name());
        #[cfg(windows)]
        assert_eq!(candidate.file_name().unwrap(), "pult.exe");
        #[cfg(not(windows))]
        assert_eq!(candidate.file_name().unwrap(), "pult");
    }

    #[test]
    fn resolve_pult_path_prefers_a_valid_configured_override() {
        let dir = tempfile::tempdir().unwrap();
        let configured = dir.path().join("my-pult");
        std::fs::write(&configured, b"").unwrap();

        let result = resolve_pult_path(
            Some(configured.to_string_lossy().to_string()),
            || panic!("PATH shouldn't be consulted when an override is configured"),
            dir.path().join("sidecar-should-not-be-used"),
        );
        assert_eq!(result.unwrap(), configured);
    }

    #[test]
    fn resolve_pult_path_errors_on_a_configured_override_that_does_not_exist() {
        let result = resolve_pult_path(
            Some("/nonexistent/pult".to_string()),
            || panic!("PATH shouldn't be consulted when an override is configured"),
            PathBuf::new(),
        );
        let err = result.unwrap_err();
        assert!(err.contains("/nonexistent/pult"), "error was: {err}");
    }

    #[test]
    fn resolve_pult_path_prefers_path_over_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let on_path = dir.path().join("path-pult");
        let sidecar = dir.path().join("sidecar-pult");
        std::fs::write(&on_path, b"").unwrap();
        std::fs::write(&sidecar, b"").unwrap();

        let result = resolve_pult_path(None, || Some(on_path.clone()), sidecar);
        assert_eq!(result.unwrap(), on_path);
    }

    #[test]
    fn resolve_pult_path_falls_back_to_the_sidecar_when_present() {
        let dir = tempfile::tempdir().unwrap();
        let sidecar = dir.path().join(sidecar_binary_name());
        std::fs::write(&sidecar, b"").unwrap();

        let result = resolve_pult_path(None, || None, sidecar.clone());
        assert_eq!(result.unwrap(), sidecar);
    }

    #[test]
    fn resolve_pult_path_errors_when_nothing_resolves() {
        let dir = tempfile::tempdir().unwrap();
        let missing_sidecar = dir.path().join("not-actually-there");

        let err = resolve_pult_path(None, || None, missing_sidecar).unwrap_err();
        assert!(err.contains("Install pult"), "error was: {err}");
    }

    #[test]
    fn interpolate_source_substitutes_depends_on_values_shell_quoted() {
        let depends_on = vec!["region".to_string()];
        let mut values = HashMap::new();
        values.insert("region".to_string(), "eu-west-1".to_string());

        let script =
            interpolate_source("echo target-{region}-a; echo target-{region}-b", &depends_on, &values)
                .unwrap();
        assert_eq!(script, "echo target-'eu-west-1'-a; echo target-'eu-west-1'-b");
    }

    #[test]
    fn interpolate_source_shell_quotes_embedded_single_quotes() {
        let depends_on = vec!["name".to_string()];
        let mut values = HashMap::new();
        values.insert("name".to_string(), "o'brien".to_string());

        let script = interpolate_source("echo {name}", &depends_on, &values).unwrap();
        assert_eq!(script, "echo 'o'\\''brien'");
    }

    #[test]
    fn interpolate_source_escapes_doubled_braces() {
        let depends_on: Vec<String> = vec![];
        let values = HashMap::new();

        let script = interpolate_source("echo {{literal}}", &depends_on, &values).unwrap();
        assert_eq!(script, "echo {literal}");
    }

    #[test]
    fn interpolate_source_rejects_a_name_outside_depends_on() {
        let depends_on: Vec<String> = vec![];
        let values = HashMap::new();

        let err = interpolate_source("echo {region}", &depends_on, &values).unwrap_err();
        assert!(err.contains("region"), "error should name the offending param: {err}");
    }

    #[test]
    fn interpolate_source_rejects_unterminated_placeholder() {
        let depends_on: Vec<String> = vec![];
        let values = HashMap::new();

        let err = interpolate_source("echo {region", &depends_on, &values).unwrap_err();
        assert!(err.contains("unterminated"), "error was: {err}");
    }
}
