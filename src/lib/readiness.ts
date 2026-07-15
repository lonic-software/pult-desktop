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
export type MeterState = "running" | "ready" | "failed" | "no-check" | "none";

export function meterStateFor(readiness: Readiness, running: boolean): MeterState {
  if (running) return "running";
  if (readiness === "ready") return "ready";
  if (readiness === "failed") return "failed";
  if (readiness === "no-check") return "no-check";
  return "none"; // covers both "untrusted" and "doctor hasn't answered yet"
}
