// Small formatting helpers shared by +page.svelte (the exit-summary line)
// and RunView (started/elapsed/total, stage-ladder meta) — kept in one place
// so "M:SS" and "HH:MM:SS" mean the same thing everywhere they're shown.

/** `ms` as "M:SS" (no leading zero on minutes, seconds zero-padded) — matches
 *  the design reference's "elapsed 1:14" / "total 2:33" / "done in 2:33". */
export function formatDuration(ms: number): string {
  const totalSeconds = Math.max(0, Math.round(ms / 1000));
  const m = Math.floor(totalSeconds / 60);
  const s = totalSeconds % 60;
  return `${m}:${String(s).padStart(2, "0")}`;
}

/** A local wall-clock timestamp (epoch ms) as "HH:MM:SS" — matches the
 *  design reference's "started 12:04:01" / "finished 12:06:34". */
export function formatClock(ts: number): string {
  return new Date(ts).toLocaleTimeString(undefined, {
    hour12: false,
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

/** `ts` relative to `now` (both epoch ms) as "just now" / "Nm ago" / "Nh
 *  ago" / "Nd ago" — matches the design reference's "last run 2h ago ·
 *  passed" header line, shown once a run has finished and the details page
 *  is revisited *later* (see RunView's `finishedDuringVisit`/standing-
 *  outcome handling — this is deliberately the *other* case: a run from
 *  earlier that isn't standing anymore). Coarse on purpose: a glanceable
 *  "how stale is this" hint, not a precise timestamp (formatClock covers
 *  that for the "started"/"finished"/"stopped" variants). */
export function formatRelative(ts: number, now: number): string {
  const deltaMin = Math.floor(Math.max(0, now - ts) / 60000);
  if (deltaMin < 1) return "just now";
  if (deltaMin < 60) return `${deltaMin}m ago`;
  const deltaHr = Math.floor(deltaMin / 60);
  if (deltaHr < 24) return `${deltaHr}h ago`;
  const deltaDay = Math.floor(deltaHr / 24);
  return `${deltaDay}d ago`;
}
