import { describe, expect, it } from "vitest";
import { reconcileDecision } from "./reconcile";
import type { RunRecord, RunSummary } from "./types";

function record(overrides: Partial<RunRecord> = {}): RunRecord {
  return {
    runId: "run-current",
    running: true,
    lines: [],
    step: null,
    stepHistory: [],
    progress: null,
    status: null,
    stopped: false,
    crashed: false,
    exitCode: null,
    startedAt: Date.parse("2026-07-22T12:00:00.000Z"),
    endedAt: null,
    interactive: false,
    ...overrides,
  };
}

function summary(overrides: Partial<RunSummary> = {}): RunSummary {
  return {
    run_id: "run-summary",
    command_id: "deploy",
    command_title: "Deploy",
    status: "running",
    exit_code: null,
    started_at: "2026-07-22T12:00:00.000Z",
    ended_at: null,
    origin: "app",
    interactive: false,
    ...overrides,
  };
}

describe("reconcileDecision", () => {
  it("seeds when no record exists yet for the command", () => {
    expect(reconcileDecision(undefined, summary())).toEqual({ action: "seed" });
  });

  it("refreshes when the record already holds this exact run_id", () => {
    const current = record({ runId: "run-a" });
    expect(reconcileDecision(current, summary({ run_id: "run-a" }))).toEqual({
      action: "refresh",
    });
  });

  it("skips a different run_id whose start time is no later than the live record's — stale poll response", () => {
    const current = record({
      runId: "run-live",
      running: true,
      startedAt: Date.parse("2026-07-22T12:05:00.000Z"),
    });
    const stale = summary({
      run_id: "run-old",
      started_at: "2026-07-22T12:00:00.000Z", // earlier than the live run
    });
    expect(reconcileDecision(current, stale)).toEqual({ action: "skip" });
  });

  it("skips a different run_id with the exact same start time as the live record (boundary: <=)", () => {
    const startedAt = Date.parse("2026-07-22T12:05:00.000Z");
    const current = record({ runId: "run-live", running: true, startedAt });
    const same = summary({ run_id: "run-old", started_at: new Date(startedAt).toISOString() });
    expect(reconcileDecision(current, same)).toEqual({ action: "skip" });
  });

  it("reseeds when a different run_id is genuinely newer than the live record", () => {
    const current = record({
      runId: "run-live",
      running: true,
      startedAt: Date.parse("2026-07-22T12:00:00.000Z"),
    });
    const newer = summary({
      run_id: "run-new",
      started_at: "2026-07-22T12:10:00.000Z", // later than the live run
    });
    expect(reconcileDecision(current, newer)).toEqual({ action: "reseed" });
  });

  it("reseeds a different run_id even if older, when the held record isn't running", () => {
    const current = record({
      runId: "run-finished",
      running: false,
      startedAt: Date.parse("2026-07-22T12:05:00.000Z"),
    });
    const olderButSuperseding = summary({
      run_id: "run-new",
      started_at: "2026-07-22T12:00:00.000Z",
    });
    expect(reconcileDecision(current, olderButSuperseding)).toEqual({ action: "reseed" });
  });

  // Fix round 3's NaN-safe recency guard: `Date.parse` on an unparseable
  // `started_at` returns `NaN`, and `NaN <= current.startedAt` is always
  // `false` in JS — without the explicit guard, an unparseable summary would
  // fall straight through to "reseed", stomping a live record with garbage.
  it("skips a different run_id with an unparseable started_at while the held record is running", () => {
    const current = record({ runId: "run-live", running: true });
    const garbled = summary({ run_id: "run-old", started_at: "not-a-date" });
    expect(reconcileDecision(current, garbled)).toEqual({ action: "skip" });
  });

  it("reseeds a different run_id with an unparseable started_at when the held record isn't running", () => {
    // A dead record has nothing left to protect — the summary (garbled
    // timestamp and all) is the only information left about this command's
    // newest run.
    const current = record({ runId: "run-finished", running: false });
    const garbled = summary({ run_id: "run-new", started_at: "not-a-date" });
    expect(reconcileDecision(current, garbled)).toEqual({ action: "reseed" });
  });
});
