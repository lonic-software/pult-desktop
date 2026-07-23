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
import { SUCCESS_BLINK_COUNT, STOPPED_BLINK_COUNT, TOWER_FAILURE_BLINK_COUNT } from "./meterLiveness";

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

/** The CSS var contract for the visual-polish ambient glow + screen
 *  "shine-back" reflection (docs/design-language.md's "Analog liveness"):
 *  RunView computes this ONCE from the same `TowerDisplay` it already feeds
 *  Tower.svelte, and sets the two fields as inline custom properties
 *  (`--meter-glow-color`/`--meter-glow-level`) on the page root — the one
 *  ancestor shared by both the tower module and the far-away params/stages/
 *  output screens, so a plain CSS `var()` reaches both without prop-drilling
 *  across that DOM distance. Tower.svelte's own ambient wash (`.well::before`
 *  there) reads the exact same two properties by inheritance rather than
 *  recomputing this mapping a second time.
 *
 *  `gray` (no-check) and `none` (untrusted/dark) never glow — same rule as
 *  Tower.svelte's `flickerEligible`: a "no signal" reading shouldn't warm
 *  anything nearby, on the tower or on the glass across the room.
 *
 *  `blinkKind`/`blinkCount` are the doctrine refinement (docs/design-
 *  language.md's "Blink is a mode": cast light now flashes in lockstep with
 *  the lamp): the var-driven layers Tower.svelte's own well can't reach by
 *  class alone — the params/stages/output screens' shine-back (crt.css) and
 *  Rack.svelte's sidebar shine, both siblings of the tower reading only
 *  these forwarded custom properties — need to know not just the color/
 *  level but WHETHER a blink is live and how many pulses it has left, so
 *  they can swap their static level-driven opacity for a sharp on/off pulse
 *  at the exact same period/count as the tower's own `.well.blink-*`
 *  animations instead of drifting out of phase with a second, independently
 *  guessed cadence. Derived straight from `display.state` (the three blink
 *  variants are exhaustive and already carry their own iteration count in
 *  Tower.svelte's stylesheet — `TOWER_BLINK_COUNT` below is the single place
 *  those counts are mirrored into JS) rather than threaded through as a
 *  separate parameter, so this can never disagree with what `towerStateFor`
 *  already decided. `null`/`0` outside a blink — consumers render exactly as
 *  before then, unchanged by this addition. */
export interface MeterGlowVars {
  /** A CSS color (one of the existing lamp tokens, or `transparent`) — never
   *  a literal hex, so it already tracks the light/dark theme the same way
   *  the tower's own glow does. */
  color: string;
  /** Normalized 0..1 — how much to show, not just whether to. Consumers
   *  (the ambient wash's `color-mix` percentage, the screen reflection's
   *  `opacity`) each apply their own modest ceiling on top of this. */
  level: number;
  /** Non-null while the tower is in a blink event mode — the event kind
   *  driving which of Tower.svelte's `.well.blink-*` classes is live right
   *  now. Consumers only need this to know THAT a blink is live (the color
   *  above is already the right event color); kept as the kind rather than
   *  a bare boolean since it's occasionally useful for debugging/attribute
   *  selectors to see which one. */
  blinkKind: "success" | "run-failed" | "run-stopped" | null;
  /** The blink's `animation-iteration-count` — mirrors Tower.svelte's own
   *  `.well.blink-success`/`.well.blink-failed`/`.well.blink-stopped` counts
   *  (SUCCESS_BLINK_COUNT/TOWER_FAILURE_BLINK_COUNT/STOPPED_BLINK_COUNT)
   *  exactly, so a var-driven layer's pulse count can never drift from the
   *  lamp's own. `0` when `blinkKind` is null (unused then — the tower's
   *  blink is always finite, unlike the board's latched run-failed, so
   *  there's no "infinite" case a var-driven layer here ever needs). */
  blinkCount: number;
}

const TOWER_BLINK_KIND: Partial<Record<TowerState, MeterGlowVars["blinkKind"]>> = {
  "blink-success": "success",
  "blink-run-failed": "run-failed",
  "blink-run-stopped": "run-stopped",
};

const TOWER_BLINK_COUNT: Partial<Record<TowerState, number>> = {
  "blink-success": SUCCESS_BLINK_COUNT,
  "blink-run-failed": TOWER_FAILURE_BLINK_COUNT,
  "blink-run-stopped": STOPPED_BLINK_COUNT,
};

export function towerGlowVars(display: TowerDisplay): MeterGlowVars {
  const blinkKind = TOWER_BLINK_KIND[display.state] ?? null;
  const blinkCount = blinkKind ? (TOWER_BLINK_COUNT[display.state] ?? 0) : 0;
  if (display.color === "gray" || display.color === "none") {
    return { color: "transparent", level: 0, blinkKind: null, blinkCount: 0 };
  }
  const color =
    display.color === "green"
      ? "var(--lamp-green)"
      : display.color === "red"
        ? "var(--lamp-red)"
        : "var(--accent)"; // amber
  return { color, level: display.level, blinkKind, blinkCount };
}
