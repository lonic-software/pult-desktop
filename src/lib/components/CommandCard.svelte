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
    min-height: 120px;
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
    min-height: 96px;
  }

  .title {
    font-size: 15px;
    font-weight: 600;
    letter-spacing: -0.01em;
    line-height: 1.2;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
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

  /* The rack's narrowest (1-column, 150px) modules don't leave much room
     for both the id and the marker on one line. The id is the stable
     identifier — like a part number — so it keeps a readable floor (at
     least ~4 characters before ellipsis) and absorbs the available space;
     the marker is the secondary, disposable annotation, so it's the one
     that gives way, capped so it can never squeeze the id down to a single
     character. flex-grow on the id also naturally pushes the marker to the
     row's end when there's room, preserving the right-aligned look. */
  .id {
    flex: 1 1 auto;
    min-width: 6ch;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .marker {
    flex: 0 1 auto;
    min-width: 0;
    max-width: 40%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .running-marker {
    color: var(--accent);
    /* Active-state feedback matters more here than the static param-count
       marker this replaces, so it gets more room before eliding. */
    max-width: 70%;
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
