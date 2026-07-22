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

// `run_command` spawns pult detached and returns as soon as the process is
// launched — it no longer starts a tail itself (see
// src-tauri/src/commands.rs). Tailing is the frontend's job: the caller
// (+page.svelte's `handleRun`) invokes `tailRun` explicitly once this
// resolves, the same single tail-creation path every other run source
// (hydration, the poll, a lazy tail) already goes through. This just
// forwards the invoke and lets a spawn error (a bad path, pult missing,
// pult itself rejecting the run) surface as a rejection.
export async function runCommand(
  path: string,
  id: string,
  values: Record<string, string>,
  runId: string,
): Promise<void> {
  await invoke<void>("run_command", { path, id, runId, values });
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

// One shared subscription to the whole `pult://run-output` channel, for
// callers that need to watch events across potentially many concurrently-
// tailed runs (hydration, the CLI-visibility poll, a lazily-tailed finished
// run) without opening a fresh `listen()` per run the way `runCommand`'s own
// dedicated one does — see +page.svelte's `activeTails` for the run_id
// routing this feeds. Returns an unsubscribe function immediately, safe to
// call even before the underlying `listen()` promise has resolved (the
// `cancelled` flag makes that a same-tick no-op once it does).
export function subscribeRunOutput(onEvent: (event: RunEvent) => void): () => void {
  let unlisten: (() => void) | undefined;
  let cancelled = false;
  listen<RunEvent>("pult://run-output", (event) => onEvent(event.payload)).then((un) => {
    if (cancelled) {
      un();
      return;
    }
    unlisten = un;
  });
  return () => {
    cancelled = true;
    unlisten?.();
  };
}
