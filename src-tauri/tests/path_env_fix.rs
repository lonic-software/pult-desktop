//! Verifies the login-shell PATH repair (`fix_path_env::fix()`, called
//! first thing in `lib.rs::run()`, before anything spawns) actually
//! repairs a launchd-minimal PATH — the exact failure mode this fixes is a
//! packaged GUI app inheriting `/usr/bin:/bin:/usr/sbin:/sbin`-ish PATH, so
//! `pult`'s `check:`/`run:` shell-outs can't find Homebrew/toolchain
//! binaries even though the same checks pass from a terminal.
//!
//! This can't drive Tauri's own `run()` (it needs a real window), so it
//! calls `fix_path_env::fix()` directly, which is the entire fix — `run()`
//! only adds the "don't fail the app" wrapper around it. One test, own
//! file/process: it mutates the global `PATH` env var, which would race
//! any other test in the same process that reads or spawns based on PATH.
//!
//! Windows is skipped: the crate's Windows behavior addresses a different
//! bug (an inheritance glitch in `std::process::Command`, not a missing
//! shell PATH) and isn't reproducible by pre-setting `PATH` in-process.

#[test]
#[cfg(not(windows))]
fn fix_path_env_repairs_a_launchd_minimal_path() {
    let minimal = "/usr/bin:/bin:/usr/sbin:/sbin";
    unsafe {
        std::env::set_var("PATH", minimal);
    }

    let result = fix_path_env::fix();
    assert!(result.is_ok(), "fix() failed: {:?}", result.err());

    let repaired = std::env::var("PATH").expect("PATH should still be set");
    assert_ne!(
        repaired, minimal,
        "PATH should have changed after fix() — got the minimal PATH back unchanged"
    );

    let repaired_dirs: Vec<&str> = repaired.split(':').collect();
    assert!(
        repaired_dirs.len() > minimal.split(':').count(),
        "repaired PATH has no more entries than the minimal PATH: {repaired}"
    );

    // The dirs that made the minimal PATH broken in the first place should
    // survive the repair — this isn't a replacement, it's an addition.
    for dir in minimal.split(':') {
        assert!(
            repaired_dirs.contains(&dir),
            "repaired PATH dropped {dir}: {repaired}"
        );
    }
}
