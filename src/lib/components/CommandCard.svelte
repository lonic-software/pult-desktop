<script lang="ts">
  import type { CommandInfo, Readiness } from "../types";
  import { meterStateFor } from "../readiness";
  import Meter from "./Meter.svelte";

  interface Props {
    command: CommandInfo;
    state: Readiness;
    running: boolean;
    staggerDelay: string;
    onSelect: () => void;
  }

  let { command, state, running, staggerDelay, onSelect }: Props = $props();

  const meterState = $derived(meterStateFor(state, running));

  const paramMarker = $derived.by(() => {
    if (command.interactive) return "terminal-only";
    const n = command.params.length;
    if (n === 0) return "";
    return n === 1 ? "1 param" : `${n} params`;
  });
</script>

<button type="button" class="card micro" onclick={onSelect}>
  <Meter state={meterState} {staggerDelay} />

  <div class="content">
    <span class="title">{command.title}</span>

    {#if command.description}
      <p class="desc">{command.description}</p>
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
    min-height: 138px;
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
    min-height: 114px;
  }

  /* Single line reads best, but a wrapped 2-word title (e.g. "Show status")
     shouldn't ever ellipsize after a couple of characters — clamp to 2
     lines instead of forcing nowrap. min-height reserves the full 2-line
     box regardless of actual line count, so a 1-line title's card doesn't
     sit shorter than its 2-line neighbor in the same row. */
  .title {
    font-size: 15px;
    font-weight: 600;
    letter-spacing: -0.01em;
    line-height: 1.2;
    min-height: 2.4em;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .desc {
    margin: 0;
    font-size: 12.5px;
    color: var(--muted);
    line-height: 1.4;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
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
</style>
