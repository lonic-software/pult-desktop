// Fix round 2's generation-fenced tail restart (src-tauri/src/journal.rs's
// `TailRegistry`): pulled out as pure functions — no Svelte state, no
// `activeTails` — so the fencing decision is unit-testable on its own, same
// pattern as `reconcileDecision` (reconcile.ts).
//
// A tail's very first emission is always `tail_start`, carrying its
// generation; +page.svelte's shared subscription router adopts that as the
// "current" generation for a run_id (stored per-`activeTails`-entry, not on
// the `RunRecord` itself — see `activeTails`'s doc comment) via
// `adoptTailGen`, THEN gates every event — `tail_start` included — through
// `shouldAcceptEvent` before applying it. Doing adoption first, unconditionally,
// on every event (not just inside a `tail_start`-only branch) is what closes
// fix round 3's null-window finding: the two functions are simple enough that
// composing them the same way for every event, with no special-cased branch,
// is itself the fix — a special case is exactly where the old bug hid.
//
// **Invariant (forward-only adoption):** the adopted generation for a run_id
// only ever increases, and a fresh tail's generation is always higher than
// whatever came before (`TailRegistry::claim` bumps by one each time — see
// journal.rs). Given that, "accept an event only if its `tail_gen` equals the
// adopted generation" and "adopt a `tail_start`'s `tail_gen` only if it's
// strictly higher than what's already adopted" compose consistently: a
// straggler from an old, superseded generation can never look newer than the
// generation actually adopted, so it's always correctly dropped, and the
// very first `tail_start` a fresh `activeTails` entry (`gen: null`) ever sees
// is always adopted (null adopts anything) and always accepted (see the
// matrix below) — there is no window where a stamped event can sneak through
// before its own generation's `tail_start` has been adopted, because
// `tail_run` (journal.rs) emits `TailStart` synchronously, before any
// blocking work even spawns (see `tail_run`'s doc comment) — every other
// event for that generation is necessarily produced after it.

import type { RunEvent } from "./types";

/** Decides whether a `tail_start` event bumps the adopted generation for its
 *  run_id. Forward-only: adopts `event.tail_gen` only if `adopted` is `null`
 *  (nothing adopted yet — anything adopts) or `event.tail_gen` is strictly
 *  higher than `adopted` (a genuinely newer generation superseding the one
 *  already adopted). A non-`tail_start` event never changes adoption — only
 *  `tail_start` carries the fence's own version number, and only when it's
 *  actually newer, so a stale/out-of-order `tail_start` (shouldn't happen —
 *  see the module invariant above — but this must not regress into a
 *  downgrade if it somehow did) leaves the adopted generation untouched.
 */
export function adoptTailGen(adopted: number | null, event: RunEvent): number | null {
  if (event.kind !== "tail_start") return adopted;
  if (adopted === null) return event.tail_gen;
  return event.tail_gen > adopted ? event.tail_gen : adopted;
}

/** Whether an event should be applied to the record, given `adoptedGen` —
 *  the generation currently adopted for this run_id (see `adoptTailGen`),
 *  or `null` if none has been adopted yet.
 *
 *  The full decision matrix — `adoptedGen` (`null` | `n`) × the event's
 *  `tail_gen` (absent | `m < n` | `m == n` | `m > n`; `tail_start` carries a
 *  `tail_gen` too, so it's just another value of `m`, not a separate case):
 *
 *  | adoptedGen | event.tail_gen  | accept? | why                                                 |
 *  |------------|-----------------|---------|------------------------------------------------------|
 *  | `null`     | absent          | yes     | gen-less producer (the mock) — nothing to fence       |
 *  | `null`     | `tail_start`, m | yes     | the very first tail_start this run_id will ever see — |
 *  |            |                 |         | `adoptTailGen` (called first) has already adopted it |
 *  | `null`     | non-tail_start, m | no    | fix round 3's closed window: a fence-aware producer   |
 *  |            |                 |         | exists but we haven't seen OUR tail_start yet — this  |
 *  |            |                 |         | is a straggler from a generation that predates us     |
 *  | `n`        | absent          | yes     | gen-less producer — still nothing to fence            |
 *  | `n`        | m < n           | no      | straggler from an old, superseded generation          |
 *  | `n`        | m == n          | yes     | current generation                                    |
 *  | `n`        | m > n           | no      | shouldn't happen (the invariant above — a higher       |
 *  |            |                 |         | generation's own tail_start would already have been   |
 *  |            |                 |         | adopted by `adoptTailGen` before this runs); rejected  |
 *  |            |                 |         | defensively rather than silently accepted              |
 *
 *  Note the `null` row's middle case: `shouldAcceptEvent` doesn't special-case
 *  `tail_start` — it's covered by the SAME `adoptedGen === null` branch as
 *  the "drop a stamped event" row below it, because by the time this
 *  function runs, `adoptTailGen` has already updated `adoptedGen` for a
 *  `tail_start` event (the caller always calls `adoptTailGen` first — see
 *  +page.svelte's router). Tested here in isolation (without that ordering)
 *  the two `null`-row cases must still both come out right on their own,
 *  which is exactly why `adoptedGen === null` alone isn't enough to decide —
 *  the event's `tail_gen` presence is what tells them apart.
 */
export function shouldAcceptEvent(adoptedGen: number | null, event: RunEvent): boolean {
  if (event.tail_gen === undefined) return true;
  if (adoptedGen === null) return event.kind === "tail_start";
  return event.tail_gen === adoptedGen;
}
