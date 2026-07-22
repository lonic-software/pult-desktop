import { describe, expect, it } from "vitest";
import { adoptTailGen, shouldAcceptEvent } from "./tailGen";
import type { RunEvent } from "./types";

function line(tailGen?: number): RunEvent {
  return { kind: "line", run_id: "r1", stream: "stdout", text: "hi", tail_gen: tailGen };
}

function exit(tailGen?: number): RunEvent {
  return { kind: "exit", run_id: "r1", code: 0, stopped: false, tail_gen: tailGen };
}

function tailStart(tailGen: number): RunEvent {
  return { kind: "tail_start", run_id: "r1", tail_gen: tailGen };
}

describe("adoptTailGen", () => {
  it("adopts a tail_start's generation when nothing is adopted yet", () => {
    expect(adoptTailGen(null, tailStart(1))).toBe(1);
  });

  it("adopts a strictly higher tail_start generation", () => {
    expect(adoptTailGen(1, tailStart(2))).toBe(2);
  });

  it("does not adopt a tail_start at or below the current generation (forward-only)", () => {
    expect(adoptTailGen(2, tailStart(2))).toBe(2);
    expect(adoptTailGen(2, tailStart(1))).toBe(2);
  });

  it("never changes adoption for a non-tail_start event, gen-less or stamped", () => {
    expect(adoptTailGen(null, line())).toBe(null);
    expect(adoptTailGen(null, line(1))).toBe(null);
    expect(adoptTailGen(2, line(2))).toBe(2);
    expect(adoptTailGen(2, exit(5))).toBe(2);
  });
});

// The full (adoptedGen null|n) x (event tail_gen absent|m<n|m=n|m>n|tail_start)
// matrix from tailGen.ts's doc comment — every cell explicit.
describe("shouldAcceptEvent", () => {
  it("null adopted, gen-less event: accept (nothing to fence — the mock)", () => {
    expect(shouldAcceptEvent(null, line())).toBe(true);
    expect(shouldAcceptEvent(null, exit())).toBe(true);
  });

  it("null adopted, tail_start: accept (the very first tail_start this run_id sees)", () => {
    expect(shouldAcceptEvent(null, tailStart(1))).toBe(true);
    expect(shouldAcceptEvent(null, tailStart(7))).toBe(true);
  });

  it("null adopted, stamped non-tail_start event: drop (revert-check — see below)", () => {
    expect(shouldAcceptEvent(null, line(1))).toBe(false);
    expect(shouldAcceptEvent(null, exit(1))).toBe(false);
  });

  it("n adopted, gen-less event: accept unconditionally", () => {
    expect(shouldAcceptEvent(2, line())).toBe(true);
    expect(shouldAcceptEvent(2, exit())).toBe(true);
  });

  it("n adopted, event gen below n: drop (straggler from a superseded generation)", () => {
    expect(shouldAcceptEvent(2, line(1))).toBe(false);
    expect(shouldAcceptEvent(2, exit(1))).toBe(false);
    expect(shouldAcceptEvent(2, tailStart(1))).toBe(false);
  });

  it("n adopted, event gen equal to n: accept (current generation)", () => {
    expect(shouldAcceptEvent(2, line(2))).toBe(true);
    expect(shouldAcceptEvent(2, exit(2))).toBe(true);
    expect(shouldAcceptEvent(2, tailStart(2))).toBe(true);
  });

  it("n adopted, event gen above n: drop defensively (shouldn't happen — adoptTailGen runs first in composed use)", () => {
    expect(shouldAcceptEvent(1, line(2))).toBe(false);
    expect(shouldAcceptEvent(1, exit(2))).toBe(false);
    expect(shouldAcceptEvent(1, tailStart(2))).toBe(false);
  });

  // Revert-check (fix round 3's actual regression target): under the OLD
  // rule ("adopted === null -> accept unconditionally, regardless of the
  // event's own tail_gen"), a stale STAMPED exit from a since-cancelled
  // generation arriving before this run_id's own tail_start was wrongly
  // accepted and would finish the record. This test fails if
  // `shouldAcceptEvent`'s null-adopted branch reverts to that old
  // unconditional-accept rule — it is exactly the scenario the fix closes.
  it("REVERT-CHECK: drops a stale stamped Exit arriving before this run_id's own tail_start", () => {
    const staleExitFromCancelledGeneration = exit(1);
    expect(shouldAcceptEvent(null, staleExitFromCancelledGeneration)).toBe(false);
  });
});
