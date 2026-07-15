//! The Tauri commands the frontend calls. Thin wrappers over `pult_bin`:
//! resolve the binary, run it, parse or translate its output. Every error
//! is a plain string the UI shows verbatim (operator-side copy, not
//! internals) per the product spec.

use std::collections::HashMap;

use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

use crate::pult_bin::{resolve_pult, run_capture, set_stored_pult_path, stderr_text, stored_pult_path};
use crate::types::{DoctorReport, Listing, RunEvent};

/// Open a repository: run `pult --list --json` there and parse the listing.
#[tauri::command]
pub async fn open_repo(app: AppHandle, path: String) -> Result<Listing, String> {
    let bin = resolve_pult(&app)?;
    let output = run_capture(&bin, &path, &["--list", "--json"], None).await?;

    if output.status.success() {
        serde_json::from_slice::<Listing>(&output.stdout)
            .map_err(|e| format!("Couldn't read this repository's commands: {e}"))
    } else {
        let stderr = stderr_text(&output);
        if stderr.contains("no pult.yaml found") {
            Err("No pult.yaml here — point me at a repository that has one".to_string())
        } else {
            Err(format!("Couldn't read this repository's commands: {stderr}"))
        }
    }
}

/// Trust a manifest at its current resolved hash: `pult --trust --list`.
/// The frontend re-calls `open_repo` afterward to reload with `trusted: true`.
#[tauri::command]
pub async fn trust_repo(app: AppHandle, path: String) -> Result<(), String> {
    let bin = resolve_pult(&app)?;
    let output = run_capture(&bin, &path, &["--trust", "--list"], None).await?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "Couldn't trust this repository: {}",
            stderr_text(&output)
        ))
    }
}

/// Run every command's `check:` and report readiness: `pult doctor --json`.
/// Trust-gated by pult itself — only call this once `open_repo` reports
/// `trusted: true`. Note doctor's own process exit code is 1 whenever any
/// check failed; that's expected and not an error, so we parse stdout first
/// and only fall back to an error if it isn't valid JSON.
#[tauri::command]
pub async fn doctor(app: AppHandle, path: String) -> Result<DoctorReport, String> {
    let bin = resolve_pult(&app)?;
    let output = run_capture(&bin, &path, &["doctor", "--json"], None).await?;

    if let Ok(report) = serde_json::from_slice::<DoctorReport>(&output.stdout) {
        return Ok(report);
    }
    Err(format!(
        "Couldn't check readiness: {}",
        stderr_text(&output)
    ))
}

/// `pult --version`.
#[tauri::command]
pub async fn pult_version(app: AppHandle) -> Result<String, String> {
    let bin = resolve_pult(&app)?;
    let dir = std::env::temp_dir();
    let output = run_capture(&bin, &dir.to_string_lossy(), &["--version"], None).await?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(format!(
            "Couldn't get pult's version: {}",
            stderr_text(&output)
        ))
    }
}

/// The currently configured `pult` path override, if any (not the resolved
/// binary — this is only what the user explicitly set in Settings).
#[tauri::command]
pub fn get_pult_path(app: AppHandle) -> Option<String> {
    stored_pult_path(&app)
}

/// Save a `pult` path override.
#[tauri::command]
pub fn set_pult_path(app: AppHandle, path: String) -> Result<(), String> {
    set_stored_pult_path(&app, &path)
}

/// Run a command: `pult <id> --params-json`, values fed on stdin (keeps
/// secrets out of argv, which is exactly why the flag exists). Streams
/// stdout/stderr lines to the frontend as they arrive via the
/// `pult://run-output` event, then a final `Exit` event with the code.
#[tauri::command]
pub async fn run_command(
    app: AppHandle,
    path: String,
    id: String,
    values: HashMap<String, String>,
) -> Result<(), String> {
    let bin = resolve_pult(&app)?;
    let payload = serde_json::to_string(&values).map_err(|e| e.to_string())?;

    let mut cmd = Command::new(&bin);
    cmd.arg(&id)
        .arg("--params-json")
        .current_dir(&path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Couldn't start pult: {e}"))?;

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(payload.as_bytes()).await;
        // dropped here: closes stdin so pult doesn't wait for more input
    }

    let stdout = child.stdout.take().expect("stdout was piped");
    let stderr = child.stderr.take().expect("stderr was piped");

    let out_app = app.clone();
    let out_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(text)) = lines.next_line().await {
            let _ = out_app.emit(
                "pult://run-output",
                RunEvent::Line {
                    stream: "stdout".to_string(),
                    text,
                },
            );
        }
    });

    let err_app = app.clone();
    let err_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(text)) = lines.next_line().await {
            let _ = err_app.emit(
                "pult://run-output",
                RunEvent::Line {
                    stream: "stderr".to_string(),
                    text,
                },
            );
        }
    });

    let status = child
        .wait()
        .await
        .map_err(|e| format!("pult didn't exit cleanly: {e}"))?;
    let _ = out_task.await;
    let _ = err_task.await;

    let _ = app.emit(
        "pult://run-output",
        RunEvent::Exit {
            code: status.code(),
        },
    );

    Ok(())
}
