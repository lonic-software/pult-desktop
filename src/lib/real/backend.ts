// The real backend: thin wrappers over Tauri's invoke() + event system,
// matching the mock backend's shape (src/lib/mock/backend.ts) field for
// field so App.svelte is agnostic to which one it's using.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import type { DoctorReport, Listing, RunEvent, RunSummary } from "../types";

export async function pickFolder(): Promise<string | null> {
  const result = await open({ directory: true, multiple: false, title: "Mount a repository" });
  return typeof result === "string" ? result : null;
}

export async function openRepo(path: string): Promise<Listing> {
  return invoke<Listing>("open_repo", { path });
}

export async function trustRepo(path: string): Promise<void> {
  await invoke<void>("trust_repo", { path });
}

export async function doctorCheck(path: string): Promise<DoctorReport> {
  return invoke<DoctorReport>("doctor", { path });
}

export async function pultVersion(): Promise<string> {
  return invoke<string>("pult_version");
}

export async function getPultPath(): Promise<string | null> {
  return invoke<string | null>("get_pult_path");
}

export async function setPultPath(path: string): Promise<void> {
  await invoke<void>("set_pult_path", { path });
}

export async function resolvePickSource(
  path: string,
  commandId: string,
  paramName: string,
  values: Record<string, string>,
): Promise<string[]> {
  return invoke<string[]>("resolve_pick_source", { path, commandId, paramName, values });
}

// `run_command` now spawns pult detached and returns as soon as the backend
// has started tailing its journal, not when the run finishes (see
// docs/run-journal.md) — so this must resolve on this run_id's own `exit`
// event, not on `invoke` completion. Listen is wired up before `invoke` is
// even called, so no early event on a fast run can be missed; every event
// (including a synthesized crash `exit` if pult never journaled at all —
// see `crate::journal::tail_run`'s bounded wait) still reaches `onEvent`.
export async function runCommand(
  path: string,
  id: string,
  values: Record<string, string>,
  runId: string,
  onEvent: (event: RunEvent) => void,
): Promise<void> {
  return new Promise((resolve, reject) => {
    let unlisten: (() => void) | undefined;
    const cleanup = () => unlisten?.();

    // The event channel is shared across every in-flight run, so filter to
    // this call's own run_id — otherwise a concurrent run's output would
    // leak into this one's output pane (see RunEvent's doc comment).
    listen<RunEvent>("pult://run-output", (event) => {
      if (event.payload.run_id !== runId) return;
      onEvent(event.payload);
      if (event.payload.kind === "exit") {
        cleanup();
        resolve();
      }
    })
      .then((un) => {
        unlisten = un;
        return invoke<void>("run_command", { path, id, runId, values });
      })
      .catch((e) => {
        cleanup();
        reject(e);
      });
  });
}

// Stop a journaled run: `path` scopes which repo's journal to read `run_id`'s
// `pgid` from (see `journal::stop_run` in src-tauri/src/journal.rs) — works
// identically for a run this app never spawned.
export async function stopRun(path: string, runId: string): Promise<void> {
  await invoke<void>("stop_run", { path, runId });
}

// This repo's run history — every journaled run, newest first (see
// `journal::list_runs`).
export async function listRuns(path: string): Promise<RunSummary[]> {
  return invoke<RunSummary[]>("list_runs", { path });
}

// Start (or no-op if already tailing) tailing `runId`'s journal — events
// arrive on the same `pult://run-output` channel `runCommand` uses.
export async function tailRun(path: string, runId: string): Promise<void> {
  await invoke<void>("tail_run", { path, runId });
}
