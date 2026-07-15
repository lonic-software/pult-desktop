<script lang="ts">
  import type { CommandInfo } from "../types";
  import type { Readiness } from "../types";
  import Lamp from "./Lamp.svelte";

  interface Props {
    command: CommandInfo;
    state: Readiness;
    running: boolean;
    staggerDelay: string;
    onSelect: () => void;
  }

  let { command, state, running, staggerDelay, onSelect }: Props = $props();

  const paramMarker = $derived.by(() => {
    if (command.interactive) return "terminal-only";
    const n = command.params.length;
    if (n === 0) return "";
    return n === 1 ? "1 param" : `${n} params`;
  });
</script>

<button
  type="button"
  class="card micro"
  style="--lamp-delay: {staggerDelay}"
  onclick={onSelect}
>
  <div class="card-top">
    <Lamp {state} />
    <span class="card-title">{command.title}</span>
  </div>

  {#if command.description}
    <p class="card-desc">{command.description}</p>
  {/if}

  <div class="card-footer mono">
    <span class="card-id">{command.id}</span>
    {#if running}
      <span class="card-marker running-marker">Running…</span>
    {:else if paramMarker}
      <span class="card-marker">{paramMarker}</span>
    {/if}
  </div>

  {#if running}
    <!-- The PULT_EVENTS step ladder (progress/status/step events) will
         replace this indeterminate strip with a determinate one once the
         desktop app claims that channel — see README's "Next steps". -->
    <div class="running-strip" aria-hidden="true"></div>
  {/if}
</button>

<style>
  .card {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    text-align: left;
    padding: var(--space-4);
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: var(--radius-panel);
    color: var(--ink);
    min-width: 0;
  }

  .card:hover {
    border-color: color-mix(in srgb, var(--ink) 40%, var(--line));
  }

  .card-top {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .card-title {
    font-size: 15px;
    font-weight: 600;
    letter-spacing: -0.01em;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }

  .card-desc {
    margin: 0;
    font-size: 13px;
    color: var(--muted);
    line-height: 1.4;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  .card-footer {
    margin-top: auto;
    padding-top: var(--space-1);
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    color: var(--muted);
  }

  .card-id {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }

  .card-marker {
    flex: none;
    white-space: nowrap;
  }

  .running-marker {
    color: var(--accent);
  }

  .running-strip {
    position: absolute;
    left: var(--radius-panel);
    right: var(--radius-panel);
    bottom: 0;
    height: 2px;
    overflow: hidden;
    background: color-mix(in srgb, var(--accent) 18%, transparent);
  }

  .running-strip::after {
    content: "";
    position: absolute;
    top: 0;
    bottom: 0;
    width: 40%;
    background: var(--accent);
    animation: running-sweep 1.1s ease-in-out infinite;
  }

  @keyframes running-sweep {
    0% {
      left: -40%;
    }
    100% {
      left: 100%;
    }
  }
</style>
