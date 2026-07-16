mod commands;
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
        .invoke_handler(tauri::generate_handler![
            commands::open_repo,
            commands::trust_repo,
            commands::doctor,
            commands::pult_version,
            commands::get_pult_path,
            commands::set_pult_path,
            commands::run_command,
            commands::resolve_pick_source,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
