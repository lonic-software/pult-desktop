// Fix round 2's generation-fenced tail restart (src-tauri/src/journal.rs's
// `TailRegistry`): pulled out as a pure function — no Svelte state, no
// `activeTails` — so the fencing decision is unit-testable on its own, same
// pattern as `reconcileDecision` (reconcile.ts).
//
// A tail's very first emission is always `tail_start`, carrying its
// generation; +page.svelte's shared subscription router adopts that as the
// "current" generation for a run_id (stored per-`activeTails`-entry, not on
// the `RunRecord` itself — see `activeTails`'s doc comment). Every
// subsequent event for that run_id is checked against it here before being
// applied.

/** Whether an event carrying (optional) `tail_gen` should be accepted, given
 *  `adoptedGen` — the generation the run_id's `tail_start` last set, or
 *  `null` if none has arrived yet (a run_id not currently associated with
 *  any tail, or a producer — the mock backend — that never emits
 *  `tail_start` at all).
 *
 *  - No `tail_gen` on the event at all: accepted unconditionally. Nothing to
 *    fence against — older/other producers, and every mock-backend event.
 *  - No adopted generation yet: accepted unconditionally too — there's
 *    nothing to compare against (shouldn't happen for a real tail, whose
 *    first emission is always its own `tail_start` setting this before
 *    anything else arrives, but a router must not silently drop events over
 *    a bookkeeping gap it didn't cause).
 *  - Otherwise: accepted only if the event's `tail_gen` matches — a mismatch
 *    is a straggler from a since-cancelled, superseded tail and must be
 *    dropped, never applied to the record.
 */
export function shouldAcceptEvent(
  adoptedGen: number | null,
  eventTailGen: number | undefined,
): boolean {
  if (eventTailGen === undefined) return true;
  if (adoptedGen === null) return true;
  return eventTailGen === adoptedGen;
}
