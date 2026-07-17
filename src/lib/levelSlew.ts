// Shared "needle physics" for every LED column's lit-segment count — see
// docs/design-language.md's "Levels slew" principle: a meter's level never
// jumps. Any change to what's lit — ready -> running, a progress jump,
// run-end -> readiness recovery, a doctor refresh, no-check <-> ready, even
// the very first mount — animates segment-by-segment through every
// intermediate value, in both directions. Board Meter.svelte (5 segments)
// and the details page's Tower.svelte (28 segments) each hold a local
// `displayedLitCount` and drive it toward their own computed target with a
// `setTimeout` chain, re-armed one step at a time via `stepToward`, at a
// cadence this module plans for them (`planChase`/`slewStepDelay` below)
// rather than a flat per-caller constant — see that section for the model.
//
// Deliberately just this — no setTimeout, no DOM, no Svelte runes — so the
// chase decision itself (which direction, when to stop, how fast) is
// unit-testable apart from whatever schedules it. Segment *color* is never
// this module's concern: each caller's own `.seg` keeps its existing
// `background-color` transition, which fires independently whenever a
// segment's lit class changes — riding that existing transition alongside
// this chase's stepped reveal/dismissal is what makes "climb down through
// green while turning amber" fall out for free (see Meter.svelte's
// script-block comment) rather than needing to be built as its own effect.
//
// A blink (see meterLiveness.ts's `blinkTiming`/docs/design-language.md's
// "Blink is a mode") is the one case that does NOT run through this chase:
// while blinking, a caller freezes `displayedLitCount` wherever it already
// was and renders the full column directly instead (see Meter.svelte's
// `isBlinking`/`litCount`) — which is also exactly why recovery afterward
// reads as "slew from wherever the level was," per the design doc: nothing
// ever reset it mid-blink.

/** One step of a segment-count chase toward `target`, moving at most one
 *  segment per call. The caller drives the cadence; this only decides
 *  direction and stops exactly at `target` rather than overshooting. */
export function stepToward(current: number, target: number): number {
  if (current === target) return current;
  return current < target ? current + 1 : current - 1;
}

// --- Cadence: duration-bounded, not flat -----------------------------------
//
// The original model was a flat 25ms/segment for every chase, board and
// tower alike. That's the right feel for a short slew — ready dropping a
// couple of segments to running's floor, a progress tick — but a flat
// per-segment rate scales linearly with distance, so a full-scale sweep
// gets slower in direct proportion to how many segments it crosses: the
// tower's 28 segments at a flat 25ms/step meant a ~700ms worst-case
// full-scale sweep (versus the board's 5 segments x 25ms = a 125ms worst
// case — proportionate, but it meant the tower's longest sweeps, most
// visibly the dim -> ready mount climb, no longer felt like the same
// needle as the board's, just slower). Feel-tested as too slow for that
// climb and for long transitions generally — the fix is to bound the
// SWEEP's total duration, not the per-segment step.
//
// `BASE_MS` keeps the short-slew feel exactly as it was: for a small
// distance the step still costs 25ms/segment, so a couple-of-segments nudge
// (a progress tick, ready <-> running's floor) reads identically to before.
// `CAP_MS` bounds the other end — past some distance, holding 25ms/segment
// would push the total sweep past a comfortable ceiling, so the per-step
// delay instead shrinks to keep the whole sweep inside `CAP_MS`:
// `slewStepDelay` below is just `min(BASE_MS, CAP_MS / distance)`. The two
// terms cross at `distance = CAP_MS / BASE_MS` (14 segments at these
// numbers) — below that, `BASE_MS` wins and the step holds at 25ms; at or
// past it, `CAP_MS` wins and the step shrinks so `distance * step ≈ CAP_MS`.
//
// The tower's 28-segment full-scale sweep (well past the 14-segment
// crossover) now lands at 350ms / 28 ≈ 12.5ms/step — about half the old
// flat cadence's ~700ms worst case, which is the ~2x the dim -> ready mount
// climb (also a long sweep: readiness's 80% level is 22 of 28 segments)
// needed to feel right. The board's 5-segment full sweep never reaches the
// 14-segment crossover at all — its worst case, 5 segments, is still short
// of the 14 where `CAP_MS` would start to bind (5 x 25ms = a 125ms worst
// case, comfortably under `CAP_MS` on its own) — so `slewStepDelay` needs
// no board/tower special case: the identical `min(BASE_MS, CAP_MS /
// distance)` produces the old flat-25ms board feel and the new capped tower
// feel from the same formula, purely because their distances sit on
// opposite sides of the crossover.
//
// Symmetric in both directions, same as `stepToward` above: the cadence
// depends only on `distance`, an absolute value, so a long fall (e.g.
// check-failed's 100% collapsing to a fresh run's low starting progress)
// sweeps exactly as fast as an equally long rise — needle physics, not a
// gauge that eases down and snaps up.
//
// This still reads as a distinct kind of motion from a blink (meterLiveness
// .ts's `BLINK_PERIOD_MS`, 400ms, `steps(1,end)` — see docs/design-
// language.md's "Blink is a mode"/"Flicker is texture; blink is signal"):
// the old argument was "the slew stays slower than the blink's on/off", but
// that's no longer true in the small — a capped tower sweep's 12.5ms step is
// faster than a 200ms blink half-cycle. What still keeps the two
// unmistakable is shape, not just speed: a slew is many small steps
// climbing through every intermediate segment count over the *whole*
// sweep (≤350ms even at its fastest, still comfortably slower than a single
// blink half-cycle), while a blink is one sharp, binary flip of the entire
// column with no intermediate values at all. Different motions, not just
// different speeds — they were never going to be confused for each other.
export const BASE_MS = 25;
export const CAP_MS = 350;

/** Per-step delay for a chase covering `distance` segments — see the
 *  cadence section above. `distance <= 0` (a same-position "chase" —
 *  callers never actually schedule one, they stop at `current === target`)
 *  falls back to `BASE_MS` rather than dividing by zero or going negative. */
export function slewStepDelay(distance: number): number {
  if (distance <= 0) return BASE_MS;
  return Math.min(BASE_MS, CAP_MS / distance);
}

/** A chase's cadence, planned once per leg (see `planChase` below) rather
 *  than recomputed every step. */
export interface ChasePlan {
  /** The target this cadence was planned for. `planChase` compares against
   *  this, not against `current`, to tell "still the same leg, keep
   *  chasing at the same rate" apart from "the target moved, this is a
   *  retarget, replan." */
  target: number;
  delayMs: number;
}

/** (Re)plans a chase's cadence. Call this every time the caller's own effect
 *  runs — i.e. every step of an in-flight chase, not just its first — and
 *  use the `delayMs` it returns to schedule the next step. It only actually
 *  recomputes when `target` has changed since `prev` was planned: every
 *  other call (each step of an already-running leg toward the same target)
 *  returns `prev` unchanged, which is what holds a leg's cadence constant
 *  for its whole sweep instead of drifting as its own progress eats into
 *  the remaining distance — recomputing from the *shrinking* remaining
 *  distance every step would make the step slow down over the course of a
 *  single sweep, the opposite of a bounded total duration.
 *
 *  A genuine retarget — the target changes again before the previous one
 *  was reached, e.g. progress ticking upward again mid-slew — re-derives
 *  the distance from `current`'s position right now, per "the remaining
 *  distance defines a new cadence": a long chase that gets retargeted close
 *  by slows back down to the short-slew feel; one retargeted far away
 *  speeds up to fit the new distance inside the same `CAP_MS`. Either way
 *  nothing strands a stale cadence — a fast plan from a long-since-passed
 *  leg can't survive being reused for a now-much-shorter remaining
 *  distance, and vice versa, because a `target` mismatch always forces a
 *  fresh `slewStepDelay` call keyed off where the chase actually is *now*.
 *  `prev` is `null` for a chase's very first leg (mount, or the first
 *  change out of a resting state) — there's no earlier plan to compare
 *  against, so this always recomputes for it. */
export function planChase(current: number, target: number, prev: ChasePlan | null): ChasePlan {
  if (prev && prev.target === target) return prev;
  return { target, delayMs: slewStepDelay(Math.abs(target - current)) };
}

/** Whether the user has asked for reduced motion — checked live (not
 *  cached) since it's cheap and can change mid-session. Every caller that
 *  drives a JS-timed chase (as opposed to a CSS transition/animation, which
 *  global.css already collapses via `prefers-reduced-motion`) has to ask
 *  this explicitly: a `setTimeout` chain doesn't read CSS media queries on
 *  its own. Per docs/design-language.md's "Levels slew" principle, reduced
 *  motion's answer is to snap straight to the target instead of chasing it.
 *  `matchMedia` is guarded for non-browser contexts (SSR/tests) the same
 *  way the rest of this app assumes a DOM is present at runtime elsewhere. */
export function prefersReducedMotion(): boolean {
  return (
    typeof window !== "undefined" &&
    typeof window.matchMedia === "function" &&
    window.matchMedia("(prefers-reduced-motion: reduce)").matches
  );
}
