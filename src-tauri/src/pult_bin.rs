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
use std::time::Duration;

use tauri::AppHandle;
use tauri_plugin_store::StoreExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::time::timeout;

use crate::types::Listing;

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
///
/// Deliberately does *not* scrub an inherited `PULT_EVENTS` (contrast
/// `spawn_run`, which must): none of `run_capture`'s callers ever ask pult
/// to execute a manifest command — `--list`/`--trust`/`doctor`/`--version`
/// never reach the tui repo's `exec.rs`/`runner.rs` (the only place a
/// spawned pult reads `$PULT_EVENTS`), `doctor`'s `check:` scripts run via a
/// plain unguarded `sh` with no events machinery at all. An inherited value
/// here is simply never consulted, so there is nothing to starve.
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

    let mut child = cmd.spawn().map_err(|e| format!("Couldn't run pult: {e}"))?;

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
            return Err(format!(
                "{param_name} depends on {dep}, which has no value yet"
            ));
        }
    }

    let script = interpolate_source(source, &depends_on, values)?;
    // `run_dir` is where commands and option sources execute (differs from
    // `dir` only in user scope, per docs/reference.md's field notes); fall
    // back to the given path defensively if a future schema ever omits it.
    let cwd = if listing.run_dir.is_empty() {
        path
    } else {
        &listing.run_dir
    };

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
///
/// Also deliberately does not scrub `PULT_EVENTS`, same reasoning as
/// `run_capture` above but one step further removed: this spawns `sh`, not
/// `pult`, at all — there's no pult process here to starve of its channel
/// in the first place. (A manifest `pick.source` script that itself shells
/// out to `pult` would inherit whatever this process's environment has,
/// same as any other env var reaching manifest-authored code — no different
/// a case than a `check:` script, out of scope here.)
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

/// Spawn `pult <id> --params-json --run-id <run_id>` **detached**: pult owns
/// its own journal and its own `PULT_EVENTS` channel now (see
/// `docs/run-journal.md`), so this app no longer pipes stdout/stderr, no
/// longer claims `PULT_EVENTS`, and no longer tracks the child to stop it —
/// a repo-scoped `crate::journal::tail_run` reads everything back from the
/// run's journal instead, and `crate::journal::stop_run` signals the
/// journaled `pgid` directly. Values are still fed as JSON on stdin exactly
/// as before (keeps secrets out of argv/shell history); stdout/stderr are
/// `Stdio::null()` since nothing here reads them.
///
/// `PULT_ORIGIN=desktop` is set so the journal's `meta.json.origin` records
/// who spawned the run. The child is placed in its own process group (unix
/// only, `process_group(0)`) — matching `meta.json`'s `pgid`, the stop
/// capability any reader (including this app's own `stop_run`) signals.
///
/// A background task reaps the child (`child.wait()`) so it never becomes a
/// zombie while this app is alive; if the app quits first the run simply
/// keeps going, journaled — the desired quit behavior per the run-journal
/// spec (a run's lifetime is decoupled from any viewer's).
///
/// This only reports a *spawn-level* failure (binary missing, exec failed).
/// Whether pult then actually journals the run (a too-old binary might not
/// understand `--run-id` at all, or there's a startup race before the run
/// dir exists) is something only the journal tail can observe — see
/// `crate::journal::tail_run`'s bounded wait for the run dir to appear.
pub async fn spawn_run(
    bin: &PathBuf,
    dir: &str,
    id: &str,
    values: &HashMap<String, String>,
    run_id: &str,
) -> Result<(), String> {
    let payload = serde_json::to_string(values).map_err(|e| e.to_string())?;

    let mut cmd = Command::new(bin);
    cmd.arg(id)
        .arg("--params-json")
        .arg("--run-id")
        .arg(run_id)
        .current_dir(dir)
        .env("PULT_ORIGIN", "desktop")
        // Scrub any inherited `PULT_EVENTS`: this app is very often itself
        // launched from inside a running `pult` command (the exact `pult
        // dev` workflow docs/run-journal.md is written for), which sets
        // `PULT_EVENTS=<fd>` in *our* environment and, since pult clears
        // CLOEXEC on that fd (tui repo's runner.rs `wire_up_own_fd`), leaves
        // both the fd and the var alive down the whole process tree. Left
        // alone, the spawned pult would see a channel already claimed and
        // honor the documented passthrough rule (docs/run-journal.md,
        // "Interaction with PULT_EVENTS") — creating no pipe of its own and
        // journaling no step/progress/status for this run at all, silently,
        // for every run this app ever spawns from within one. Removing the
        // var here is what lets the spawned pult claim its own channel and
        // journal normally, exactly as if launched fresh from a shell.
        .env_remove("PULT_EVENTS")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(unix)]
    cmd.process_group(0);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Couldn't start pult: {e}"))?;

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(payload.as_bytes()).await;
        // dropped here: closes stdin so pult doesn't wait for more input
    }

    // Reap in the background rather than awaiting here: `run_command`
    // returns as soon as the tail has started, not when the run finishes.
    tokio::spawn(async move {
        let _ = child.wait().await;
    });

    Ok(())
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

        let script = interpolate_source(
            "echo target-{region}-a; echo target-{region}-b",
            &depends_on,
            &values,
        )
        .unwrap();
        assert_eq!(
            script,
            "echo target-'eu-west-1'-a; echo target-'eu-west-1'-b"
        );
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
        assert!(
            err.contains("region"),
            "error should name the offending param: {err}"
        );
    }

    #[test]
    fn interpolate_source_rejects_unterminated_placeholder() {
        let depends_on: Vec<String> = vec![];
        let values = HashMap::new();

        let err = interpolate_source("echo {region", &depends_on, &values).unwrap_err();
        assert!(err.contains("unterminated"), "error was: {err}");
    }
}
