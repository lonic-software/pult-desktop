//! Locating and invoking the `pult` binary.
//!
//! v0 resolves `pult` from (in order): a path saved in the settings store,
//! then `which pult` on the user's PATH. Sidecar bundling — shipping a
//! checksummed release binary alongside the app so it works with no
//! separate install — is deliberately not wired up in v0; see the README's
//! "Sidecar bundling" section for the plan.

use std::path::PathBuf;
use std::process::{Output, Stdio};

use tauri::AppHandle;
use tauri_plugin_store::StoreExt;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

const SETTINGS_STORE: &str = "settings.json";
const PULT_PATH_KEY: &str = "pultPath";

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
/// Sidecar bundling plan (not implemented yet): once wired up, this
/// function's precedence would become (1) the settings override above,
/// unchanged — an explicit user choice always wins — (2) `which pult` on
/// PATH, so a system install is still preferred over a bundled copy, then
/// (3) the bundled sidecar binary as the final fallback, so the app works
/// out of the box with no separate install. The sidecar itself would be:
/// a checksummed release binary fetched per-target-triple at package time
/// (`src-tauri/tauri.conf.json`'s `bundle.externalBin`, resolved at runtime
/// via `tauri_plugin_shell`'s sidecar API or a bundled-resource path), with
/// the checksum verified against pult's published release manifest before
/// first use. See the README's "Sidecar bundling" section.
pub fn resolve_pult(app: &AppHandle) -> Result<PathBuf, String> {
    if let Some(configured) = stored_pult_path(app) {
        let p = PathBuf::from(&configured);
        if p.is_file() {
            return Ok(p);
        }
        return Err(format!(
            "The configured pult path doesn't exist: {configured}. Fix it in Settings."
        ));
    }
    which::which("pult").map_err(|_| "Install pult or set its path in Settings".to_string())
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
