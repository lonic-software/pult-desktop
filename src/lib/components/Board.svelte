<script lang="ts">
  import type { CommandGroup } from "../grouping";
  import type { CommandInfo, DoctorReport } from "../types";
  import { readinessFor } from "../readiness";
  import CommandCard from "./CommandCard.svelte";

  interface RunRecord {
    runId: string;
    running: boolean;
  }

  interface Props {
    groups: CommandGroup[];
    trusted: boolean;
    doctorReport: DoctorReport | null;
    runs: Record<string, RunRecord>;
    search: string;
    onSelect: (id: string) => void;
  }

  let { groups, trusted, doctorReport, runs, search, onSelect }: Props = $props();

  // Row-major power-on stagger across the whole board (not per-section) —
  // matches the sidebar's original cross-group stagger. Capped so a long
  // listing doesn't leave the last cards waiting seconds to light up.
  const MAX_STAGGER_ROWS = 24;
  const flat = $derived(groups.flatMap((g) => g.commands));
  const flatIndexOf = $derived(new Map(flat.map((c: CommandInfo, i: number) => [c.id, i])));

  function staggerDelay(id: string): string {
    const i = Math.min(flatIndexOf.get(id) ?? 0, MAX_STAGGER_ROWS);
    return `${i * 40}ms`;
  }
</script>

{#key doctorReport}
  <div class="board">
    {#if groups.length === 0}
      <div class="no-matches">
        <p>No commands match{search.trim() ? ` "${search.trim()}"` : ""}.</p>
      </div>
    {:else}
      {#each groups as group (group.key)}
        <section class="section">
          <h2 class="group-label">{group.label}</h2>
          <div class="grid">
            {#each group.commands as cmd (cmd.id)}
              {@const state = readinessFor(cmd, trusted, doctorReport)}
              <CommandCard
                command={cmd}
                {state}
                running={runs[cmd.id]?.running ?? false}
                staggerDelay={staggerDelay(cmd.id)}
                onSelect={() => onSelect(cmd.id)}
              />
            {/each}
          </div>
        </section>
      {/each}
    {/if}
  </div>
{/key}

<style>
  .board {
    height: 100%;
    overflow-y: auto;
    padding: var(--space-6);
    display: flex;
    flex-direction: column;
    gap: var(--space-6);
  }

  .section {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
    gap: var(--space-4);
    /* Without this, grid's default row-stretch forces title-only cards to
       match the height of a description-bearing neighbor in the same row,
       leaving a dead gap above the footer — exactly the "looks like a bug"
       case the title-only card is supposed to avoid. Each card sizes to its
       own content instead. */
    align-items: start;
  }

  .no-matches {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--muted);
    text-align: center;
  }
</style>
