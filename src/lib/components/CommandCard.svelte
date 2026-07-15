<script lang="ts">
  import { onMount, untrack } from "svelte";
  import type { CommandInfo, Readiness } from "../types";
  import { meterStateFor } from "../readiness";
  import Meter from "./Meter.svelte";

  interface Props {
    command: CommandInfo;
    state: Readiness;
    running: boolean;
    staggerDelay: string;
    onSelect: () => void;
    /** Mock-screenshot hook only — see Board.svelte's forceTooltipId. */
    forceTooltip?: boolean;
  }

  // Destructured as `readiness`, not `state` — a local binding literally
  // named `state` shadows Svelte's `$state` rune with its `$store` legacy
  // auto-subscription syntax (any `$state(...)` call would resolve as
  // "subscribe to the local `state` store" instead), silently downgrading
  // every `$state` field below to a plain non-reactive `let` (compiler
  // warning: `non_reactive_update` / `store_rune_conflict`).
  let { command, state: readiness, running, staggerDelay, onSelect, forceTooltip = false }: Props =
    $props();

  const meterState = $derived(meterStateFor(readiness, running));

  const paramMarker = $derived.by(() => {
    if (command.interactive) return "terminal-only";
    const n = command.params.length;
    if (n === 0) return "";
    return n === 1 ? "1 param" : `${n} params`;
  });

  // Custom description tooltip: only for descriptions the 3-line clamp
  // actually truncates (checked via scrollHeight vs clientHeight, kept in
  // sync with a ResizeObserver since the rack reflows the card's width),
  // shown ~500ms after a hover over the description itself or immediately
  // on keyboard focus of the card (its one focusable button — see the
  // README's "single focusable button" note), never via the native `title`
  // attribute. Rendered through a body-level portal so the card's own
  // hover `transform` (a new containing block for `position: fixed`
  // descendants) can't throw off the fixed-viewport math.
  const tooltipId = $derived(`desc-tooltip-${command.id}`);
  let descEl: HTMLElement | undefined = $state();
  let tooltipEl: HTMLElement | undefined = $state();
  let descTruncated = $state(false);
  let tooltipOpen = $state(false);
  let tooltipPos = $state({ top: 0, left: 0 });
  let hoverTimer: ReturnType<typeof setTimeout> | undefined;

  function checkTruncation() {
    if (descEl) descTruncated = descEl.scrollHeight - descEl.clientHeight > 1;
  }

  function openTooltip() {
    if (!descTruncated || !descEl) return;
    const rect = descEl.getBoundingClientRect();
    tooltipPos = { top: rect.bottom + 6, left: rect.left };
    tooltipOpen = true;
  }

  function scheduleTooltip() {
    if (!descTruncated) return;
    if (hoverTimer) clearTimeout(hoverTimer);
    hoverTimer = setTimeout(openTooltip, 500);
  }

  function onDescLeave() {
    if (hoverTimer) clearTimeout(hoverTimer);
    if (!cardFocused) tooltipOpen = false;
  }

  let cardFocused = false;

  function onCardFocus() {
    cardFocused = true;
    openTooltip();
  }

  function onCardBlur() {
    cardFocused = false;
    tooltipOpen = false;
  }

  // Refine the guessed position once the bubble's real size is known,
  // flipping above the description when there isn't room below.
  $effect(() => {
    if (!tooltipOpen || !tooltipEl || !descEl) return;
    const tRect = tooltipEl.getBoundingClientRect();
    const dRect = descEl.getBoundingClientRect();
    untrack(() => {
      let top = dRect.bottom + 6;
      if (top + tRect.height > window.innerHeight - 8) {
        top = dRect.top - tRect.height - 6;
      }
      let left = dRect.left;
      const maxLeft = window.innerWidth - tRect.width - 8;
      if (left > maxLeft) left = Math.max(8, maxLeft);
      tooltipPos = { top, left };
    });
  });

  function portal(node: HTMLElement) {
    document.body.appendChild(node);
    return {
      destroy() {
        node.remove();
      },
    };
  }

  onMount(() => {
    checkTruncation();
    if (!descEl) return;
    const ro = new ResizeObserver(checkTruncation);
    ro.observe(descEl);
    if (forceTooltip) {
      // Mock-screenshot hook: skip the hover delay entirely.
      queueMicrotask(() => {
        checkTruncation();
        openTooltip();
      });
    }
    return () => ro.disconnect();
  });
</script>

<button
  type="button"
  class="card micro"
  onclick={onSelect}
  onfocus={onCardFocus}
  onblur={onCardBlur}
  aria-describedby={tooltipOpen ? tooltipId : undefined}
>
  <Meter state={meterState} {staggerDelay} />

  <div class="content">
    <span class="title">{command.title}</span>

    {#if command.description}
      <p
        class="desc"
        bind:this={descEl}
        onmouseenter={scheduleTooltip}
        onmouseleave={onDescLeave}
      >
        {command.description}
      </p>
    {/if}

    <div class="footer mono">
      <span class="id">{command.id}</span>
      {#if running}
        <span class="marker running-marker">Running…</span>
      {:else if paramMarker}
        <span class="marker">{paramMarker}</span>
      {/if}
    </div>
  </div>

  {#if running}
    <!-- The PULT_EVENTS step ladder (progress/status/step events) will
         replace this indeterminate strip with a determinate one once the
         desktop app claims that channel — see README's "Next steps". -->
    <div class="running-strip" aria-hidden="true"></div>
  {/if}
</button>

{#if tooltipOpen && command.description}
  <div
    class="desc-tooltip"
    id={tooltipId}
    role="tooltip"
    use:portal
    bind:this={tooltipEl}
    style="top: {tooltipPos.top}px; left: {tooltipPos.left}px"
  >
    {command.description}
  </div>
{/if}

<style>
  /* A pressable pad: raised at rest (emboss highlight + soft drop shadow),
     pressed on hover/active (nudges down 1px, shadow flattens). */
  .card {
    position: relative;
    display: flex;
    gap: 11px;
    align-items: flex-start;
    text-align: left;
    padding: 13px;
    background: var(--pad);
    border: 1px solid var(--line);
    border-radius: var(--radius-control);
    color: var(--ink);
    cursor: pointer;
    min-height: 134px;
    min-width: 0;
    box-shadow:
      inset 0 1px 0 var(--emboss-light),
      0 1px 3px rgba(0, 0, 0, 0.28);
  }

  .card:hover,
  .card:active {
    transform: translateY(1px);
    box-shadow:
      inset 0 1px 0 var(--emboss-light),
      0 1px 1px rgba(0, 0, 0, 0.2);
  }

  .content {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 0;
    flex: 1;
    min-height: 108px;
  }

  /* Titles are the short (1-2 word) side of pult's authoring convention —
     the description carries the explanation — so a single line with an
     ellipsis fallback is enough; no reserved 2-line box needed anymore. */
  .title {
    font-size: 15px;
    font-weight: 600;
    letter-spacing: -0.01em;
    line-height: 1.2;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* 3-line clamp (was 2) — the space reclaimed from the title's old 2-line
     reserve. Overflow beyond 3 lines surfaces via the custom tooltip above,
     not silently. */
  .desc {
    margin: 0;
    font-size: 12.5px;
    color: var(--muted);
    line-height: 1.4;
    display: -webkit-box;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  .footer {
    margin-top: auto;
    padding-top: 4px;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: 11px;
    color: var(--muted);
  }

  /* At the rack's 200px unit, the marker text ("terminal-only", "N params",
     "Running…") is short and bounded, so it never shrinks — it always
     renders whole. The id is the one that gives way if a long id and a
     marker can't both fit (rare at this width — verified against the
     mocks' "terminal-only"/"shell" and "2 params"/"import" pairings), with
     a floor so it never collapses to a single character. flex-grow on the
     id also naturally pushes the marker to the row's end when there's
     room, preserving the right-aligned look. */
  .id {
    flex: 1 1 auto;
    min-width: 4ch;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .marker {
    flex: none;
    white-space: nowrap;
  }

  .running-marker {
    color: var(--accent);
  }

  .running-strip {
    position: absolute;
    left: 8px;
    right: 8px;
    bottom: 0;
    height: 2px;
    overflow: hidden;
    border-radius: 0 0 6px 6px;
    background: color-mix(in srgb, var(--accent) 22%, transparent);
  }

  .running-strip::after {
    content: "";
    position: absolute;
    top: 0;
    bottom: 0;
    width: 40%;
    background: var(--accent);
    animation: pultsweep 1.1s ease-in-out infinite;
  }

  @keyframes pultsweep {
    0% {
      left: -40%;
    }
    100% {
      left: 100%;
    }
  }

  /* The description tooltip: panel-styled (not a native title bubble) —
     --panel surface, hairline border, soft shadow, capped width so long
     descriptions wrap into a readable paragraph rather than a single long
     line. Portaled to <body> (see the `portal` action) and positioned in
     JS, so it floats above the whole app regardless of the card's own
     stacking/transform context. */
  .desc-tooltip {
    position: fixed;
    z-index: 1000;
    max-width: 320px;
    padding: 8px 10px;
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: 6px;
    box-shadow: 0 6px 16px rgba(0, 0, 0, 0.32);
    color: var(--ink);
    font-size: 12.5px;
    line-height: 1.45;
    pointer-events: none;
  }
</style>
