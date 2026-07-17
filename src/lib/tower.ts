// State derivation for the details page's SIGNAL tower (Tower.svelte) — kept
// separate from readiness.ts's board-meter logic (not a superset/reuse of
// `MeterState`) because the tower's vocabulary is genuinely different: it
// mirrors board readiness while idle, tracks live progress while running,
// and — for one blink's worth of time right after a run finishes — shows
// that run's own outcome, the same way the board's post-run overlay does
// (see readiness.ts's `BoardMeterOverride`, which this file's
// `TowerBlinkOverride` deliberately mirrors).
//
// The tower does NOT keep a standing PASSED/FAILED/STOPPED display the way
// it used to — see docs/design-language.md's "Blink is a mode": a finished
// run gets a few blinks (success: a few short green; failure: a few more,
// red — this is the one surface where a handful of blinks stands in for the
// board's latch, since being on the command's own page during a failure
// already counts as having seen it; stopped: the same brief, calm treatment
// as the board) and then the tower slews back to mirroring readiness, same
// as it would after any other transition. The run's outcome keeps living in
// words instead — RunView's last-run line, the output ✓/✗ summary, and the
// stage cards — none of which this file touches.
//
// `TowerBlinkOverride` is computed and timed by RunView (see its
// `towerBlink`/`towerBlinkTimer`, the same pattern +page.svelte uses for the
// board's overlay) — a component-local flag rather than anything here,
// since it only ever exists for the length of one blink sequence and
// RunView is destroyed and recreated every time the user leaves the details
// page and reopens a command (+page.svelte only renders it inside
// `{#if view === "run" && selectedCommand}`), so there's nothing to reset
// between visits.

import type { Readiness } from "./types";

export type TowerState =
  | "ready"
  | "check-failed"
  | "no-check"
  | "none"
  | "running-progress"
  | "running-indeterminate"
  | "blink-success"
  | "blink-run-failed"
  | "blink-run-stopped";

export type TowerColor = "green" | "red" | "amber" | "gray" | "none";

export interface TowerDisplay {
  state: TowerState;
  /** Fraction of the 28-segment column that's lit, 0..1 — meaningless
   *  during a blink state (Tower.svelte forces the full column while
   *  blinking, ignoring this field; see its `isBlinking`/`litCount`), kept
   *  populated anyway so `displayedLitCount`'s post-blink recovery slew has
   *  a coherent value to resume chasing toward the instant the blink ends. */
  level: number;
  /** The big digital readout ("62%" / "RDY" / "RUN" / "ERR" / "100%") — for
   *  a blink state this is the outcome's word form, shown for the blink's
   *  duration before the readout follows the readiness mirror again. */
  readout: string;
  /** The engraved word underneath ("PROGRESS" / "READY" / ...). */
  label: string;
  color: TowerColor;
  /** Whether the leading (top) lit segment should carry the pronounced
   *  climbing-edge pulse — running states only; every other state (idle
   *  mirrors and blink states alike) just gets the normal ambient liveness,
   *  itself suspended entirely during a blink (see Tower.svelte). */
  pulse: boolean;
}

const TOWER_LABEL: Record<TowerState, string> = {
  ready: "READY",
  "check-failed": "CHECK FAILED",
  "no-check": "NO CHECK",
  none: "NO SIGNAL",
  "running-progress": "PROGRESS",
  "running-indeterminate": "ACTIVE",
  "blink-success": "PASSED",
  "blink-run-failed": "FAILED",
  "blink-run-stopped": "STOPPED",
};

const TOWER_COLOR: Record<TowerState, TowerColor> = {
  ready: "green",
  "check-failed": "red",
  "no-check": "gray",
  none: "none",
  "running-progress": "amber",
  "running-indeterminate": "amber",
  "blink-success": "green",
  "blink-run-failed": "red",
  "blink-run-stopped": "amber",
};

// Standing levels per docs/design-language.md's state table, with the two
// implementation deltas agreed for this page (indeterminate 60%, ready 80%
// — see the design reference's header comment). `running-progress` isn't
// here — its level depends on live progress data, computed in
// `towerDisplay` below. The blink states' levels are never actually
// rendered as a partial column (see the `level` doc comment above) but
// still need a real number for the post-blink recovery slew to resume from.
const TOWER_LEVEL: Record<Exclude<TowerState, "running-progress">, number> = {
  ready: 0.8,
  "check-failed": 1,
  "no-check": 0.2,
  none: 0,
  "running-indeterminate": 0.6,
  "blink-success": 1,
  "blink-run-failed": 1,
  "blink-run-stopped": 0.6,
};

/** A minimal view of a run record — just the fields the tower's state
 *  machine actually branches on, so RunView doesn't have to import the full
 *  `RunRecord` shape here. */
export interface TowerRunInput {
  running: boolean;
  progressPct: number | null;
  stopped: boolean;
  exitCode: number | null;
}

/** The tower's post-run blink overlay — mirrors readiness.ts's
 *  `BoardMeterOverride` deliberately: same three kinds, same "only while a
 *  run isn't currently in flight" precedence rule below. Computed and timed
 *  by RunView (see this file's header comment), not here. */
export interface TowerBlinkOverride {
  kind: "success" | "run-failed" | "run-stopped";
}

export function towerStateFor(
  readiness: Readiness,
  run: TowerRunInput | null,
  blink: TowerBlinkOverride | null,
): TowerState {
  if (run?.running) {
    return run.progressPct != null ? "running-progress" : "running-indeterminate";
  }
  if (blink) {
    if (blink.kind === "success") return "blink-success";
    if (blink.kind === "run-failed") return "blink-run-failed";
    return "blink-run-stopped";
  }
  // Idle: mirror the board's readiness semantics exactly (see
  // readiness.ts's `meterStateFor`) — "untrusted" and "doctor hasn't
  // answered yet" both collapse to the same dark "none" reading there, and
  // do here too.
  if (readiness === "ready") return "ready";
  if (readiness === "failed") return "check-failed";
  if (readiness === "no-check") return "no-check";
  return "none";
}

/** Resolves a `TowerState` into everything Tower.svelte needs to paint one
 *  frame. `progressPct` is the run's *current* progress (used for the live
 *  climb) — unused by every other state, blink states included (their level
 *  comes from `TOWER_LEVEL`, not live progress, since the run that produced
 *  them has already ended by the time a blink state is ever reached). */
export function towerDisplay(state: TowerState, progressPct: number | null): TowerDisplay {
  const pulse = state === "running-progress" || state === "running-indeterminate";
  const color = TOWER_COLOR[state];
  const label = TOWER_LABEL[state];

  if (state === "running-progress") {
    const pct = Math.max(0, Math.min(100, progressPct ?? 0));
    return { state, level: pct / 100, readout: `${pct}%`, label, color, pulse };
  }
  const level = TOWER_LEVEL[state];
  const readout =
    state === "ready"
      ? "RDY"
      : state === "check-failed"
        ? "ERR"
        : state === "no-check"
          ? "--"
          : state === "none"
            ? "--"
            : state === "running-indeterminate"
              ? "RUN"
              : state === "blink-success"
                ? "100%"
                : state === "blink-run-stopped"
                  ? "—" // em dash — stop is pre-acknowledged, no pass/fail verdict to report
                  : "ERR"; // blink-run-failed
  return { state, level, readout, label, color, pulse };
}
