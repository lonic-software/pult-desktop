<!-- The details page's SIGNAL tower: a 28-segment variant of the same LED
     instrument as the board's Meter.svelte (see docs/design-language.md's
     "The meter is one instrument" — color/level/solid/blink mean the same
     thing here as there). It's a sibling component rather than a shared one:
     the segment count (28 vs. 5), state vocabulary (readiness, run progress,
     and a post-run blink, vs. the board's readiness+running+blink), and
     digital readout/label footer are different enough that forcing both
     through one component would mean branching most of it on "which meter is
     this". What IS shared is extracted — the seeded flicker/level/blink
     timing math lives in meterLiveness.ts, used by both, and the level chase
     lives in levelSlew.ts, also used by both — and the animation *technique*
     (tip-flicker/sub-tip/overshoot as ::after overlays, the zero-dead-time
     mount climb, the blink's full-column alternation) is deliberately copied
     verbatim from Meter.svelte rather than reinvented, so a change to the
     "feel" of one should be mirrored in the other by inspection.

     The tower no longer keeps a standing PASSED/FAILED/STOPPED display (see
     tower.ts's header comment) — a finished run gets the same kind of blink
     the board's overlay does (docs/design-language.md's "Blink is a mode"),
     then the tower slews back to mirroring readiness like any other
     transition. The only extra liveness beyond the shared ambient system is
     the running states' "leading segment pulse" (a separate, more
     pronounced ::before overlay, the design reference's `pultpulse`), which
     fires for neither an idle mirror state nor a blink. -->
<script lang="ts">
  import { onMount } from "svelte";
  import type { TowerDisplay, TowerState } from "../tower";
  import { livenessTiming } from "../meterLiveness";
  import { stepToward, prefersReducedMotion, planChase, type ChasePlan } from "../levelSlew";

  interface Props {
    display: TowerDisplay;
    /** Command id — same deterministic-desync seed as Meter.svelte. */
    seed?: string;
  }

  let { display, seed = "" }: Props = $props();

  const SEGMENT_COUNT = 28;

  // Zero-dead-time mount climb — identical fix to Meter.svelte's `mounted`/
  // `displayState` (see its comment for the full rationale): RunView mounts
  // with `display` already resolved from props that predate the component,
  // so without forcing the first frame dark this would paint final colors
  // instantly and never animate in.
  let mounted = $state(false);
  onMount(() => {
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        mounted = true;
      });
    });
  });
  const shown = $derived(
    mounted
      ? display
      : { ...display, level: 0, color: "none" as const, pulse: false, state: "none" as TowerState },
  );

  // Blink states (docs/design-language.md's "Blink is a mode") — see
  // Meter.svelte's `isBlinking` for the full rationale, mirrored here: while
  // blinking, the level chase below is frozen (so recovery resumes from
  // wherever it was) and `litCount` is forced to the full column instead of
  // tracking `shown.level`.
  const BLINK_STATES = new Set<TowerState>(["blink-success", "blink-run-failed", "blink-run-stopped"]);
  const isBlinking = $derived(BLINK_STATES.has(shown.state));

  // The level chase (docs/design-language.md's "Levels slew") — see
  // Meter.svelte's `displayedLitCount` effect for the full rationale; same
  // mechanism, same shared `planChase`/`slewStepDelay` cadence (levelSlew.ts
  // — a bounded-total-duration model, not a flat per-segment one), 28-segment
  // target instead of 5. The tower's sweeps are the ones that actually reach
  // levelSlew.ts's `CAP_MS` in practice (the board's 5 segments never do —
  // see that file's header comment), which is exactly why this page's dim
  // -> ready mount climb and other long sweeps read roughly 2x faster than
  // the old flat cadence without this component doing anything special.
  const targetLitCount = $derived(Math.round(shown.level * SEGMENT_COUNT));
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

  const litCount = $derived(isBlinking ? SEGMENT_COUNT : displayedLitCount);
  const segments = $derived(Array.from({ length: SEGMENT_COUNT }, (_, i) => i < litCount));

  // "none" (dark) and "gray" (no-check's neutral "powered, no probe" read)
  // never flicker — same rule as Meter.svelte's single no-check segment: a
  // static segment reads as "no signal", which is the point. Blink states
  // are excluded too, same as Meter.svelte — a blink suspends ALL ambient
  // liveness, not just the leading pulse (`shown.pulse` is already false
  // for every blink state — see tower.ts — so that part needs no extra
  // guard here).
  const flickerEligible = $derived(
    !isBlinking && shown.color !== "none" && shown.color !== "gray",
  );
  const flickerTipIndex = $derived(flickerEligible && litCount > 0 ? litCount - 1 : -1);
  const subTipIndex = $derived(flickerTipIndex >= 0 ? flickerTipIndex - 1 : -1);
  const overshootIndex = $derived(
    flickerEligible && litCount > 0 && litCount < SEGMENT_COUNT ? litCount : -1,
  );

  const timing = $derived(livenessTiming(seed));
</script>

<div class="tower">
  <div class="header mono">SIGNAL</div>

  <div
    class="well glow-{shown.color}"
    class:blink-success={shown.state === "blink-success"}
    class:blink-failed={shown.state === "blink-run-failed"}
    class:blink-stopped={shown.state === "blink-run-stopped"}
    style="--flicker-duration: {timing.flickerDuration}ms; --flicker-delay: {timing.flickerDelay}ms; --level-duration: {timing.levelDuration}ms; --level-delay: {timing.levelDelay}ms; --level-delay-2: {timing.levelDelay2}ms"
    aria-hidden="true"
  >
    <div class="segments">
      {#each segments as lit, i (i)}
        <span
          class="seg"
          class:lit-green={lit && shown.color === "green"}
          class:lit-red={lit && shown.color === "red"}
          class:lit-amber={lit && shown.color === "amber"}
          class:lit-gray={lit && shown.color === "gray"}
          class:blink-success={shown.state === "blink-success"}
          class:blink-failed={shown.state === "blink-run-failed"}
          class:blink-stopped={shown.state === "blink-run-stopped"}
          class:tip-flicker={i === flickerTipIndex}
          class:sub-tip={i === subTipIndex}
          class:overshoot={i === overshootIndex}
          class:leading-pulse={shown.pulse && i === flickerTipIndex}
        ></span>
      {/each}
    </div>
  </div>

  <div class="footer">
    <div class="readout mono state-{shown.color}">{shown.readout}</div>
    <div class="label mono">{shown.label}</div>
  </div>
</div>

<style>
  .tower {
    height: 100%;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  .header {
    flex: none;
    text-align: center;
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.2em;
    text-transform: uppercase;
    color: var(--engrave);
    text-shadow: 0 1px 0 var(--emboss-light);
    padding-bottom: 9px;
    margin-bottom: 12px;
    border-bottom: 1px solid var(--line);
  }

  /* The recessed LED well — see Meter.svelte's `.well` comment for why only
     illumination (box-shadow, segment background-color) ever animates; the
     well's own chrome renders immediately, so a doctor-latency gap reads as
     dark hardware, not missing UI. */
  .well {
    flex: 1;
    min-height: 0;
    padding: 6px;
    border-radius: 5px;
    background: var(--led-well);
    box-shadow:
      inset 0 1px 3px var(--well-inset, rgba(0, 0, 0, 0.75)),
      0 0 18px -2px var(--glow, transparent);
    transition: box-shadow 200ms ease;
  }

  .well.glow-green {
    --glow: color-mix(in srgb, var(--lamp-green) 60%, transparent);
    --overshoot-color: var(--lamp-green);
  }

  .well.glow-red {
    --glow: color-mix(in srgb, var(--lamp-red) 60%, transparent);
  }

  .well.glow-amber {
    --glow: color-mix(in srgb, var(--accent) 60%, transparent);
    --overshoot-color: var(--accent);
  }

  /* .glow-gray and .glow-none intentionally have no rules — --glow stays
     the default `transparent` (see the box-shadow above), matching
     Meter.svelte's no-check/none: a neutral or dark well never haloes. */

  /* Green/red/amber breathe ambiently UNLESS this is a blink state (see the
     `:not(...)` guards) — blink-success is green, blink-run-failed is red,
     blink-run-stopped is amber, same colors as ready/check-failed/running,
     which is exactly why the exclusion has to be explicit here instead of
     just "not glow-none/glow-gray" the way Meter.svelte's simpler state
     vocabulary allows (see Meter.svelte's `.well.glow-running,
     .well.glow-ready, .well.glow-failed` comment for the non-ambiguous
     version of this same rule). */
  .well.glow-green:not(.blink-success):not(.blink-failed):not(.blink-stopped),
  .well.glow-red:not(.blink-success):not(.blink-failed):not(.blink-stopped),
  .well.glow-amber:not(.blink-success):not(.blink-failed):not(.blink-stopped) {
    animation: tower-lamp-breathe var(--flicker-duration, 600ms) steps(1, end) infinite;
    animation-delay: var(--flicker-delay, 0ms);
  }

  /* Verbatim technique/shape from Meter.svelte's `lamp-breathe` — see that
     file for the full rationale on the uneven 9-stop steps(1,end) shape. */
  @keyframes tower-lamp-breathe {
    0%,
    100% {
      box-shadow:
        inset 0 1px 3px var(--well-inset, rgba(0, 0, 0, 0.75)),
        0 0 20px 1px var(--glow, transparent);
    }
    13% {
      box-shadow:
        inset 0 1px 3px var(--well-inset, rgba(0, 0, 0, 0.75)),
        0 0 17px 0px var(--glow, transparent);
    }
    27% {
      box-shadow:
        inset 0 1px 3px var(--well-inset, rgba(0, 0, 0, 0.75)),
        0 0 19px 1px var(--glow, transparent);
    }
    38% {
      box-shadow:
        inset 0 1px 3px var(--well-inset, rgba(0, 0, 0, 0.75)),
        0 0 16px 0px var(--glow, transparent);
    }
    54% {
      box-shadow:
        inset 0 1px 3px var(--well-inset, rgba(0, 0, 0, 0.75)),
        0 0 20px 1px var(--glow, transparent);
    }
    63% {
      box-shadow:
        inset 0 1px 3px var(--well-inset, rgba(0, 0, 0, 0.75)),
        0 0 17px 0px var(--glow, transparent);
    }
    78% {
      box-shadow:
        inset 0 1px 3px var(--well-inset, rgba(0, 0, 0, 0.75)),
        0 0 16px 0px var(--glow, transparent);
    }
    89% {
      box-shadow:
        inset 0 1px 3px var(--well-inset, rgba(0, 0, 0, 0.75)),
        0 0 19px 1px var(--glow, transparent);
    }
  }

  /* Blink mode (docs/design-language.md's "Blink is a mode") — verbatim
     technique from Meter.svelte's `.well.glow-success`/`.well.glow-stopped`/
     `.well.glow-run-failed`, scaled to this instrument: the well's halo
     swings sharply between lit and essentially off, in the event color,
     never blended with the ambient breathing above (see the `:not(...)`
     guards on it). All three are finite here (unlike the board's latched
     run-failed) — RunView times the override's clear to match the
     iteration-count below (meterLiveness.ts's SUCCESS_BLINK_COUNT/
     STOPPED_BLINK_COUNT/TOWER_FAILURE_BLINK_COUNT), same 400ms period
     (BLINK_PERIOD_MS) as the board, unseeded — only one tower is ever on
     screen, so there's no lockstep to break up. Reduced motion: same
     degradation as Meter.svelte's blink states (0%/100% = "on", so it
     collapses to a steady solid well in the event color for the same
     real-world duration the JS timer already holds the override for). */
  .well.blink-success {
    --glow: color-mix(in srgb, var(--lamp-green) 70%, transparent);
    animation: tower-lamp-blink-well 400ms steps(1, end) 3;
  }

  .well.blink-failed {
    --glow: color-mix(in srgb, var(--lamp-red) 70%, transparent);
    animation: tower-lamp-blink-well 400ms steps(1, end) 5;
  }

  .well.blink-stopped {
    --glow: color-mix(in srgb, var(--accent) 70%, transparent);
    animation: tower-lamp-blink-well 400ms steps(1, end) 2;
  }

  @keyframes tower-lamp-blink-well {
    0%,
    45%,
    100% {
      box-shadow:
        inset 0 1px 3px var(--well-inset, rgba(0, 0, 0, 0.75)),
        0 0 22px 2px var(--glow, transparent);
    }
    50%,
    95% {
      box-shadow:
        inset 0 1px 3px var(--well-inset, rgba(0, 0, 0, 0.75)),
        0 0 3px 0px transparent;
    }
  }

  .segments {
    height: 100%;
    display: flex;
    flex-direction: column-reverse;
    gap: 3px;
  }

  .seg {
    position: relative;
    flex: 1;
    border-radius: 1.5px;
    background: var(--seg-off);
    /* No per-segment transition-delay stagger anymore — see Meter.svelte's
       `.seg` comment: the climb is a real JS-driven chase now
       (`displayedLitCount`/levelSlew.ts) that already lights segments one
       at a time, `chasePlan.delayMs` apart (levelSlew.ts's `planChase`/
       `slewStepDelay`), so a CSS delay on top would double-count the
       stagger. */
    transition: background-color 200ms ease;
  }

  .seg.lit-green {
    background: var(--lamp-green);
  }

  .seg.lit-red {
    background: var(--lamp-red);
  }

  .seg.lit-amber {
    background: var(--accent);
  }

  .seg.lit-gray {
    background: var(--muted);
  }

  /* Blink mode, segment side — verbatim technique from Meter.svelte's
     `.seg.lit-success`/`.seg.lit-stopped`/`.seg.lit-run-failed`: every
     segment is "lit" during a blink (`litCount` forces the full column —
     see the script block's `isBlinking`), and all of them blink together
     between their color and fully dim (`--seg-off`, not just a faded tint —
     the "off" phase has to look like a genuinely unlit segment). One shared
     keyframe driven off `--tint` so the three states only differ in color
     and animation-iteration-count/duration, matching the well rules above
     exactly (same reduced-motion degradation too). */
  .seg.blink-success {
    --tint: var(--lamp-green);
    background: var(--tint);
    animation: tower-lamp-blink-seg 400ms steps(1, end) 3;
  }

  .seg.blink-failed {
    --tint: var(--lamp-red);
    background: var(--tint);
    animation: tower-lamp-blink-seg 400ms steps(1, end) 5;
  }

  .seg.blink-stopped {
    --tint: var(--accent);
    background: var(--tint);
    animation: tower-lamp-blink-seg 400ms steps(1, end) 2;
  }

  @keyframes tower-lamp-blink-seg {
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

  /* Ambient tip flicker — verbatim technique from Meter.svelte's
     `.seg.tip-flicker`/`lamp-tip-flicker`. */
  .seg.tip-flicker {
    animation: tower-lamp-tip-flicker var(--flicker-duration, 600ms) steps(1, end) infinite;
    animation-delay: var(--flicker-delay, 0ms);
  }

  @keyframes tower-lamp-tip-flicker {
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

  .seg.tip-flicker::after {
    content: "";
    position: absolute;
    inset: 0;
    border-radius: inherit;
    background: #000;
    opacity: 0;
    animation: tower-lamp-level-dip-tip var(--level-duration, 3600ms) steps(1, end) infinite;
    animation-delay: var(--level-delay, 0ms);
  }

  @keyframes tower-lamp-level-dip-tip {
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

  .seg.sub-tip::after {
    content: "";
    position: absolute;
    inset: 0;
    border-radius: inherit;
    background: #000;
    opacity: 0;
    animation: tower-lamp-level-dim-sub-tip var(--level-duration, 3600ms) steps(1, end) infinite;
    animation-delay: var(--level-delay, 0ms);
  }

  @keyframes tower-lamp-level-dim-sub-tip {
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

  .seg.overshoot::after {
    content: "";
    position: absolute;
    inset: 0;
    border-radius: inherit;
    background: var(--overshoot-color, var(--lamp-green));
    opacity: 0;
    animation: tower-lamp-level-glimmer var(--level-duration, 3600ms) steps(1, end) infinite;
    animation-delay: var(--level-delay-2, var(--level-delay, 0ms));
  }

  @keyframes tower-lamp-level-glimmer {
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

  /* Leading segment pulse — running states only (see `shown.pulse` above),
     the design reference's `pultpulse`: a distinctly more pronounced swing
     (1 -> 0.45 -> 1) than the ambient tip-flicker's subtle shimmer, reading
     as "this edge is actively climbing" rather than texture. Painted on its
     own ::before layer (the tip segment's own opacity is already driven by
     `tip-flicker` above, and `::after` is already claimed by the dip/
     glimmer overlays) so none of the three animations fight over a shared
     property. */
  .seg.leading-pulse::before {
    content: "";
    position: absolute;
    inset: -3px;
    border-radius: inherit;
    box-shadow: 0 0 10px 2px var(--accent);
    animation: tower-leading-pulse 1.1s ease-in-out infinite;
  }

  @keyframes tower-leading-pulse {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.45;
    }
  }

  .footer {
    flex: none;
    padding-top: 10px;
    margin-top: 12px;
    border-top: 1px solid var(--line);
    text-align: center;
  }

  .readout {
    font-size: 19px;
    font-weight: 600;
    letter-spacing: 0.02em;
  }

  .readout.state-green {
    color: var(--lamp-green);
  }

  .readout.state-red {
    color: var(--lamp-red);
  }

  .readout.state-amber {
    color: var(--accent);
  }

  .readout.state-gray,
  .readout.state-none {
    color: var(--muted);
  }

  .label {
    margin-top: 3px;
    font-size: 9.5px;
    font-weight: 600;
    letter-spacing: 0.2em;
    text-transform: uppercase;
    color: var(--engrave);
    text-shadow: 0 1px 0 var(--emboss-light);
  }
</style>
