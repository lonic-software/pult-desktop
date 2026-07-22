mod commands;
pub mod events;
pub mod journal;
pub mod pult_bin;
pub mod types;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // GUI launches (Finder/dock/desktop-entry, not a terminal) inherit
    // launchd's/the desktop session's minimal PATH — missing Homebrew and
    // toolchain dirs — which breaks every shell-out this app or `pult`
    // makes (checks, run commands, pick sources). Repair PATH from the
    // user's login shell before anything spawns. A broken/exotic login
    // shell must not brick the GUI, so failure is logged and ignored, not
    // propagated; a terminal launch already has a correct PATH, so this is
    // a no-op there.
    if let Err(e) = fix_path_env::fix() {
        eprintln!("Couldn't repair PATH from the login shell: {e}");
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        // Run ids currently being tailed, so hydration, a fresh spawn and a
        // poll can never double-tail the same run — see
        // `journal::TailRegistry`.
        .manage(journal::TailRegistry::new())
        // Pre-journal spawn-failure diagnostics (fix round 2's point fix
        // B), fed by `pult_bin::spawn_run`'s reaper and probed by
        // `journal::wait_for_run_dir_at` — see `journal::SpawnOutcomes`.
        .manage(journal::SpawnOutcomes::new())
        .invoke_handler(tauri::generate_handler![
            commands::open_repo,
            commands::trust_repo,
            commands::doctor,
            commands::pult_version,
            commands::get_pult_path,
            commands::set_pult_path,
            commands::run_command,
            commands::stop_run,
            commands::resolve_pick_source,
            commands::list_runs,
            commands::tail_run,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
