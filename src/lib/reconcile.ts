// Shared reconcile decision (docs/run-journal.md's hydration + CLI-poll
// legs — see +page.svelte's `reconcile`): given a command's current
// in-memory `RunRecord` (if any) and the journal's newest `RunSummary` for
// that command, decides what the caller should do with it. Pulled out as a
// pure function — no Svelte state, no `activeTails`, no backend calls — so
// the recency guard (the rule that used to let a stale poll response stomp
// a run that had *just* started) is unit-testable on its own; see
// `reconcile.test.ts`.
import type { RunRecord, RunSummary } from "./types";

export type ReconcileDecision =
  /** No record exists yet for this command: seed one from the summary. */
  | { action: "seed" }
  /** A record exists and holds the SAME run_id as the summary: this is a
   *  refresh, not a new run — the caller re-checks whether the journal now
   *  knows something the record doesn't (the crash/missed-exit catch-up). */
  | { action: "refresh" }
  /** A record exists holding a DIFFERENT run_id, but it's still `running`
   *  and the summary's own start time is no later than the record's —
   *  the journal simply hasn't caught up to the run we hold live yet. The
   *  summary is stale, not authoritative: leave the live record alone. */
  | { action: "skip" }
  /** A record exists holding a different run_id, and the summary is
   *  genuinely newer (e.g. a CLI run superseding an old session record):
   *  reseed from it, same as `seed`. */
  | { action: "reseed" };

export function reconcileDecision(
  current: RunRecord | undefined,
  summary: RunSummary,
): ReconcileDecision {
  if (!current) return { action: "seed" };
  if (current.runId === summary.run_id) return { action: "refresh" };
  const summaryStartedAt = Date.parse(summary.started_at);
  if (current.running && summaryStartedAt <= current.startedAt) {
    return { action: "skip" };
  }
  return { action: "reseed" };
}
