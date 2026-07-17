import type { CommandInfo, DoctorReport, Readiness } from "./types";

export function readinessFor(
  cmd: CommandInfo,
  trusted: boolean,
  doctorReport: DoctorReport | null,
): Readiness {
  if (!trusted) return "untrusted";
  if (!doctorReport) return "none";
  const entry = doctorReport.commands.find((d) => d.id === cmd.id);
  if (!entry) return "none";
  if (entry.ready === null) return "no-check"; // doctor answered: no `check:` declared
  return entry.ready ? "ready" : "failed";
}

export function readinessLabel(state: Readiness): string {
  switch (state) {
    case "ready":
      return "Ready";
    case "failed":
      return "Check failed";
    case "no-check":
      return "No check declared";
    case "untrusted":
      return "Untrusted";
    case "none":
    default:
      return "Not checked yet";
  }
}

/** The segmented LED meter's state — a command currently running always
 *  shows as "running" regardless of its readiness (matches the design
 *  template's meterFor: in-progress trumps the last known check result).
 *  "no-check" (confirmed `check:` is null) and "none" ("untrusted", or
 *  doctor simply hasn't answered yet) are kept distinct: the former lights
 *  a single neutral segment ("powered, no probe"), the latter stays fully
 *  dark — dark board reads as "nothing known yet", not "broken". */
// Board meter states. `running` covers both determinate and indeterminate
// progress — a caller that has a progress fraction passes it separately (see
// Meter.svelte's `level` prop) rather than that being folded into the state
// enum, since "how much" and "what" are independent per docs/design-
// language.md ("Color = what. Level = how much."). `success`/`run-failed`/
// `stopped` are the board's post-run transient/latch overlay (see
// `BoardMeterOverride` below) — distinct from `failed` (a *standing* check
// failure doctor reports, solid, unrelated to any run) even though both
// render red: `failed` is steady, `run-failed` blinks until acknowledged.
export type MeterState =
  | "running"
  | "ready"
  | "failed"
  | "no-check"
  | "none"
  | "success"
  | "run-failed"
  | "stopped";

export function meterStateFor(readiness: Readiness, running: boolean): MeterState {
  if (running) return "running";
  if (readiness === "ready") return "ready";
  if (readiness === "failed") return "failed";
  if (readiness === "no-check") return "no-check";
  return "none"; // covers both "untrusted" and "doctor hasn't answered yet"
}

/** What the board's post-run overlay is currently showing for one command,
 *  layered on top of its plain readiness — see docs/design-language.md's
 *  "Only failures latch" section. Session-scoped, tracked centrally in
 *  +page.svelte (`boardOverrides`); this file only holds the pure
 *  state-derivation math, not the timers/acknowledgment bookkeeping that
 *  decide when an override is set or cleared. */
export interface BoardMeterOverride {
  kind: "success" | "run-failed" | "stopped";
}

/** The board meter's full state: plain readiness+running (see
 *  `meterStateFor` above), with a run's post-run overlay folded in once it's
 *  no longer running. A run currently in flight always wins over any stale
 *  leftover overlay from a *previous* run of the same command — the overlay
 *  is only ever consulted once `running` is false, which is also why
 *  +page.svelte clears any override the instant a new run starts rather
 *  than relying on this function to hide it. */
export function boardMeterFor(
  readiness: Readiness,
  running: boolean,
  override: BoardMeterOverride | null,
): MeterState {
  if (running) return "running";
  if (override) return override.kind;
  return meterStateFor(readiness, false);
}
