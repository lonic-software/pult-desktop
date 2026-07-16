<script lang="ts">
  import { onMount } from "svelte";
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

  // Destructured as `propState`, not `state` — Svelte's compiler treats any
  // `$name` where a local binding named `name` exists as a store-subscription
  // read (`$store`), so a local `state` would make the `$state` rune below
  // ambiguous with "subscribe to the store named state" and fail to compile.
  let { state: propState, size = "sm", staggerDelay = "0ms", seed = "" }: Props = $props();

  // Power-on climb, every mount: force the very first frame to render fully
  // dark regardless of what `state` already resolved to, then flip to the
  // real state two frames later so the `.well`/`.seg` transitions below
  // (which only fire on an actual value change — see their comment) always
  // have something to climb from. A board card usually looked this way
  // already, since doctor's answer typically lands a beat after mount; this
  // makes it true unconditionally instead of racing doctor. A details-page
  // meter (RunView) mounts with `state` already resolved from props that
  // existed before the component did, so without this it painted the final
  // colors at frame one and never animated in at all — the dead-mount bug
  // this exists to fix. No caller-side prop needed: this is the same fix for
  // both callers, just invisible on the board where the race usually already
  // produced the same result. Two nested rAFs, not one — a single rAF risks
  // Svelte coalescing both the initial dark render and the flip into one
  // flush with nothing ever actually painted in between.
  let mounted = $state(false);
  onMount(() => {
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        mounted = true;
      });
    });
  });
  const displayState = $derived(mounted ? propState : ("none" as MeterState));

  // Segments lit bottom-up per the design template's meterFor: running (in
  // progress right now) always wins and shows 3 amber regardless of
  // readiness; otherwise ready shows 4 green, a failing check shows all 5
  // red. "no-check" (doctor confirmed there's no `check:` to run) lights
  // exactly one neutral segment — "powered, no probe" — while "none"
  // (untrusted, doctor hasn't answered yet, or still mid-climb per
  // `displayState` above) stays fully dark.
  const LIT_COUNT: Record<MeterState, number> = {
    running: 3,
    ready: 4,
    failed: 5,
    "no-check": 1,
    none: 0,
  };
  const litCount = $derived(LIT_COUNT[displayState]);
  const segments = $derived(Array.from({ length: 5 }, (_, i) => i < litCount));

  // Tip flicker: the top-most lit segment of running, ready, or failed
  // wobbles — analog current isn't perfectly constant, and that illusion
  // shouldn't stop just because the LED reads amber instead of green/red.
  // (CommandCard separately runs a `.running-strip` progress-bar sweep along
  // the bottom edge of a running card — a distinct element, not the LED
  // well, and not touched by any of this — so there's no duplication
  // between that and the meter's own texture.) The single no-check segment
  // means "no signal," so it stays put; "none" is fully dark, so there's
  // nothing to flicker. Index is bottom-up (see `segments` above), so the
  // top lit segment is always `litCount - 1`. LEDs are fixed hardware —
  // nothing ever translates — so the "level wobbles up and down" impression
  // is built from two independent loops layered on this tip segment (and
  // its neighbors), not one. A fast loop (`.seg.tip-flicker`'s own opacity/
  // filter keyframes, `--flicker-*` below) is pure signal-noise texture —
  // sub-second shimmer with no narrative. A slower loop (`--level-*` below)
  // carries the actual "level" events: the tip occasionally sags (see the
  // tip's dip overlay) in the same instant the segment directly below it
  // dims a touch too (see `subTipIndex`/`.seg.sub-tip`), so the top of the
  // column reads as sagging together — and, for running/ready (where the
  // well isn't already full to its top segment), the unlit segment above
  // the tip catches a decorrelated overshoot glimmer, colored to match the
  // state (see `overshootIndex`/`.seg.overshoot` below). Two loops at two
  // different rates read as analog drift; cramming both into one sub-second
  // cycle is what made the glimmer read as a mechanical ~2Hz beat instead.
  // Neither loop ever changes which count of segments reads as lit.
  const flickerTipIndex = $derived(
    displayState === "running" || displayState === "ready" || displayState === "failed"
      ? litCount - 1
      : -1,
  );

  // The segment directly below the tip — where the slow loop's dip (see
  // `--level-*` below) shows a faint sympathetic dim so the sag reads as
  // the *column's* level settling, not a single flickering pixel. Bottom-
  // up indexing (see `segments` above) means this is always the segment
  // one below the tip; `flickerTipIndex` is only ever -1, or >=2 (litCount
  // is 3, 4, or 5 for the only states that set it), so no extra guard is
  // needed beyond mirroring flickerTipIndex's own -1-when-inapplicable.
  const subTipIndex = $derived(flickerTipIndex >= 0 ? flickerTipIndex - 1 : -1);

  // Deterministic per-card desync: a tiny string hash (no shared PRNG
  // state, stable across re-renders) picks a period in 0.45-0.8s and a
  // phase within that period from the command id, so cards never flicker
  // in lockstep.
  function seedHash(s: string, salt: number): number {
    let h = salt >>> 0;
    for (let i = 0; i < s.length; i++) {
      h = (Math.imul(h, 31) + s.charCodeAt(i)) >>> 0;
    }
    return h;
  }
  const FLICKER_MIN_MS = 450;
  const FLICKER_RANGE_MS = 350; // 0.45s-0.8s period
  const flickerDuration = $derived(FLICKER_MIN_MS + (seedHash(seed, 0x9e3779b1) % FLICKER_RANGE_MS));

  // flickerDelay drives the two layers that DO compete with an in-flight
  // transition on the same property: the well's glow ripple (`lamp-breathe`
  // below, animates box-shadow — the same property the well's own turn-on
  // transition uses) and the tip segment's own noise (`lamp-tip-flicker`,
  // animates opacity/filter on the very segment that may still be mid
  // background-color climb — see the per-segment climb delay on `.seg`
  // below). CSS animations always take over a property from a transition
  // the moment they're active, so starting either of these before the climb
  // finished painting the tip segment's final frame would visibly cut the
  // climb short and make the glow/tip pop rather than settle.
  // FLICKER_SETTLE_MS is a fixed floor comfortably past the slowest climb
  // this component ever plays (the 200ms base transition plus up to 4
  // segments' worth of climb stagger for a `failed` well's tip); a small
  // seeded jitter on top keeps a sliver of the old per-card desync on these
  // two layers specifically. This mirrors the original design's fixed
  // (not --meter-delay-relative) settle constant — a heavily board-staggered
  // card can in principle still out-wait this floor, exactly as it could
  // before; fixing that is a `--meter-delay`-wide problem out of scope here.
  // The dip/glimmer overlays below don't share this constraint — see
  // `levelDelay`.
  const FLICKER_SETTLE_MS = 320;
  const FLICKER_SETTLE_JITTER_MS = 150;
  const flickerDelay = $derived(
    FLICKER_SETTLE_MS + (seedHash(seed, 0x85ebca77) % FLICKER_SETTLE_JITTER_MS),
  );

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
  //
  // Unlike `flickerDelay` above, this (and `levelDelay2` below) never needs
  // a positive settle floor: both overlays it drives paint on their own
  // `::after` layer, animating only that layer's opacity — a property
  // nothing else on the segment ever transitions — so there's no shared-
  // property fight to wait out (see the `.seg.tip-flicker::after` comment).
  // A NEGATIVE delay puts the animation already mid-cycle, at its own
  // seeded phase, the instant the class is applied — mount-climb end, or a
  // later state change — instead of waiting up to a full `--level-duration`
  // (2.8s-4.8s) to naturally arrive there. That open-ended wait was the
  // visible dead time on a freshly mounted details-page meter; this is what
  // makes flicker activity begin the moment the climb finishes rather than
  // sitting inert for seconds.
  const levelDelay = $derived(-(seedHash(seed, 0x165667b1) % levelDuration));
  // The overshoot glimmer (see `.seg.overshoot` below) rides the same
  // --level-duration as the dip so they share one slow signal, but gets
  // its own seeded delay on that duration — peaking in lockstep with the
  // dip looked like a mechanical seesaw rather than analog overshoot (the
  // same reason the old fast-loop version used a second delay), so a
  // second, independently-seeded phase decorrelates the two. Also negative
  // for the same zero-wait reason as `levelDelay`.
  const levelDelay2 = $derived(-(seedHash(seed, 0xd3a2646c) % levelDuration));

  // running and ready are the two states with a segment above their tip to
  // glimmer into (failed's tip is already the top of the 5-segment well —
  // see the LIT_COUNT/flickerTipIndex comments above, and it keeps dips
  // only, no invented 6th level). Bottom-up indexing means that segment is
  // exactly `litCount`.
  const overshootIndex = $derived(
    displayState === "running" || displayState === "ready" ? litCount : -1,
  );
</script>

<div
  class="well {size} glow-{displayState}"
  style="--meter-delay: {staggerDelay}; --flicker-duration: {flickerDuration}ms; --flicker-delay: {flickerDelay}ms; --level-duration: {levelDuration}ms; --level-delay: {levelDelay}ms; --level-delay-2: {levelDelay2}ms"
  aria-hidden="true"
>
  <div class="segments">
    {#each segments as lit, i (i)}
      <span
        class="seg"
        style="--seg-index: {i}"
        class:lit-running={lit && displayState === "running"}
        class:lit-ready={lit && displayState === "ready"}
        class:lit-failed={lit && displayState === "failed"}
        class:lit-no-check={lit && displayState === "no-check"}
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
     their computed value actually changes. `displayState` (see the script
     block's mount-climb comment) guarantees every real mount starts from
     "none" and flips to the resolved state a couple of frames later, so
     that first change is always real — nothing pops in unanimated, on the
     board or on the details page. The glow is expressed as a second,
     always-present box-shadow layer (transparent when unlit) rather than
     conditionally adding/removing a shadow layer, so the two-layer value
     can interpolate smoothly instead of popping. */
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
    --overshoot-color: var(--accent);
  }

  .well.glow-ready {
    --glow: color-mix(in srgb, var(--lamp-green) 65%, transparent);
    --overshoot-color: var(--lamp-green);
  }

  .well.glow-failed {
    --glow: color-mix(in srgb, var(--lamp-red) 65%, transparent);
  }

  /* Tip flicker, part 1: the glow carries a faint ripple (11-14px blur on
     the same shadow layer built above — never dips below ~79% of its peak,
     so the halo stays clearly visible at every frame) in running, ready,
     and failed alike, phase-locked to the tip segment's opacity/brightness
     noise below. Nine uneven keyframe stops plus `steps(1, end)` (a held
     value that jumps to the next, no easing) read as electrical noise
     rather than a pulse — at a sub-second period a smooth ease would still
     look like a swell. `animation-delay` (see --flicker-delay above) has a
     fixed positive floor past the segments' own climb-in transitions, so
     the animation only takes over box-shadow *after* that has already
     finished — avoiding a fight between the transition and this infinite
     loop over the same property (see the `flickerDelay` comment in the
     script block). prefers-reduced-motion already collapses all
     animation-duration to ~0 globally (see global.css), so this is inert
     there without any extra guard. */
  .well.glow-running,
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
    /* Per-segment climb: `--seg-index` (set inline per span, bottom-up —
       see `segments` above) adds a small stagger on top of the card-level
       `--meter-delay`, so a column with several segments changing at once
       (a fresh power-on, or e.g. running's 3-amber swapping to ready's
       4-green) visibly lights bottom-up instead of all segments cross-
       fading in lockstep — the same "climb" the board's card-by-card
       stagger produces, one level down. 25ms/segment keeps a 5-segment
       `failed` well's total climb (200ms + 4*25ms = 300ms) short enough to
       read as instant, not sluggish; `flickerDelay`'s floor above accounts
       for this worst case explicitly. */
    transition-delay: calc(var(--meter-delay, 0ms) + var(--seg-index, 0) * 25ms);
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
     This independence from the tip's own animated properties is also why
     `--level-delay` (unlike `--flicker-delay`) can go negative and start
     the instant the class applies — see that comment in the script block.

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

  /* Level events, part 3 — overshoot: the segment directly above a
     running or ready tip (see `overshootIndex`) is never lit (LIT_COUNT
     never changes), but it can catch a faint, brief glimmer — the analog
     equivalent of a VU meter's next LED catching a transient. `failed` has
     no segment above its tip (5 lit is the top of the 5-segment well) so it
     never gets this class; per the design there is no undershoot-only case
     that also invents a 6th level.

     This segment's own `background` stays `--seg-off` for its whole
     lifetime (it is not, and must never look like, a steadily-lit segment)
     — the glimmer is painted on a `::after` overlay instead, colored via
     `--overshoot-color` (set per state on the `.well.glow-*` rules above —
     amber for running, green for ready — so this one rule serves both
     without duplicating it per state) and driven purely by `opacity`. That
     keeps every animated property here compositor-cheap (opacity only,
     same tier as the tip's opacity/filter above) without ever touching
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
    background: var(--overshoot-color, var(--lamp-green));
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
