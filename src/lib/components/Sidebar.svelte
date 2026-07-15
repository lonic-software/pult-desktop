<script lang="ts">
  import type { CommandGroup } from "../grouping";
  import type { CommandInfo, DoctorReport } from "../types";
  import { readinessFor } from "../readiness";
  import Lamp from "./Lamp.svelte";

  interface Props {
    groups: CommandGroup[];
    selectedId: string | null;
    trusted: boolean;
    doctorReport: DoctorReport | null;
    onSelect: (id: string) => void;
  }

  let { groups, selectedId, trusted, doctorReport, onSelect }: Props = $props();

  const flat = $derived(groups.flatMap((g) => g.commands));

  // Cross-group row index for the power-on stagger, capped so a long
  // listing doesn't leave the last rows waiting seconds to light up.
  const MAX_STAGGER_ROWS = 24;
  const flatIndexOf = $derived(new Map(flat.map((c: CommandInfo, i: number) => [c.id, i])));

  function staggerDelay(id: string): string {
    const i = Math.min(flatIndexOf.get(id) ?? 0, MAX_STAGGER_ROWS);
    return `${i * 40}ms`;
  }

  function indexOf(id: string | null): number {
    if (id === null) return -1;
    return flat.findIndex((c: CommandInfo) => c.id === id);
  }

  function onKeydown(event: KeyboardEvent) {
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    event.preventDefault();
    if (flat.length === 0) return;
    const current = indexOf(selectedId);
    const delta = event.key === "ArrowDown" ? 1 : -1;
    const next = current < 0 ? 0 : (current + delta + flat.length) % flat.length;
    onSelect(flat[next].id);
  }
</script>

{#key doctorReport}
  <div class="sidebar" role="listbox" aria-label="Commands" tabindex="-1" onkeydown={onKeydown}>
    {#each groups as group (group.key)}
      <div class="group">
        <div class="group-label">{group.label}</div>
        {#each group.commands as cmd (cmd.id)}
          {@const state = readinessFor(cmd, trusted, doctorReport)}
          <button
            type="button"
            role="option"
            aria-selected={selectedId === cmd.id}
            class="row micro"
            class:selected={selectedId === cmd.id}
            style="--lamp-delay: {staggerDelay(cmd.id)}"
            onclick={() => onSelect(cmd.id)}
          >
            <Lamp state={state} />
            <span class="title">{cmd.title}</span>
          </button>
        {/each}
      </div>
    {/each}
  </div>
{/key}

<style>
  .sidebar {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    padding: var(--space-4) var(--space-2);
    overflow-y: auto;
    height: 100%;
  }

  .group {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .group-label {
    padding: 0 var(--space-3);
    margin-bottom: var(--space-1);
  }

  .row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    border: none;
    background: transparent;
    border-radius: var(--radius-control);
    text-align: left;
    color: var(--ink);
  }

  .row:hover {
    background: color-mix(in srgb, var(--ink) 6%, transparent);
  }

  .row.selected {
    background: color-mix(in srgb, var(--accent) 14%, transparent);
  }

  .title {
    font-size: 13px;
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
