// The VITE_MOCK=1 stand-in for the real Tauri backend. Mirrors src/lib/api.ts's
// shape exactly so App.svelte never has to know which one it's talking to.

import type { DoctorReport, Listing, RunEvent } from "../types";
import { mockDoctorReport, mockListingTrusted, mockListingUntrusted } from "./fixtures";

const FIXTURE_PATH = "/Users/operator/acme-ops";

let trusted = false;

function delay(ms: number) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

export async function pickFolder(): Promise<string | null> {
  await delay(120);
  return FIXTURE_PATH;
}

export async function openRepo(path: string): Promise<Listing> {
  await delay(250);
  if (path !== FIXTURE_PATH) {
    throw "No pult.yaml here — point me at a repository that has one";
  }
  return trusted ? mockListingTrusted : mockListingUntrusted;
}

export async function trustRepo(_path: string): Promise<void> {
  await delay(200);
  trusted = true;
}

export async function doctorCheck(_path: string): Promise<DoctorReport> {
  await delay(300);
  return mockDoctorReport;
}

export async function pultVersion(): Promise<string> {
  return "pult 0.4.0";
}

export async function getPultPath(): Promise<string | null> {
  return null;
}

export async function setPultPath(_path: string): Promise<void> {
  await delay(80);
}

const MOCK_RUN_LOG: Record<string, string[]> = {
  shell: ["opening a shell in dev…", "done"],
  status: ["checking status…", "all good"],
  import: [
    "└  running: echo importing with token '••••••' note demo",
    "importing with token hunter2 note demo",
  ],
  "aws:whoami": ["arn:aws:sts::123456789012:assumed-role/demo/operator"],
  "aws:deploy": [
    "step 1/3 build",
    "building…",
    "step 2/3 push",
    "pushing image…",
    "step 3/3 release",
    "releasing eu-west-1…",
    "done",
  ],
};

export async function runCommand(
  _path: string,
  id: string,
  _values: Record<string, string>,
  runId: string,
  onEvent: (event: RunEvent) => void,
): Promise<void> {
  const lines = MOCK_RUN_LOG[id] ?? ["running…", "done"];
  for (const text of lines) {
    await delay(180);
    onEvent({ kind: "line", run_id: runId, stream: "stdout", text });
  }
  await delay(120);
  onEvent({ kind: "exit", run_id: runId, code: 0 });
}
