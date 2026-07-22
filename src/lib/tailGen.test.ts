import { describe, expect, it } from "vitest";
import { shouldAcceptEvent } from "./tailGen";

describe("shouldAcceptEvent", () => {
  it("accepts an event with no tail_gen unconditionally, regardless of adopted generation", () => {
    expect(shouldAcceptEvent(null, undefined)).toBe(true);
    expect(shouldAcceptEvent(3, undefined)).toBe(true);
  });

  it("accepts an event carrying tail_gen when nothing has been adopted yet", () => {
    expect(shouldAcceptEvent(null, 1)).toBe(true);
  });

  it("accepts an event whose tail_gen matches the adopted generation", () => {
    expect(shouldAcceptEvent(2, 2)).toBe(true);
  });

  it("drops an event whose tail_gen is a stale generation (a cancelled tail's straggler)", () => {
    expect(shouldAcceptEvent(2, 1)).toBe(false);
  });

  it("drops an event whose tail_gen is somehow ahead of the adopted generation too", () => {
    // Shouldn't happen in practice (tail_start always arrives before any of
    // its own tail's other events), but the fence is symmetric: only an
    // exact match is accepted.
    expect(shouldAcceptEvent(1, 2)).toBe(false);
  });
});
