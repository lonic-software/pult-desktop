// Shared analog-liveness math for every LED column in the app (the board's
// Meter.svelte, the details page's Tower.svelte) — see Meter.svelte's
// script-block comments for the full rationale on the two-loop fast-
// texture/slow-event split this feeds (tip flicker vs. level dip/glimmer).
// Deliberately JS-only: each caller still owns its own CSS keyframes,
// segment count, and color mapping (they differ enough — 5 vs. 28 segments,
// different state vocabularies — that sharing markup would mean branching
// most of a shared component on "which meter is this"), but the seeded-
// desync numbers themselves are identical math and belong in exactly one
// place rather than being copy-pasted per component.
//
// Level *slewing* (the chase toward a target lit-segment count — docs/
// design-language.md's "Levels slew") is a sibling concern, not this file's:
// see levelSlew.ts for that pure timing math. This file is only the ambient
// texture (flicker/level-dip/glimmer) plus the blink cadence below.

/** Deterministic per-card desync: a tiny string hash (no shared PRNG state,
 *  stable across re-renders) — see Meter.svelte's original comment for why
 *  this beats a shared clock (cards never flicker in lockstep, no shared
 *  mutable state to coordinate). */
function seedHash(s: string, salt: number): number {
  let h = salt >>> 0;
  for (let i = 0; i < s.length; i++) {
    h = (Math.imul(h, 31) + s.charCodeAt(i)) >>> 0;
  }
  return h;
}

const FLICKER_MIN_MS = 450;
const FLICKER_RANGE_MS = 350; // 0.45s-0.8s period
const FLICKER_SETTLE_MS = 320;
const FLICKER_SETTLE_JITTER_MS = 150;
const LEVEL_MIN_MS = 2800;
const LEVEL_RANGE_MS = 2000; // 2.8s-4.8s period

export interface LivenessTiming {
  flickerDuration: number;
  flickerDelay: number;
  levelDuration: number;
  levelDelay: number;
  levelDelay2: number;
}

/** Every seeded duration/delay a lit LED column needs for the ambient tip-
 *  flicker/level-dip/overshoot-glimmer system, derived from a command id —
 *  see Meter.svelte for what each one drives on the segment/well elements. */
export function livenessTiming(seed: string): LivenessTiming {
  const flickerDuration = FLICKER_MIN_MS + (seedHash(seed, 0x9e3779b1) % FLICKER_RANGE_MS);
  const flickerDelay =
    FLICKER_SETTLE_MS + (seedHash(seed, 0x85ebca77) % FLICKER_SETTLE_JITTER_MS);
  const levelDuration = LEVEL_MIN_MS + (seedHash(seed, 0x27d4eb2f) % LEVEL_RANGE_MS);
  const levelDelay = -(seedHash(seed, 0x165667b1) % levelDuration);
  const levelDelay2 = -(seedHash(seed, 0xd3a2646c) % levelDuration);
  return { flickerDuration, flickerDelay, levelDuration, levelDelay, levelDelay2 };
}

// Blink is a mode, not texture (docs/design-language.md's "Blink is a mode:
// flicker OFF, full column fully-dim <-> fully-lit"): success, failure, and
// stopped all suspend the ambient flicker/level system above entirely and
// alternate the *whole* column, sharply (steps, no easing), between fully
// dim and fully lit in the event color — never just the tip, never a fade.
// `BLINK_PERIOD_MS` is the shared ~2.5Hz cadence every blink (board and
// tower alike) animates at; each call site pairs it with its own
// `animation-iteration-count` (see the *_BLINK_COUNT constants below) to
// get its specific number of blinks — CSS can't import these constants
// directly, so the animation-iteration-count baked into each component's
// stylesheet must be kept in sync with the count used here to schedule the
// JS-side recovery (see +page.svelte's `SUCCESS_PULSE_MS`/`STOP_FLASH_MS`
// and RunView.svelte's tower-blink timers, both computed from these).
export const BLINK_PERIOD_MS = 400; // ~2.5Hz — sharp, no easing (steps(1,end))

// Success: a few short blinks that self-clear. Stopped: pre-acknowledged,
// briefer still so it never reads as an alarm (see docs/design-language.md's
// "A user stop is pre-acknowledged"). Both board and tower use the same
// counts so the two surfaces feel like the same instrument at different
// scale, per the task's "keep tower and board blinks visually consistent."
export const SUCCESS_BLINK_COUNT = 3; // 3 * 400ms = 1200ms total
export const STOPPED_BLINK_COUNT = 2; // 2 * 400ms = 800ms total

// The tower's failure blink is NOT latched (see tower.ts's doc comment —
// being on the command's own page during a failure counts as having seen
// it), so it needs its own finite count instead of the board's infinite
// latch below. Slightly more insistent than success (5 vs. 3 blinks) since
// this is the one surface where a few extra blinks *are* the acknowledgment
// mechanism, standing in for the board's latch.
export const TOWER_FAILURE_BLINK_COUNT = 5; // 5 * 400ms = 2000ms total

// Board-only cadence, deliberately NOT part of the shared timing above: the
// run-failed blink (Meter.svelte only — the tower's failure blink is finite,
// see TOWER_FAILURE_BLINK_COUNT) is the one blink that never stops on its
// own — it latches until acknowledged (opening/being on the command's
// details page, or starting a new run — see +page.svelte). Still lightly
// seeded per card so multiple failed cards don't blink in perfect lockstep,
// but close enough together that the "alarm" cadence reads consistently
// across the board — texture is allowed to vary card to card, a signal
// shouldn't.
const BLINK_LATCH_MIN_MS = 550;
const BLINK_LATCH_RANGE_MS = 150;

export interface BlinkTiming {
  blinkDuration: number;
  blinkDelay: number;
}

/** Seeded cadence for the board's latched run-failed blink — see
 *  `BLINK_LATCH_MIN_MS`/`BLINK_LATCH_RANGE_MS` above. Every other blink
 *  (success, stopped, the tower's failure blink) is finite and unseeded —
 *  there's only ever one of them live at a time (a self-clearing overlay,
 *  or the single on-page tower), so there's no lockstep to break up. */
export function blinkTiming(seed: string): BlinkTiming {
  const blinkDuration = BLINK_LATCH_MIN_MS + (seedHash(seed, 0x1b873593) % BLINK_LATCH_RANGE_MS);
  const blinkDelay = seedHash(seed, 0xcc9e2d51) % blinkDuration;
  return { blinkDuration, blinkDelay };
}
