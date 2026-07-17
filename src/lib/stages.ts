// Derives the details page's STAGES module (a card per declared step) from a
// command's static `steps` and a run's live `step`/`stepHistory`/`progress`
// events — pure functions of (command, run), no component state, so they're
// trivially testable and RunView just renders whatever comes out.

import type { CommandInfo, RunRecord, StepEvent } from "./types";
import { formatDuration } from "./time";

export type StageKind = "done" | "active" | "failed" | "pending";

export interface StageCard {
  key: string;
  name: string;
  kind: StageKind;
  meta: string;
}

/** Whether the STAGES module should render at all — see the design
 *  reference's STAGES MODULE comment: shown once the command declares steps
 *  (a pending skeleton, even before the first run — "all pending" preview)
 *  or step events have actually arrived (covers a command with no declared
 *  `steps:` that still emits them at runtime). Otherwise there's nothing
 *  stage-shaped to show, so the module is omitted rather than rendered
 *  empty. */
export function stagesVisible(command: CommandInfo, run: RunRecord | null): boolean {
  return (command.steps?.length ?? 0) > 0 || (run?.stepHistory.length ?? 0) > 0;
}

/** Builds the stage ladder from a command's declared steps (the skeleton)
 *  plus a run's step events, progress, and terminal outcome.
 *
 *  Rule (see the design reference's STAGES MODULE comment): while running,
 *  steps before the current one are "done", the current one is "active",
 *  and the rest are "pending" ("queued"). On exit: success marks every step
 *  done (a step whose own `k` event never technically arrived — race with a
 *  fast finish — still counts as done, since the run as a whole succeeded);
 *  failure marks the active step "failed" and leaves the rest "pending"
 *  ("skipped"); a stop leaves the active step and everything after it
 *  "pending" ("skipped") too — the user caused it, nothing after the stop
 *  point ran, full stop.
 *
 *  A command with no declared `steps:` but live step events falls back to
 *  naming stages from the events themselves (`n` taken from the highest `n`
 *  any event has reported), so a dynamically-stepped run still gets a
 *  ladder instead of nothing. */
export function deriveStages(command: CommandInfo, run: RunRecord | null): StageCard[] {
  const declared = command.steps;
  const history = run?.stepHistory ?? [];
  const n = declared?.length ?? (history.length > 0 ? Math.max(...history.map((s) => s.n)) : 0);
  if (n === 0) return [];

  const names: string[] =
    declared ??
    Array.from({ length: n }, (_, i) => history.find((s) => s.k === i + 1)?.name ?? `Step ${i + 1}`);

  const byK = new Map(history.map((s) => [s.k, s]));
  const currentK = run?.step?.k ?? 0; // 0 = no step event has arrived yet
  const running = run?.running ?? false;
  const terminal = !!run && !running;
  const outcome: "success" | "failed" | "stopped" | null = !terminal
    ? null
    : run!.stopped
      ? "stopped"
      : run!.exitCode === 0
        ? "success"
        : "failed";

  return names.map((name, i) => {
    const k = i + 1;
    const entry = byK.get(k);
    const nextEntry = byK.get(k + 1);
    let kind: StageKind;
    let meta: string;

    if (outcome === "success") {
      kind = "done";
      meta = elapsedMeta(entry, nextEntry, run!.endedAt);
    } else if (outcome === "failed") {
      if (k < currentK) {
        kind = "done";
        meta = elapsedMeta(entry, nextEntry, null);
      } else if (k === currentK) {
        kind = "failed";
        meta = run!.status ?? (run!.exitCode !== null ? `exit ${run!.exitCode}` : "failed");
      } else {
        kind = "pending";
        meta = "skipped";
      }
    } else if (outcome === "stopped") {
      if (k < currentK) {
        kind = "done";
        meta = elapsedMeta(entry, nextEntry, null);
      } else {
        kind = "pending";
        meta = "skipped";
      }
    } else if (running) {
      if (k < currentK) {
        kind = "done";
        meta = elapsedMeta(entry, nextEntry, null);
      } else if (k === currentK) {
        kind = "active";
        meta = run?.progress?.pct != null ? `${run.progress.pct}%` : "…";
      } else {
        kind = "pending";
        meta = "queued";
      }
    } else {
      // No run yet this visit — the pre-run skeleton, all pending.
      kind = "pending";
      meta = "queued";
    }

    return { key: `${k}:${name}`, name, kind, meta };
  });
}

/** A "done" step's elapsed time, only when cheaply derivable from the local
 *  arrival timestamps already on hand (this step's own `at` to the next
 *  step's `at`, or to the run's `endedAt` for the last one) — never
 *  fabricated. Falls back to a minimal honest "done" when there's nothing
 *  to subtract (see StepEvent's doc comment in types.ts: `at` is a local
 *  arrival stamp, not part of the wire protocol, so it can be missing or
 *  imprecise). */
function elapsedMeta(
  entry: StepEvent | undefined,
  nextEntry: StepEvent | undefined,
  endedAt: number | null,
): string {
  if (!entry) return "done";
  const end = nextEntry?.at ?? endedAt ?? undefined;
  if (end === undefined) return "done";
  return formatDuration(end - entry.at);
}
