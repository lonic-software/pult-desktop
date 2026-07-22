// The VITE_MOCK=1 stand-in for the real Tauri backend. Mirrors src/lib/api.ts's
// shape exactly so App.svelte never has to know which one it's talking to.

import type { DoctorReport, Listing, RunEvent, RunSummary } from "../types";
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

// Canned `pick.source` resolution for the dynamic-pick fixture
// (`aws:deploy`'s `customer` param, which `depends_on: ["region"]`) — keyed
// by `<commandId>.<paramName>` so the mock stays shaped like the real
// resolve call (repo, command, param, depends_on values). A small artificial
// delay so the loading state is demoable without a real `pult`.
const MOCK_PICK_SOURCE_OPTIONS: Record<string, (values: Record<string, string>) => string[]> = {
  "aws:deploy.customer": (values) =>
    values.region === "us-east-1"
      ? ["us-nova-holdings", "us-atlas-retail"]
      : ["eu-nova-holdings", "eu-atlas-retail"],
};

export async function resolvePickSource(
  _path: string,
  commandId: string,
  paramName: string,
  values: Record<string, string>,
): Promise<string[]> {
  await delay(350);
  const resolver = MOCK_PICK_SOURCE_OPTIONS[`${commandId}.${paramName}`];
  return resolver ? resolver(values) : [];
}

// A scripted run's timeline: one action per PULT_EVENTS/output kind (see
// RunEvent in types.ts), each preceded by its own delay so a script can pace
// itself — a burst of quick lines here, a slower deliberate pause there — to
// demo something specific rather than uniformly ticking at one rate the way
// the old flat MOCK_RUN_LOG did.
type MockAction =
  | { delay: number; kind: "line"; stream: "stdout" | "stderr"; text: string }
  | { delay: number; kind: "step"; k: number; n: number; name: string }
  | { delay: number; kind: "progress"; pct: number | null; text?: string | null }
  | { delay: number; kind: "status"; text: string };

interface MockScript {
  actions: MockAction[];
  /** `null` exit code = the run "fails" without a normal process exit code,
   *  same shape `RunEvent`'s `exit.code` allows for a signal-killed process;
   *  every script here uses either `0` or a small positive int. */
  exitCode: number;
}

function line(delayMs: number, text: string, stream: "stdout" | "stderr" = "stdout"): MockAction {
  return { delay: delayMs, kind: "line", stream, text };
}

// Demo scripts, one per fixture command that needs to say something
// specific about the SIGNAL tower/board meter language (see
// docs/design-language.md and RunView/Tower.svelte):
//
//   aws:deploy  — determinate progress + stages, ends in success. Emits
//                 step/progress events alongside its lines so the tower
//                 climbs with a "<pct>%" readout, the stage ladder advances
//                 card by card, and the board meter's level tracks pct too.
//   status      — indeterminate (no step/progress events at all, ever) —
//                 the tower reads 60% "RUN"/"ACTIVE", the board meter stays
//                 the fixed 3-lit floor plus its sweep strip. Paced slower
//                 than a 2-line command needs so there's time to screenshot
//                 the running state.
//   import      — fails (non-zero exit, stderr line) — proves the board's
//                 blinking red latch and the details page's own few-blinks-
//                 then-recover "ERR" tower + failed stage-less output.
//   test:smoke  — a longer, evenly-paced run with nothing time-sensitive in
//                 it, meant to be interrupted mid-run via Stop — proves the
//                 stop flow (brief amber blink, no latch) without racing a
//                 screenshot script against a run that finishes on its own.
const MOCK_SCRIPTS: Record<string, MockScript> = {
  shell: { actions: [line(180, "opening a shell in dev…"), line(180, "done")], exitCode: 0 },

  status: {
    actions: [
      line(250, "checking status…"),
      { delay: 400, kind: "status", text: "verifying environment" },
      line(450, "all good"),
    ],
    exitCode: 0,
  },

  import: {
    actions: [
      line(200, "resolving vendor export…"),
      line(250, "authenticating with token '••••••'"),
      line(300, "error: token rejected (401 unauthorized)", "stderr"),
    ],
    exitCode: 1,
  },

  "aws:whoami": {
    actions: [line(220, "arn:aws:sts::123456789012:assumed-role/demo/operator")],
    exitCode: 0,
  },

  "aws:deploy": {
    actions: [
      { delay: 150, kind: "step", k: 1, n: 3, name: "build" },
      { delay: 200, kind: "progress", pct: 12 },
      line(250, "building image…"),
      { delay: 250, kind: "progress", pct: 33 },
      { delay: 200, kind: "step", k: 2, n: 3, name: "push" },
      { delay: 250, kind: "progress", pct: 50 },
      line(250, "pushing image…"),
      { delay: 250, kind: "progress", pct: 66 },
      { delay: 200, kind: "step", k: 3, n: 3, name: "release" },
      { delay: 250, kind: "progress", pct: 85 },
      line(250, "releasing eu-west-1…"),
      { delay: 200, kind: "progress", pct: 100 },
      line(150, "done"),
    ],
    exitCode: 0,
  },

  "test:smoke": {
    actions: [
      line(500, "collecting tests…"),
      line(500, "running unit suite…"),
      line(500, "running integration suite…"),
      line(500, "running contract suite…"),
      line(500, "running e2e suite…"),
      line(500, "all suites passed"),
    ],
    exitCode: 0,
  },

  "test:deploy": { actions: [line(180, "loading fixtures…"), line(180, "done")], exitCode: 0 },
};

const DEFAULT_SCRIPT: MockScript = { actions: [line(180, "running…"), line(180, "done")], exitCode: 0 };

// Run ids `stopRun` has been asked to stop — `runCommand`'s loop below
// checks this before each emitted action so a mock run can be interrupted
// the same way a real one can, without any real process behind it to
// signal.
const stoppedRuns = new Set<string>();

export async function stopRun(_path: string, runId: string): Promise<void> {
  await delay(80);
  stoppedRuns.add(runId);
}

// Run ids `runCommand` is actively driving directly (a fresh in-app run —
// see `runCommand` below). NOT a mirror of the real backend's
// `TailRegistry` (fix round 3 corrected this stale claim): `runCommand`
// drives an app-started run's script straight into `emitRunOutput`, gen-less,
// the moment it's called — there's no journal and no tail standing in front
// of it yet in this mock, unlike the real backend, where nothing is emitted
// for a run_id until `startTail`'s `tail_run` call actually claims it. This
// set exists purely so `tailRun`'s own explicit call right after an
// app-started run's `runCommand` resolves (`startTail`, see +page.svelte's
// `handleRun` — made only to match the real backend's single-tail-creation-
// path shape) stays a genuine no-op instead of tripping `tailRun`'s
// "unknown to this mock" fallback and synthesizing a spurious exit out from
// under a run that's actually still going. `tailRun`'s OWN generation fence
// (`tailGenerations`, below) is the actual `TailRegistry` mirror.
const drivingRuns = new Set<string>();

// Drives one script's actions into `emit`, checking `stoppedRuns` before
// each — the one place both `runCommand` (a fresh in-app run) and `tailRun`
// (the scripted "CLI-started" run, see below) turn a `MockScript` into
// actual `RunEvent`s, so the two paths can't drift out of sync with each
// other's pacing/stop-handling.
async function driveScript(
  script: MockScript,
  runId: string,
  emit: (event: RunEvent) => void,
): Promise<void> {
  for (const action of script.actions) {
    await delay(action.delay);
    if (stoppedRuns.has(runId)) {
      stoppedRuns.delete(runId);
      emit({ kind: "exit", run_id: runId, code: null, stopped: true });
      return;
    }
    switch (action.kind) {
      case "line":
        emit({ kind: "line", run_id: runId, stream: action.stream, text: action.text });
        break;
      case "step":
        emit({ kind: "step", run_id: runId, k: action.k, n: action.n, name: action.name });
        break;
      case "progress":
        emit({ kind: "progress", run_id: runId, pct: action.pct, text: action.text ?? null });
        break;
      case "status":
        emit({ kind: "status", run_id: runId, text: action.text });
        break;
    }
  }
  await delay(120);
  emit({ kind: "exit", run_id: runId, code: script.exitCode, stopped: false });
}

// Matches the real backend's contract (src/lib/real/backend.ts): resolves
// once the run is under way, not once it finishes, and delivers its events
// through the same in-module channel `subscribeRunOutput` feeds
// (`emitRunOutput`, defined below) rather than a dedicated per-call
// callback — so an app-started mock run routes through +page.svelte's
// `activeTails` exactly the way a real one does.
export async function runCommand(
  _path: string,
  id: string,
  _values: Record<string, string>,
  runId: string,
): Promise<void> {
  const script = MOCK_SCRIPTS[id] ?? DEFAULT_SCRIPT;
  drivingRuns.add(runId);
  void driveScript(script, runId, emitRunOutput).finally(() => {
    drivingRuns.delete(runId);
  });
}

// --- Journal-reader demo surface -----------------------------------------
//
// The real backend's `listRuns`/`tailRun`/`subscribeRunOutput` read pult's
// on-disk run journal and stream from a shared Tauri event channel (see
// src/lib/real/backend.ts). There's no journal and no Tauri runtime in
// VITE_MOCK, so this is a small, deliberately modest demo standing in for
// it — not a simulator of the on-disk protocol — reusing the scripted-run
// machinery above (`driveScript`/`MOCK_SCRIPTS`) rather than inventing a
// second timeline format:
//
//   - `HISTORY_RUNS` is a fixed, canned run history for the fixture repo:
//     one exited (success), one crashed, one stopped — present from the
//     very first `listRuns` call, so hydration-on-open has something to
//     seed immediately.
//   - `CLI_RUN_ID` is a single scripted "started outside the app" run: it
//     doesn't exist at all in `listRuns`'s result until
//     `CLI_RUN_APPEAR_AFTER_MS` after the fixture repo's first `listRuns`
//     call (a reasonable proxy for "since this device was opened") — that's
//     the ~3s poll's moment to discover it, appear as "running", and tail it
//     live to completion, demoing "a `pult deploy` typed in a terminal shows
//     up on the board" without any real CLI process behind it.
//
// A shared in-module pub/sub stands in for the real backend's Tauri event
// channel, so `tailRun` (and `runCommand` above, for an app-started run) can
// deliver events the same "fire and forget, listen separately" way
// `subscribeRunOutput` expects on both backends.
const outputListeners = new Set<(event: RunEvent) => void>();

function emitRunOutput(event: RunEvent): void {
  for (const fn of outputListeners) fn(event);
}

export function subscribeRunOutput(onEvent: (event: RunEvent) => void): () => void {
  outputListeners.add(onEvent);
  return () => {
    outputListeners.delete(onEvent);
  };
}

function isoAgo(ms: number): string {
  return new Date(Date.now() - ms).toISOString();
}

const MINUTE = 60_000;

const HISTORY_EXITED_ID = "mock-history-exited";
const HISTORY_CRASHED_ID = "mock-history-crashed";
const HISTORY_STOPPED_ID = "mock-history-stopped";
const HISTORY_INTERACTIVE_ID = "mock-history-interactive";

// Fixed at module load, not recomputed per call — a stable demo history
// rather than one that keeps sliding later the longer the app has been
// running.
const HISTORY_RUNS: RunSummary[] = [
  {
    run_id: HISTORY_EXITED_ID,
    command_id: "aws:deploy",
    command_title: "Deploy stack",
    status: "exited",
    exit_code: 0,
    started_at: isoAgo(41 * MINUTE),
    ended_at: isoAgo(40 * MINUTE),
    origin: "app",
    interactive: false,
  },
  {
    run_id: HISTORY_CRASHED_ID,
    command_id: "import",
    command_title: "Import",
    status: "crashed",
    exit_code: null,
    started_at: isoAgo(25 * MINUTE),
    ended_at: null,
    origin: "cli",
    interactive: false,
  },
  {
    run_id: HISTORY_STOPPED_ID,
    command_id: "test:smoke",
    command_title: "Smoke test",
    status: "stopped",
    exit_code: null,
    started_at: isoAgo(12 * MINUTE),
    ended_at: isoAgo(11 * MINUTE),
    origin: "app",
    interactive: false,
  },
  // Demos docs/run-journal.md's "Interactive commands" reader rule (see
  // OutputPane.svelte's `showInteractiveNote`): `origin: "cli"` because the
  // app itself can never spawn an interactive command (RunView's
  // `disabledReason` blocks the Run button for one) — this is what hydrating
  // a real `pult shell` typed in a terminal looks like.
  {
    run_id: HISTORY_INTERACTIVE_ID,
    command_id: "shell",
    command_title: "Shell",
    status: "exited",
    exit_code: 0,
    started_at: isoAgo(8 * MINUTE),
    ended_at: isoAgo(7 * MINUTE),
    origin: "cli",
    interactive: true,
  },
];

// A few canned lines + a terminal exit per history run — what `tailRun`
// replays the first (and only, in practice — the frontend never re-tails a
// run whose output it already has) time each is looked at. `crashed: true`
// on the crashed run's exit is the one place this mock deliberately does
// what a real crash-detecting tail does (see the `RunEvent.exit.crashed` doc
// comment in types.ts) — purely to demo that rendering.
const HISTORY_REPLAY: Record<
  string,
  { lines: { stream: "stdout" | "stderr"; text: string }[]; exit: RunEvent & { kind: "exit" } }
> = {
  // A few lines here carry real ANSI escapes (as `FORCE_COLOR=1`-style
  // output would) so VITE_MOCK demos ../ansi.ts + OutputPane's rendering of
  // it without needing a real colored CLI running: a green ✓ (bold, since
  // most tools bold their pass/fail glyphs), a cyan URL, and a bold red
  // transient error — this run still exits 0, matching a real deploy log
  // that retries a flaky step and moves on rather than failing outright.
  [HISTORY_EXITED_ID]: {
    lines: [
      { stream: "stdout", text: "\x1b[1;32m✓\x1b[0m building image…" },
      { stream: "stdout", text: "\x1b[1;32m✓\x1b[0m pushing image…" },
      {
        stream: "stdout",
        text: "\x1b[1;31merror: health check timed out, retrying (1/3)\x1b[0m",
      },
      { stream: "stdout", text: "\x1b[1;32m✓\x1b[0m releasing eu-west-1…" },
      {
        stream: "stdout",
        text: "deployed: \x1b[36mhttps://acme-ops.eu-west-1.example.com\x1b[0m",
      },
      { stream: "stdout", text: "done" },
    ],
    exit: { kind: "exit", run_id: HISTORY_EXITED_ID, code: 0, stopped: false },
  },
  [HISTORY_CRASHED_ID]: {
    lines: [
      { stream: "stdout", text: "resolving vendor export…" },
      { stream: "stdout", text: "authenticating with token '••••••'" },
    ],
    exit: { kind: "exit", run_id: HISTORY_CRASHED_ID, code: null, stopped: false, crashed: true },
  },
  [HISTORY_STOPPED_ID]: {
    lines: [
      { stream: "stdout", text: "collecting tests…" },
      { stream: "stdout", text: "running unit suite…" },
    ],
    exit: { kind: "exit", run_id: HISTORY_STOPPED_ID, code: null, stopped: true },
  },
  // No lines — an interactive run's `events.jsonl` legitimately contains
  // only its `exit` record (docs/run-journal.md), so this is the one replay
  // in the set with an empty `lines` array; `tailRun` below still replays
  // the exit event, which composes with OutputPane's placeholder note
  // rather than replacing it (see OutputPane.svelte's `showInteractiveNote`).
  [HISTORY_INTERACTIVE_ID]: {
    lines: [],
    exit: { kind: "exit", run_id: HISTORY_INTERACTIVE_ID, code: 0, stopped: false },
  },
};

// The single scripted "CLI-started" run (see the file-header comment above)
// — its command reuses the existing `status` script (an indeterminate run:
// no step/progress events, just a couple of lines) so it has something to
// stream once tailed.
const CLI_RUN_ID = "mock-cli-run";
const CLI_RUN_COMMAND_ID = "status";
const CLI_RUN_APPEAR_AFTER_MS = 10_000;

let cliRunFirstSeenAt: number | null = null;
let cliRunPhase: "hidden" | "running" | "done" = "hidden";
let cliRunStartedAt = 0;
let cliRunEndedAt: string | null = null;

export async function listRuns(path: string): Promise<RunSummary[]> {
  await delay(60);
  if (path !== FIXTURE_PATH) return [];
  if (cliRunFirstSeenAt === null) cliRunFirstSeenAt = Date.now();
  if (cliRunPhase === "hidden" && Date.now() - cliRunFirstSeenAt >= CLI_RUN_APPEAR_AFTER_MS) {
    cliRunPhase = "running";
    cliRunStartedAt = Date.now();
  }
  const runs = [...HISTORY_RUNS];
  if (cliRunPhase === "running" || cliRunPhase === "done") {
    runs.unshift({
      run_id: CLI_RUN_ID,
      command_id: CLI_RUN_COMMAND_ID,
      command_title: "Status",
      status: cliRunPhase === "running" ? "running" : "exited",
      exit_code: cliRunPhase === "done" ? 0 : null,
      started_at: new Date(cliRunStartedAt).toISOString(),
      ended_at: cliRunPhase === "done" ? cliRunEndedAt : null,
      origin: "cli",
      interactive: false,
    });
  }
  return runs;
}

// Fix round 3's generation fence (mirrors the real backend's `TailRegistry`,
// src-tauri/src/journal.rs), modeled just enough to exercise the OBSERVABLE
// wire contract under VITE_MOCK: re-invoking `tailRun` for a run_id already
// claimed here cancels/supersedes that claim (a flag its own replay checks
// before every emit — not real concurrency, just the same
// call-cancels-the-old-one outcome) and hands out a fresh, strictly
// increasing generation starting the replay over from scratch, leading with
// a `tail_start` event and stamping every event that generation goes on to
// emit with it — the same shape types.ts's `RunEvent.tail_gen` doc comment
// promises from a real tail. Deliberately not modeling more than that (no
// real second thread ever races here) — the mock doesn't need more to demo
// the fence.
const tailGenerations = new Map<string, { generation: number; cancelled: { value: boolean } }>();

function claimTailGeneration(runId: string): { generation: number; cancelled: { value: boolean } } {
  const existing = tailGenerations.get(runId);
  if (existing) existing.cancelled.value = true;
  const claimed = { generation: (existing?.generation ?? 0) + 1, cancelled: { value: false } };
  tailGenerations.set(runId, claimed);
  return claimed;
}

export async function tailRun(path: string, runId: string): Promise<void> {
  if (path !== FIXTURE_PATH) return;
  // An app-started run's own script is already being driven directly by
  // `runCommand` (see `drivingRuns`'s doc comment) — this call only exists
  // to match the real backend's single-tail-creation-path shape (`startTail`
  // always calls this right after `runCommand` resolves); there's no
  // journal here to re-tail, so unlike everything below, this is a genuine
  // no-op, not a generation-fenced restart.
  if (drivingRuns.has(runId)) return;

  const { generation, cancelled } = claimTailGeneration(runId);
  emitRunOutput({ kind: "tail_start", run_id: runId, tail_gen: generation });
  const emit = (event: RunEvent) => {
    if (cancelled.value) return;
    emitRunOutput({ ...event, tail_gen: generation });
  };

  const replay = HISTORY_REPLAY[runId];
  if (replay) {
    await delay(150);
    for (const l of replay.lines) {
      await delay(180);
      emit({ kind: "line", run_id: runId, stream: l.stream, text: l.text });
    }
    await delay(150);
    emit(replay.exit);
    return;
  }
  if (runId === CLI_RUN_ID) {
    if (cliRunPhase !== "running") return; // finished, or not yet visible
    const script = MOCK_SCRIPTS[CLI_RUN_COMMAND_ID] ?? DEFAULT_SCRIPT;
    await driveScript(script, runId, emit);
    cliRunPhase = "done";
    cliRunEndedAt = new Date().toISOString();
    return;
  }
  // Unknown to this mock (never in `HISTORY_RUNS`/`CLI_RUN_ID`) — mirror the
  // real backend's "never journaled" fallback so a caller waiting on this
  // run's events isn't left hanging forever.
  emit({ kind: "exit", run_id: runId, code: null, stopped: false });
}
