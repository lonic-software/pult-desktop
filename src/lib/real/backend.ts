// The real backend: thin wrappers over Tauri's invoke() + event system,
// matching the mock backend's shape (src/lib/mock/backend.ts) field for
// field so App.svelte is agnostic to which one it's using.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import type { DoctorReport, Listing, RunEvent } from "../types";

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

export async function runCommand(
  path: string,
  id: string,
  values: Record<string, string>,
  runId: string,
  onEvent: (event: RunEvent) => void,
): Promise<void> {
  // The event channel is shared across every in-flight run, so filter to
  // this call's own run_id — otherwise a concurrent run's output would leak
  // into this one's output pane (see RunEvent's doc comment in types.ts).
  const unlisten = await listen<RunEvent>("pult://run-output", (event) => {
    if (event.payload.run_id === runId) onEvent(event.payload);
  });
  try {
    await invoke<void>("run_command", { path, id, runId, values });
  } finally {
    unlisten();
  }
}

// Stop a run started by `runCommand`. `run_id` isn't scoped to a repo path
// here — the backend's `RunRegistry` is keyed by run_id alone (see
// `stop_run` in src-tauri/src/commands.rs) — so this is just a pass-through.
export async function stopRun(runId: string): Promise<void> {
  await invoke<void>("stop_run", { runId });
}
