<script lang="ts">
  import { onMount } from "svelte";
  import type { MeterState } from "../readiness";
  import { livenessTiming, blinkTiming } from "../meterLiveness";
  import { stepToward, prefersReducedMotion, planChase, type ChasePlan } from "../levelSlew";

  interface Props {
    state: MeterState;
    size?: "sm" | "lg";
    staggerDelay?: string;
    /** Command id — seeds the tip-flicker's phase/period deterministically
     *  so it's stable across re-renders and desynced across cards without
     *  any shared clock. Optional only so the component still works if a
     *  caller doesn't have an id handy; every real caller passes one. */
    seed?: string;
    /** Progress fraction (0..1) for the `running` state only — when set, the
     *  lit count tracks it directly (round(level*5)) instead of the fixed
     *  3-lit indeterminate reading, per docs/design-language.md ("While a
     *  run reports progress, the level is the progress fraction"). Leave
     *  unset for an indeterminate run (no progress data) — the fixed 3-lit
     *  (60%) floor plus CommandCard's own sweep strip cover that case. Has
     *  no effect on any other state. */
    level?: number;
  }

  // Destructured as `propState`, not `state` — Svelte's compiler treats any
  // `$name` where a local binding named `name` exists as a store-subscription
  // read (`$store`), so a local `state` would make the `$state` rune below
  // ambiguous with "subscribe to the store named state" and fail to compile.
  let { state: propState, size = "sm", staggerDelay = "0ms", seed = "", level }: Props = $props();

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
  // flush with nothing ever actually painted in between. `staggerMs` (parsed
  // from `staggerDelay`, Board.svelte's row-major power-on wave) delays the
  // flip itself now rather than only a CSS transition-delay (see the old
  // per-segment stagger this replaced, below) — since the segment *count*
  // is JS-driven now (see `displayedLitCount`), a card lower on the board
  // needs its climb to actually start later, not just render its color
  // fade later.
  let mounted = $state(false);
  onMount(() => {
    // Read inside the callback, not at the top of the script — staggerDelay
    // never actually changes for a mounted card, but a top-level read of a
    // prop only used once here still trips Svelte's "only captures the
    // initial value" warning; reading it inside onMount is both correct
    // (only the value at mount time matters) and quiet.
    const staggerMs = parseFloat(staggerDelay) || 0;
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        if (staggerMs > 0) setTimeout(() => (mounted = true), staggerMs);
        else mounted = true;
      });
    });
  });
  const displayState = $derived(mounted ? propState : ("none" as MeterState));

  // Blink states (docs/design-language.md's "Blink is a mode"): the board's
  // post-run overlay (readiness.ts's `BoardMeterOverride`) — success, a
  // failed run (latched until acknowledged), and a user stop — all suspend
  // the level chase below and the ambient flicker system further down, and
  // instead alternate the *whole* column between fully dim and fully lit in
  // the event color (see `.seg.lit-success`/`.seg.lit-run-failed`/
  // `.seg.lit-stopped` further down). `litCount` is forced to the full 5
  // for the duration; `displayedLitCount` is left exactly where it was, so
  // when the blink ends (+page.svelte clears the override — self-timed for
  // success/stopped, on acknowledgment for run-failed) the level chase
  // resumes from wherever it already was, which is what makes the recovery
  // read as "slew from wherever the level was to readiness" rather than a
  // fresh climb from zero.
  const BLINK_STATES = new Set<MeterState>(["success", "run-failed", "stopped"]);
  const isBlinking = $derived(BLINK_STATES.has(displayState));

  // Segments lit bottom-up per the design template's meterFor: running (in
  // progress right now) always wins and shows 3 amber (the indeterminate
  // 60% floor) regardless of readiness — UNLESS `level` is set (see the
  // Props doc comment), in which case the lit count tracks it directly;
  // otherwise ready shows 4 green, a failing check shows all 5 red. "no-check"
  // (doctor confirmed there's no `check:` to run) lights exactly one neutral
  // segment — "powered, no probe" — while "none" (untrusted, doctor hasn't
  // answered yet, or still mid-climb per `displayState` above) stays fully
  // dark. `success`/`run-failed`/`stopped` are blink states (see above) —
  // their LIT_COUNT entries are never actually read for rendering (litCount
  // forces the full 5 while blinking) but are kept here as the level the
  // chase target would otherwise reach, and as the value the recovery slew
  // starts chasing *away from* isn't this — it's wherever the run itself
  // left `displayedLitCount`, which is the whole point (see `isBlinking`
  // above).
  const LIT_COUNT: Record<MeterState, number> = {
    running: 3,
    ready: 4,
    failed: 5,
    "no-check": 1,
    none: 0,
    success: 5,
    "run-failed": 5,
    stopped: 3,
  };
  const targetLitCount = $derived(
    displayState === "running" && level !== undefined
      ? Math.max(0, Math.min(5, Math.round(level * 5)))
      : LIT_COUNT[displayState],
  );

  // The level chase itself (docs/design-language.md's "Levels slew": never
  // jump, always animate segment-by-segment through every intermediate
  // value, in both directions, on every transition — mount is just the
  // special case of chasing from 0). Pure step math lives in levelSlew.ts
  // (`stepToward`), testable apart from this scheduling glue: each time
  // `targetLitCount` changes, this effect (re)arms a single `setTimeout`
  // that nudges `displayedLitCount` one segment closer and, by writing to
  // it, re-triggers itself — a chain, not a `setInterval`, so a target that
  // changes mid-chase (e.g. progress ticking while running) redirects from
  // wherever the chase currently is instead of restarting. `chasePlan`
  // (levelSlew.ts's `planChase`) holds the current leg's per-step delay
  // steady across every step of that leg (a bounded-total-duration cadence,
  // not a flat one — see levelSlew.ts's header comment) and only
  // recomputes it when `target` itself changes; it's reset to `null`
  // whenever no chase is in flight (blinking, reduced motion, or already at
  // target) so the *next* genuine chase always plans fresh from wherever
  // `displayedLitCount` actually is. Frozen entirely while blinking (see
  // `isBlinking` above) — a blink isn't a level, it's a mode.
  // `prefersReducedMotion()` is checked live (not cached) since it's cheap
  // and can change mid-session; reduced motion snaps straight to target
  // instead of chasing it, per the design doc.
  let displayedLitCount = $state(0);
  let chasePlan: ChasePlan | null = $state(null);
  $effect(() => {
    const target = targetLitCount;
    if (isBlinking) {
      chasePlan = null;
      return;
    }
    if (prefersReducedMotion()) {
      if (displayedLitCount !== target) displayedLitCount = target;
      chasePlan = null;
      return;
    }
    if (displayedLitCount === target) {
      chasePlan = null;
      return;
    }
    chasePlan = planChase(displayedLitCount, target, chasePlan);
    const id = setTimeout(() => {
      displayedLitCount = stepToward(displayedLitCount, target);
    }, chasePlan.delayMs);
    return () => clearTimeout(id);
  });

  const litCount = $derived(isBlinking ? 5 : displayedLitCount);
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
  // `isBlinking` states are excluded entirely — a blink suspends ALL
  // ambient liveness (docs/design-language.md: "Blink is a mode"), not just
  // the run-failed one that used to get a dedicated flash instead; the two
  // motions (analog shimmer vs. sharp on/off) must never be confusable
  // (docs/design-language.md: "Flicker is texture; blink is signal").
  const flickerTipIndex = $derived(
    !isBlinking && (displayState === "running" || displayState === "ready" || displayState === "failed")
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

  // Seeded flicker/level timing (see meterLiveness.ts — shared verbatim with
  // Tower.svelte, the details page's 28-segment sibling of this component).
  // flickerDelay's fixed positive floor is what keeps the well's glow ripple
  // and the tip's own noise (both of which animate a property this
  // component's mount-climb transitions also use — box-shadow, opacity/
  // filter) from cutting that climb short; see meterLiveness.ts's own
  // comment for the fuller rationale, unchanged by this extraction.
  const timing = $derived(livenessTiming(seed));
  const flickerDuration = $derived(timing.flickerDuration);
  const flickerDelay = $derived(timing.flickerDelay);
  const levelDuration = $derived(timing.levelDuration);
  const levelDelay = $derived(timing.levelDelay);
  const levelDelay2 = $derived(timing.levelDelay2);

  // run-failed's blink is the one that latches — see meterLiveness.ts's
  // `blinkTiming` doc comment for why it alone needs a seeded cadence (the
  // others are finite, self-clearing, never more than one live at a time).
  const blinking = $derived(blinkTiming(seed));

  // running and ready are the states with a segment above their tip to
  // glimmer into (failed's tip is already the top of the 5-segment well —
  // see the LIT_COUNT/flickerTipIndex comments above, and it keeps dips
  // only, no invented 6th level). Bottom-up indexing means that segment is
  // exactly `litCount`. `litCount < 5` guards a `running` meter driven all
  // the way to full via `level` — no segment left above it to glimmer into
  // either, same as failed. `stopped` used to be in this list (a partial
  // amber overshoot) but is a blink state now (see `isBlinking` above) —
  // ambient-excluded the same way `flickerTipIndex` is.
  const overshootIndex = $derived(
    !isBlinking && (displayState === "running" || displayState === "ready") && litCount < 5
      ? litCount
      : -1,
  );

  // Ambient wash (visual-polish addendum to docs/design-language.md's
  // "Analog liveness"): a second, much larger and much subtler glow layer
  // behind the well — "the wall behind a VU meter in a dark studio" — that
  // colors the card/module around the meter. Unlike the flicker/breathe
  // system above, this layer never animates on a keyframe: it only ever
  // moves via a slow CSS `transition` (see `.well::before` below), so it
  // reads as the room settling into a new light level, not more texture.
  // Self-contained to this component (the vars are set and consumed on the
  // same `.well` element, `.well::before` just inherits them) since the
  // board has no nearby surface to reflect onto — contrast RunView's tower,
  // where the equivalent vars are set on the *page* root so the far-away
  // screens can read them too (see tower.ts's `towerGlowVars`).
  //
  // Color mirrors the well's own `--glow` mapping one-for-one (see the
  // `.well.glow-*` rules below) — success/stopped read the same hue as
  // ready/running rather than inventing new ones for a state that's on
  // screen for a moment. no-check/none never glow here either, same as they
  // never flicker (a "no signal"/dark reading shouldn't warm anything).
  const AMBIENT_COLOR: Partial<Record<MeterState, string>> = {
    running: "var(--accent)",
    ready: "var(--lamp-green)",
    failed: "var(--lamp-red)",
    success: "var(--lamp-green)",
    "run-failed": "var(--lamp-red)",
    stopped: "var(--accent)",
  };
  const ambientColor = $derived(AMBIENT_COLOR[displayState] ?? "transparent");
  // Reuses the same litCount fraction that drives the segments themselves
  // (docs/design-language.md's "Level = how much") so a meter that's barely
  // lit washes barely at all, and a full/blinking column washes at its
  // strongest — one number for both instruments, not a second one invented
  // for this layer. no-check's single lit segment (1/5) would otherwise
  // read as a faint wash despite the well itself never breathing there, so
  // it's zeroed explicitly, matching `flickerTipIndex`'s own exclusion.
  const ambientLevel = $derived(
    displayState === "no-check" || displayState === "none" ? 0 : litCount / 5,
  );
</script>

<div
  class="well {size} glow-{displayState}"
  style="--flicker-duration: {flickerDuration}ms; --flicker-delay: {flickerDelay}ms; --level-duration: {levelDuration}ms; --level-delay: {levelDelay}ms; --level-delay-2: {levelDelay2}ms; --blink-duration: {blinking.blinkDuration}ms; --blink-delay: {blinking.blinkDelay}ms; --meter-glow-color: {ambientColor}; --meter-glow-level: {ambientLevel}"
  aria-hidden="true"
>
  <div class="segments">
    {#each segments as lit, i (i)}
      <span
        class="seg"
        class:lit-running={lit && displayState === "running"}
        class:lit-ready={lit && displayState === "ready"}
        class:lit-failed={lit && displayState === "failed"}
        class:lit-no-check={lit && displayState === "no-check"}
        class:lit-success={lit && displayState === "success"}
        class:lit-run-failed={lit && displayState === "run-failed"}
        class:lit-stopped={lit && displayState === "stopped"}
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
    position: relative; /* containing block for `.well::before`'s ambient
    wash below */
    flex: none;
    padding: 4px 3px;
    border-radius: 3px;
    background: var(--led-well);
    box-shadow:
      inset 0 1px 3px var(--well-inset, rgba(0, 0, 0, 0.75)),
      0 0 11px -1px var(--glow, transparent);
    transition: box-shadow 200ms ease;
  }

  .well.lg {
    padding: 5px 4px;
    border-radius: 4px;
  }

  /* Ambient wash (see the script block's comment above `AMBIENT_COLOR`): a
     second, separate box-shadow on its own pseudo-element rather than a
     third layer on `.well`'s own `box-shadow` — that property is already
     owned by the `lamp-breathe`/blink keyframes above, and a CSS animation
     always wins the *whole* property over a transition, so a wash sharing
     it could never ease, only snap. Living on an independent element/
     property is what lets this one keep a real `transition` while the
     flicker keeps its own sharp keyframes, with neither fighting the other.
     A `color-mix` percentage driven by `--meter-glow-level` (0..1, see the
     script) rather than a separate `opacity` — one property to transition,
     and it composes with the color swap in the same interpolation. Sized
     via a large blur + modest spread (well beyond the well's own ~11-19px
     scale) so it reads as the surrounding pad/panel warming, not a bigger
     LED; kept modest enough that it stays inside the card/module's own box
     on every real call site rather than banking on bleeding past an
     ancestor's edge (CommandCard's `.card`/Board's `.module` don't clip, so
     a little overflow would be fine, but nothing here depends on it). */
  .well::before {
    content: "";
    position: absolute;
    inset: 0;
    pointer-events: none;
    box-shadow: 0 0 52px 6px color-mix(in srgb, var(--meter-glow-color, transparent) calc(var(--meter-glow-level, 0) * 28%), transparent);
    transition: box-shadow 420ms ease;
  }

  .well.lg::before {
    box-shadow: 0 0 70px 9px color-mix(in srgb, var(--meter-glow-color, transparent) calc(var(--meter-glow-level, 0) * 28%), transparent);
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
     there without any extra guard. success/run-failed/stopped are
     deliberately absent from this list — they're blink states (see below),
     which suspend this ambient breathing entirely rather than layering on
     top of it (docs/design-language.md: "Blink is a mode"). */
  .well.glow-running,
  .well.glow-ready,
  .well.glow-failed {
    animation: lamp-breathe var(--flicker-duration, 600ms) steps(1, end) infinite;
    animation-delay: var(--flicker-delay, 0ms);
  }

  /* Blink mode (docs/design-language.md's "Blink is a mode: flicker OFF,
     full column fully-dim <-> fully-lit"): success, a failed run, and a
     user stop each swing the well's halo the full way from lit to
     essentially off, sharply (steps, no easing, ~2.5Hz — `--blink-duration`
     defaults to meterLiveness.ts's BLINK_PERIOD_MS), never blended with the
     ambient breathing above. What differs between the three is only how
     many times it blinks before the override clears (see +page.svelte's
     `SUCCESS_PULSE_MS`/`STOP_FLASH_MS`, computed from the same
     SUCCESS_BLINK_COUNT/STOPPED_BLINK_COUNT this animation-iteration-count
     must stay in sync with) and the color:
       - success: a few short green blinks, self-clears.
       - stopped: briefer still, amber — pre-acknowledged, must never read
         as an alarm (docs/design-language.md: "A user stop is
         pre-acknowledged").
       - run-failed: the only one that never stops on its own — the latch,
         red, `infinite`, seeded per card (meterLiveness.ts's
         `blinkTiming`) so multiple failed cards don't blink in lockstep.

     Reduced motion: prefers-reduced-motion collapses every animation-
     duration to ~0.001ms globally (see global.css) — for a steps(1,end)
     animation that means it always samples effectively at time-in-cycle ≈
     0, i.e. permanently the 0%/100% keyframe. The keyframe below is
     written with that in mind: 0%/100% is the "on" (fully lit) frame, so
     reduced motion degrades every blink to a *steady solid* well in the
     event color rather than a frozen-mid-blink or blank one — for
     run-failed specifically, chosen over trying to preserve any
     alternation (there's no non-motion way to signal "unacknowledged"
     continuously) because the latch clears on the user's very first
     interaction with the command anyway (open its details page, or start a
     new run — see readiness.ts's `BoardMeterOverride` doc comment), so a
     steady-red well until that first click is an acceptable tradeoff, not
     a lost signal; for success/stopped, the JS-timed override clear (not
     this animation) still ends the steady color after the same real-world
     duration a sighted user's blink sequence would have taken. */
  /* Fixed, unseeded 400ms period (meterLiveness.ts's BLINK_PERIOD_MS) —
     there's only ever one success/stopped overlay live at a time, so unlike
     run-failed's latch there's no lockstep-across-cards risk to break up
     with a seed. `--blink-duration`/`--blink-delay` (set from
     meterLiveness.ts's `blinkTiming`) are reserved for run-failed alone. */
  .well.glow-success {
    --glow: color-mix(in srgb, var(--lamp-green) 75%, transparent);
    animation: lamp-blink-well 400ms steps(1, end) 3;
  }

  .well.glow-stopped {
    --glow: color-mix(in srgb, var(--accent) 75%, transparent);
    animation: lamp-blink-well 400ms steps(1, end) 2;
  }

  .well.glow-run-failed {
    --glow: color-mix(in srgb, var(--lamp-red) 75%, transparent);
    animation: lamp-blink-well var(--blink-duration, 600ms) steps(1, end) infinite;
    animation-delay: var(--blink-delay, 0ms);
  }

  @keyframes lamp-blink-well {
    0%,
    45%,
    100% {
      box-shadow:
        inset 0 1px 3px var(--well-inset, rgba(0, 0, 0, 0.75)),
        0 0 16px 2px var(--glow, transparent);
    }
    50%,
    95% {
      box-shadow:
        inset 0 1px 3px var(--well-inset, rgba(0, 0, 0, 0.75)),
        0 0 3px 0px transparent;
    }
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
    /* No per-segment transition-delay stagger anymore — the "climb" is now
       a real JS-driven chase (see the script block's `displayedLitCount`/
       levelSlew.ts's `stepToward`) that changes which segments are lit one
       at a time, `chasePlan.delayMs` apart (levelSlew.ts's `planChase`/
       `slewStepDelay` — 25ms for a short slew, shrinking toward ~12.5ms for
       a long one, never more than a bounded total sweep duration), so each
       segment's own class change already lands staggered; this transition
       only needs to smooth *that* segment's own color change once it
       happens. A per-index CSS delay on top would double-count the stagger
       and scramble the visual order on a downward slew (see levelSlew.ts's
       header comment for why that was rejected). */
    transition: background-color 200ms ease;
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

  /* Blink states (docs/design-language.md's "Blink is a mode"): every
     segment is "lit" here (see the script block's `litCount` — forced to
     the full 5 while `isBlinking`), and every one of them blinks together,
     sharply, between its color and fully dim (`--seg-off` — the same
     resting color a genuinely unlit segment shows, not just a dimmed tint,
     so the "dark" phase reads as "off," not "dim green"). One shared
     keyframe (`lamp-blink-seg`) driven off `--tint` so success/stopped/
     run-failed only differ in color and animation-iteration-count/
     duration, matching `.well.glow-success`/`.well.glow-stopped`/
     `.well.glow-run-failed` above exactly — see that comment for the
     iteration-count rationale and the reduced-motion degradation (written
     the same way here: 0%/100% = "on", so it collapses to steady solid
     color, in lockstep with the well's own glow). */
  .seg.lit-success {
    --tint: var(--lamp-green);
    background: var(--tint);
    animation: lamp-blink-seg 400ms steps(1, end) 3;
  }

  .seg.lit-stopped {
    --tint: var(--accent);
    background: var(--tint);
    animation: lamp-blink-seg 400ms steps(1, end) 2;
  }

  .seg.lit-run-failed {
    --tint: var(--lamp-red);
    background: var(--tint);
    animation: lamp-blink-seg var(--blink-duration, 600ms) steps(1, end) infinite;
    animation-delay: var(--blink-delay, 0ms);
  }

  @keyframes lamp-blink-seg {
    0%,
    45%,
    100% {
      background-color: var(--tint);
    }
    50%,
    95% {
      background-color: var(--seg-off);
    }
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
