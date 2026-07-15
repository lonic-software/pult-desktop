import type { CommandInfo, DoctorReport, Readiness } from "./types";

export function readinessFor(
  cmd: CommandInfo,
  trusted: boolean,
  doctorReport: DoctorReport | null,
): Readiness {
  if (!trusted) return "untrusted";
  if (!doctorReport) return "none";
  const entry = doctorReport.commands.find((d) => d.id === cmd.id);
  if (!entry || entry.ready === null) return "none";
  return entry.ready ? "ready" : "failed";
}

export function readinessLabel(state: Readiness): string {
  switch (state) {
    case "ready":
      return "Ready";
    case "failed":
      return "Check failed";
    case "untrusted":
      return "Untrusted";
    case "none":
    default:
      return "No check";
  }
}
