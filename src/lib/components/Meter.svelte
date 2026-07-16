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
  const flickerTipIndex = $derived(
    state === "ready" || state === "failed" ? litCount - 1 : -1,
  );

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
</script>

<div
  class="well {size} glow-{state}"
  style="--meter-delay: {staggerDelay}; --flicker-duration: {flickerDuration}ms; --flicker-delay: {flickerDelay}ms"
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
     deliberately shallow (opacity floor 0.86, brightness 0.92-1.08): a fast
     *and* deep dip reads as a strobe/malfunction, not texture, so this
     pulled back from an earlier 2.2-3.4s/0.58-floor pass that was tuned for
     a much slower cycle. */
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
</style>
