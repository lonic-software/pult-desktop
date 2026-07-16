<script lang="ts">
  import type { MeterState } from "../readiness";

  interface Props {
    state: MeterState;
    size?: "sm" | "lg";
    staggerDelay?: string;
    /** Command id — seeds the tip-flicker's phase/period deterministically
     *  so it's stable across re-renders and desynced across cards without
     *  any shared clock. Optional only so the component still works if a
     *  caller doesn't have an id handy; every real caller passes one. */
    seed?: string;
  }

  let { state, size = "sm", staggerDelay = "0ms", seed = "" }: Props = $props();

  // Segments lit bottom-up per the design template's meterFor: running (in
  // progress right now) always wins and shows 3 amber regardless of
  // readiness; otherwise ready shows 4 green, a failing check shows all 5
  // red. "no-check" (doctor confirmed there's no `check:` to run) lights
  // exactly one neutral segment — "powered, no probe" — while "none"
  // (untrusted, or doctor hasn't answered yet) stays fully dark.
  const LIT_COUNT: Record<MeterState, number> = {
    running: 3,
    ready: 4,
    failed: 5,
    "no-check": 1,
    none: 0,
  };
  const litCount = $derived(LIT_COUNT[state]);
  const segments = $derived(Array.from({ length: 5 }, (_, i) => i < litCount));

  // Tip flicker: only the top-most lit segment of a *steady* readiness
  // (ready/failed) wobbles — analog current isn't perfectly constant.
  // Running has its own sweep animation already; the single no-check
  // segment means "no signal," so it stays put. Index is bottom-up (see
  // `segments` above), so the top lit segment is always `litCount - 1`.
  // LEDs are fixed hardware — nothing ever translates — so the "level
  // wobbles up and down" impression is built from two independent loops
  // layered on this tip segment (and its neighbors), not one. A fast loop
  // (`.seg.tip-flicker`'s own opacity/filter keyframes, `--flicker-*`
  // below) is pure signal-noise texture — sub-second shimmer with no
  // narrative. A slower loop (`--level-*` below) carries the actual
  // "level" events: the tip occasionally sags (see the tip's dip overlay)
  // in the same instant the segment directly below it dims a touch too
  // (see `subTipIndex`/`.seg.sub-tip`), so the top of the column reads as
  // sagging together — and, for `ready` only, the unlit segment above the
  // tip catches a decorrelated overshoot glimmer (see `overshootIndex`/
  // `.seg.overshoot` below). Two loops at two different rates read as
  // analog drift; cramming both into one sub-second cycle is what made the
  // glimmer read as a mechanical ~2Hz beat instead. Neither loop ever
  // changes which count of segments reads as lit.
  const flickerTipIndex = $derived(
    state === "ready" || state === "failed" ? litCount - 1 : -1,
  );

  // The segment directly below the tip — where the slow loop's dip (see
  // `--level-*` below) shows a faint sympathetic dim so the sag reads as
  // the *column's* level settling, not a single flickering pixel. Bottom-
  // up indexing (see `segments` above) means this is always the segment
  // one below the tip; `flickerTipIndex` is only ever -1, or >=2 (litCount
  // is 4 or 5 for the only states that set it), so no extra guard is
  // needed beyond mirroring flickerTipIndex's own -1-when-inapplicable.
  const subTipIndex = $derived(flickerTipIndex >= 0 ? flickerTipIndex - 1 : -1);

  // Deterministic per-card desync: a tiny string hash (no shared PRNG
  // state, stable across re-renders) picks a period in 0.45-0.8s and a
  // phase within that period from the command id, so cards never flicker
  // in lockstep. `FLICKER_SETTLE_MS` is a fixed head start added on top —
  // it keeps the animation's `animation-delay` comfortably past the 200ms
  // illumination transition (see the `.well` rule below) so the one-time
  // fade-in and the infinite flicker loop never fight over the same
  // box-shadow property.
  function seedHash(s: string, salt: number): number {
    let h = salt >>> 0;
    for (let i = 0; i < s.length; i++) {
      h = (Math.imul(h, 31) + s.charCodeAt(i)) >>> 0;
    }
    return h;
  }
  const FLICKER_MIN_MS = 450;
  const FLICKER_RANGE_MS = 350; // 0.45s-0.8s period
  const FLICKER_SETTLE_MS = 260;
  const flickerDuration = $derived(FLICKER_MIN_MS + (seedHash(seed, 0x9e3779b1) % FLICKER_RANGE_MS));
  const flickerDelay = $derived(FLICKER_SETTLE_MS + (seedHash(seed, 0x85ebca77) % flickerDuration));

  // Level events: a second, much slower seeded cycle (2.8-4.8s) carries
  // the dip/glimmer "events" — same seedHash scheme, new salts, so this
  // cycle's period and phase are independent of the fast texture loop
  // above (no relationship between how fast the LED shimmers and when the
  // level next sags or overshoots). A sub-second period was the whole
  // problem with the old single-loop overshoot: two peaks packed into
  // 0.45-0.8s reads as an obvious ~2Hz beat no matter how the stops are
  // placed. A period this much longer than the fast loop, with uneven
  // (non-equidistant) keyframe gaps on top, is what actually reads as
  // random rather than looped.
  const LEVEL_MIN_MS = 2800;
  const LEVEL_RANGE_MS = 2000; // 2.8s-4.8s period
  const levelDuration = $derived(LEVEL_MIN_MS + (seedHash(seed, 0x27d4eb2f) % LEVEL_RANGE_MS));
  // Dip delay: shared verbatim by the tip's dip overlay and the sub-tip's
  // dim overlay (see `.seg.tip-flicker::after` / `.seg.sub-tip::after`
  // below) — they need to land in the exact same instant for the sag to
  // read as one column-level event rather than two coincidentally-timed
  // ones, so there is deliberately only one delay for both.
  const levelDelay = $derived(FLICKER_SETTLE_MS + (seedHash(seed, 0x165667b1) % levelDuration));
  // The overshoot glimmer (see `.seg.overshoot` below) rides the same
  // --level-duration as the dip so they share one slow signal, but gets
  // its own seeded delay on that duration — peaking in lockstep with the
  // dip looked like a mechanical seesaw rather than analog overshoot (the
  // same reason the old fast-loop version used a second delay), so a
  // second, independently-seeded phase decorrelates the two.
  const levelDelay2 = $derived(FLICKER_SETTLE_MS + (seedHash(seed, 0xd3a2646c) % levelDuration));

  // `ready` is the only steady state with a segment above its tip to
  // glimmer into (failed's tip is already the top of the 5-segment well —
  // see the LIT_COUNT/flickerTipIndex comments above). Bottom-up indexing
  // means that segment is exactly `litCount`.
  const overshootIndex = $derived(state === "ready" ? litCount : -1);
</script>

<div
  class="well {size} glow-{state}"
  style="--meter-delay: {staggerDelay}; --flicker-duration: {flickerDuration}ms; --flicker-delay: {flickerDelay}ms; --level-duration: {levelDuration}ms; --level-delay: {levelDelay}ms; --level-delay-2: {levelDelay2}ms"
  aria-hidden="true"
>
  <div class="segments">
    {#each segments as lit, i (i)}
      <span
        class="seg"
        class:lit-running={lit && state === "running"}
        class:lit-ready={lit && state === "ready"}
        class:lit-failed={lit && state === "failed"}
        class:lit-no-check={lit && state === "no-check"}
        class:tip-flicker={i === flickerTipIndex}
        class:sub-tip={i === subTipIndex}
        class:overshoot={i === overshootIndex}
      ></span>
    {/each}
  </div>
</div>

<style>
  /* The recessed LED well: segments sit in a dark slot cut into the pad.
     The well's chrome (background, inset shadow) renders immediately with
     the card — it never animates, so a doctor-latency gap reads as dark
     hardware awaiting power, not missing UI. Only the *illumination*
     transitions: box-shadow (for the glow) and each segment's
     background-color are the only animated properties, and only fire when
     their computed value actually changes — a fresh mount already has the
     right value painted at first frame, so nothing animates in from
     nothing. The glow is expressed as a second, always-present box-shadow
     layer (transparent when unlit) rather than conditionally adding/
     removing a shadow layer, so the two-layer value can interpolate
     smoothly instead of popping. */
  .well {
    flex: none;
    padding: 4px 3px;
    border-radius: 3px;
    background: var(--led-well);
    box-shadow:
      inset 0 1px 3px var(--well-inset, rgba(0, 0, 0, 0.75)),
      0 0 11px -1px var(--glow, transparent);
    transition: box-shadow 200ms ease;
    transition-delay: var(--meter-delay, 0ms);
  }

  .well.lg {
    padding: 5px 4px;
    border-radius: 4px;
  }

  .well.glow-running {
    --glow: color-mix(in srgb, var(--accent) 70%, transparent);
  }

  .well.glow-ready {
    --glow: color-mix(in srgb, var(--lamp-green) 65%, transparent);
  }

  .well.glow-failed {
    --glow: color-mix(in srgb, var(--lamp-red) 65%, transparent);
  }

  /* Tip flicker, part 1: the glow carries a faint ripple (11-14px blur on
     the same shadow layer built above — never dips below ~79% of its peak,
     so the halo stays clearly visible at every frame) in steady states
     only, phase-locked to the tip segment's opacity/brightness noise below.
     Nine uneven keyframe stops plus `steps(1, end)` (a held value that
     jumps to the next, no easing) read as electrical noise rather than a
     pulse — at a sub-second period a smooth ease would still look like a
     swell. `animation-delay` (see --flicker-delay above) is always positive
     and comfortably past the 200ms illumination transition, so the
     animation only takes over box-shadow *after* that one-time fade-in has
     already finished — avoiding a fight between the transition and this
     infinite loop over the same property. prefers-reduced-motion already
     collapses all animation-duration to ~0 globally (see global.css), so
     this is inert there without any extra guard. */
  .well.glow-ready,
  .well.glow-failed {
    animation: lamp-breathe var(--flicker-duration, 600ms) steps(1, end) infinite;
    animation-delay: var(--flicker-delay, 0ms);
  }

  @keyframes lamp-breathe {
    0%,
    100% {
      box-shadow:
        inset 0 1px 3px var(--well-inset, rgba(0, 0, 0, 0.75)),
        0 0 14px 1px var(--glow, transparent);
    }
    13% {
      box-shadow:
        inset 0 1px 3px var(--well-inset, rgba(0, 0, 0, 0.75)),
        0 0 12px 0px var(--glow, transparent);
    }
    27% {
      box-shadow:
        inset 0 1px 3px var(--well-inset, rgba(0, 0, 0, 0.75)),
        0 0 13px 1px var(--glow, transparent);
    }
    38% {
      box-shadow:
        inset 0 1px 3px var(--well-inset, rgba(0, 0, 0, 0.75)),
        0 0 11px 0px var(--glow, transparent);
    }
    54% {
      box-shadow:
        inset 0 1px 3px var(--well-inset, rgba(0, 0, 0, 0.75)),
        0 0 14px 1px var(--glow, transparent);
    }
    63% {
      box-shadow:
        inset 0 1px 3px var(--well-inset, rgba(0, 0, 0, 0.75)),
        0 0 12px 0px var(--glow, transparent);
    }
    78% {
      box-shadow:
        inset 0 1px 3px var(--well-inset, rgba(0, 0, 0, 0.75)),
        0 0 11px 0px var(--glow, transparent);
    }
    89% {
      box-shadow:
        inset 0 1px 3px var(--well-inset, rgba(0, 0, 0, 0.75)),
        0 0 13px 1px var(--glow, transparent);
    }
  }

  .segments {
    display: flex;
    flex-direction: column-reverse;
    gap: 3px;
    width: 8px;
    height: 40px;
  }

  .well.lg .segments {
    width: 11px;
    height: 56px;
    gap: 4px;
  }

  .seg {
    position: relative; /* containing block for the ::after overlays below
    (.seg.tip-flicker, .seg.sub-tip, .seg.overshoot) */
    flex: 1;
    border-radius: 1.5px;
    background: var(--seg-off);
    transition: background-color 200ms ease;
    transition-delay: var(--meter-delay, 0ms);
  }

  .well.lg .seg {
    border-radius: 2px;
  }

  .seg.lit-running {
    background: var(--accent);
  }

  .seg.lit-ready {
    background: var(--lamp-green);
  }

  .seg.lit-failed {
    background: var(--lamp-red);
  }

  /* Neutral "powered, no probe" segment — a gray chosen to clearly read as
     LIT against --seg-off in both themes (darker than --seg-off in light
     mode, lighter than --seg-off in dark mode; --muted already has that
     relationship in the token table, so it needs no bespoke token). Never
     flickers — no `tip-flicker` class is ever applied here (see
     flickerTipIndex above) since a static segment reads as "no signal",
     which is the point. */
  .seg.lit-no-check {
    background: var(--muted);
  }

  /* Tip flicker, part 2: opacity + a touch of brightness (still
     compositor/GPU-cheap — filter is composited same as opacity), shares
     the well's --flicker-duration/--flicker-delay via inheritance so the
     tip and its glow flicker in the same phase. Nine uneven keyframe stops
     at uneven values, held via `steps(1, end)` instead of eased, read as
     signal noise/texture rather than breathing — at a 0.45-0.8s period the
     eye integrates this as shimmer, not a visible swell. Amplitude is
     deliberately shallow throughout (0.86-1, brightness 0.92-1.08): a fast
     *and* deep dip reads as a strobe/malfunction, not texture, so this
     pulled back from an earlier 2.2-3.4s/0.58-floor pass that was tuned
     for a much slower cycle. This loop is texture only now — it used to
     also carry a single deeper 78% stop standing in for the "level"
     undershooting, but packing a level *event* into a sub-second period
     read as a mechanical beat once there was a second, similarly-fast
     event (the overshoot glimmer) layered on top; both moved out to the
     slow `--level-*` loop below, which is what `.seg.tip-flicker::after`
     drives. */
  .seg.tip-flicker {
    animation: lamp-tip-flicker var(--flicker-duration, 600ms) steps(1, end) infinite;
    animation-delay: var(--flicker-delay, 0ms);
  }

  @keyframes lamp-tip-flicker {
    0%,
    100% {
      opacity: 1;
      filter: brightness(1);
    }
    13% {
      opacity: 0.92;
      filter: brightness(0.96);
    }
    27% {
      opacity: 0.97;
      filter: brightness(1.04);
    }
    38% {
      opacity: 0.88;
      filter: brightness(0.94);
    }
    54% {
      opacity: 1;
      filter: brightness(1.08);
    }
    63% {
      opacity: 0.94;
      filter: brightness(0.98);
    }
    78% {
      opacity: 0.86;
      filter: brightness(0.92);
    }
    89% {
      opacity: 0.98;
      filter: brightness(1.06);
    }
  }

  /* Level events, part 1 — the dip: the tip segment already runs the fast
     noise loop above as its own opacity/filter animation, so a second
     animation competing for the same two properties would just overwrite
     the first (CSS animations don't compose on shared properties — last
     one applied wins, they don't blend). Layering the slow dip on its own
     `::after` overlay instead — a dark tint painted over the tip, opacity-
     only, animated independently — sidesteps that conflict entirely and
     keeps both loops compositor-cheap (opacity is the only animated
     property on the overlay; the tip element underneath still only
     animates opacity/filter itself). At rest the overlay is fully
     transparent, so it never darkens the resting/reduced-motion frame.

     Two brief, uneven events per `--level-duration` cycle (24%-27% and
     60%-63% — chosen clear of the glimmer's 9/41/78 stops on the
     `overshoot` overlay below, so a dip and a glimmer never read as the
     same event) held via `steps(1, end)`, each ~3% of the cycle
     (~84-144ms across the 2.8-4.8s duration range) — long enough to
     register as a sag, short enough to stay texture rather than a state
     change. Peak opacity 0.55/0.5 of this dark overlay reads as the tip
     sagging to roughly half brightness. Shares `--level-duration` and
     `--level-delay` — not `--level-delay-2` — with `.seg.sub-tip::after`
     below so the two dim in the exact same instants; see the `levelDelay`
     comment in the script block for why they're deliberately not
     decorrelated the way the overshoot is. */
  .seg.tip-flicker::after {
    content: "";
    position: absolute;
    inset: 0;
    border-radius: inherit;
    background: #000;
    opacity: 0;
    animation: lamp-level-dip-tip var(--level-duration, 3600ms) steps(1, end) infinite;
    animation-delay: var(--level-delay, 0ms);
  }

  @keyframes lamp-level-dip-tip {
    0%,
    100% {
      opacity: 0;
    }
    24% {
      opacity: 0.55;
    }
    27% {
      opacity: 0;
    }
    60% {
      opacity: 0.5;
    }
    63% {
      opacity: 0;
    }
  }

  /* Level events, part 2 — the sympathetic dim: the segment directly below
     the tip (`subTipIndex`) is steadily lit the whole time — this must
     never read as a second flickering tip, just a faint accomplice to the
     sag above it. Same overlay trick as the tip's dip (dark ::after,
     opacity-only, transparent at rest) but shallower — 0.2 peak versus the
     tip's 0.5-0.55 — so it reads as "dims to ~0.8," clearly still lit, not
     as its own event. Identical keyframe stops (24%/27%, 60%/63%) and the
     same `--level-duration`/`--level-delay` as `.seg.tip-flicker::after`
     — deliberately the *same* delay, not a second seeded one — so the two
     overlays turn on and off in the same frame and the top of the column
     visibly sags as one unit rather than two coincidentally-timed ones. */
  .seg.sub-tip::after {
    content: "";
    position: absolute;
    inset: 0;
    border-radius: inherit;
    background: #000;
    opacity: 0;
    animation: lamp-level-dim-sub-tip var(--level-duration, 3600ms) steps(1, end) infinite;
    animation-delay: var(--level-delay, 0ms);
  }

  @keyframes lamp-level-dim-sub-tip {
    0%,
    100% {
      opacity: 0;
    }
    24% {
      opacity: 0.2;
    }
    27% {
      opacity: 0;
    }
    60% {
      opacity: 0.2;
    }
    63% {
      opacity: 0;
    }
  }

  /* Level events, part 3 — overshoot: the segment directly above a `ready`
     tip (see `overshootIndex`) is never lit (LIT_COUNT never changes), but
     it can catch a faint, brief glimmer — the analog equivalent of a VU
     meter's next LED catching a transient. `failed` has no segment above
     its tip (5 lit is the top of the 5-segment well) so it never gets this
     class; per the design there is no undershoot-only case that also
     invents a 6th level.

     This segment's own `background` stays `--seg-off` for its whole
     lifetime (it is not, and must never look like, a steadily-lit 5th
     segment) — the glimmer is painted on a `::after` overlay instead, pre-
     colored in the lit green and driven purely by `opacity`. That keeps
     every animated property here compositor-cheap (opacity only, same
     tier as the tip's opacity/filter above) without ever touching
     `background-color`, which is not compositor-cheap and was rejected for
     that reason even though it would have been simpler.

     This used to ride the fast --flicker-duration loop (two peaks per
     0.45-0.8s cycle), which read as an obvious, rhythmic ~2Hz beat no
     matter how the stops were tuned — two events on a sub-second period
     are always close enough together for the eye to lock onto the gap
     between them. Moving it to the slow --level-duration loop (2.8-4.8s)
     fixes that on its own, but the fix that actually sells "random" is the
     *uneven* spacing on top: three brief spikes at 9%/41%/78% — gaps of
     9/32/37/22 points, deliberately dissimilar, not the evenly-spaced
     thirds a looped signal would fall into. Each spike is ~3% of the cycle
     (~84-144ms across the duration range, matching the dip's brevity) and
     peaks at a slightly different opacity (0.25/0.3/0.22) so no two events
     look identical either. Uses the independently-seeded --level-delay-2
     (not --level-delay) so these peaks don't land in lockstep with the
     tip/sub-tip dip above — see the `levelDelay2` comment in the script
     block. */
  .seg.overshoot::after {
    content: "";
    position: absolute;
    inset: 0;
    border-radius: inherit;
    background: var(--lamp-green);
    opacity: 0;
    animation: lamp-level-glimmer var(--level-duration, 3600ms) steps(1, end) infinite;
    animation-delay: var(--level-delay-2, var(--level-delay, 0ms));
  }

  @keyframes lamp-level-glimmer {
    0%,
    100% {
      opacity: 0;
    }
    9% {
      opacity: 0.25;
    }
    12% {
      opacity: 0;
    }
    41% {
      opacity: 0.3;
    }
    44% {
      opacity: 0;
    }
    78% {
      opacity: 0.22;
    }
    81% {
      opacity: 0;
    }
  }
</style>
