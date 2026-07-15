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
  // red, and "no check" / untrusted shows none lit at all.
  const LIT_COUNT: Record<MeterState, number> = { running: 3, ready: 4, failed: 5, none: 0 };
  const litCount = $derived(LIT_COUNT[state]);
  const segments = $derived(Array.from({ length: 5 }, (_, i) => i < litCount));
</script>

<div class="well {size} glow-{state}" style="--meter-delay: {staggerDelay}" aria-hidden="true">
  <div class="segments">
    {#each segments as lit, i (i)}
      <span class="seg" class:lit-running={lit && state === "running"} class:lit-ready={lit && state === "ready"} class:lit-failed={lit && state === "failed"}></span>
    {/each}
  </div>
</div>

<style>
  /* The recessed LED well: segments sit in a dark slot cut into the pad,
     with a soft colored glow that only appears when something's lit. */
  .well {
    flex: none;
    padding: 4px 3px;
    border-radius: 3px;
    background: var(--led-well);
    box-shadow: inset 0 1px 3px var(--well-inset, rgba(0, 0, 0, 0.75));
    animation: meter-on 200ms ease both;
    animation-delay: var(--meter-delay, 0ms);
  }

  .well.lg {
    padding: 5px 4px;
    border-radius: 4px;
  }

  .well.glow-running {
    box-shadow:
      inset 0 1px 3px var(--well-inset, rgba(0, 0, 0, 0.75)),
      0 0 11px -1px color-mix(in srgb, var(--accent) 70%, transparent);
  }

  .well.glow-ready {
    box-shadow:
      inset 0 1px 3px var(--well-inset, rgba(0, 0, 0, 0.75)),
      0 0 11px -1px color-mix(in srgb, var(--lamp-green) 65%, transparent);
  }

  .well.glow-failed {
    box-shadow:
      inset 0 1px 3px var(--well-inset, rgba(0, 0, 0, 0.75)),
      0 0 11px -1px color-mix(in srgb, var(--lamp-red) 65%, transparent);
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

  @keyframes meter-on {
    from {
      opacity: 0;
    }
    to {
      opacity: 1;
    }
  }
</style>
