<script lang="ts">
  import type { MeterState } from "../readiness";

  interface Props {
    state: MeterState;
    size?: "sm" | "lg";
    staggerDelay?: string;
  }

  let { state, size = "sm", staggerDelay = "0ms" }: Props = $props();

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
</script>

<div class="well {size} glow-{state}" style="--meter-delay: {staggerDelay}" aria-hidden="true">
  <div class="segments">
    {#each segments as lit, i (i)}
      <span
        class="seg"
        class:lit-running={lit && state === "running"}
        class:lit-ready={lit && state === "ready"}
        class:lit-failed={lit && state === "failed"}
        class:lit-no-check={lit && state === "no-check"}
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
     relationship in the token table, so it needs no bespoke token). */
  .seg.lit-no-check {
    background: var(--muted);
  }
</style>
