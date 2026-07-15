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

  // A module's width is honest to its own contents — up to 3 card columns
  // of ~240px, never more — so a 1-command group reads as a narrow module
  // and a 7-command group as a wide one with extra internal rows, sitting
  // side by side like blocks on a synth faceplate rather than stretching to
  // fill the row. That target width is a *cap*, not a fixed track size: the
  // grid itself uses auto-fit/minmax so a panel that doesn't have room for
  // its honest width (a narrow window) reflows to fewer columns instead of
  // forcing the whole board into horizontal scroll.
  const MAX_PANEL_COLUMNS = 3;
  const CARD_WIDTH_PX = 240;
  const GRID_GAP_PX = 16; // matches --space-4 — see .grid's `gap` below
  function panelMaxWidth(count: number): string {
    const cols = Math.min(count, MAX_PANEL_COLUMNS);
    return `${cols * CARD_WIDTH_PX + (cols - 1) * GRID_GAP_PX}px`;
  }
</script>

{#key doctorReport}
  <div class="board">
    {#if groups.length === 0}
      <div class="no-matches">
        <p>No commands match{search.trim() ? ` "${search.trim()}"` : ""}.</p>
      </div>
    {:else}
      <div class="panels">
        {#each groups as group (group.key)}
          <fieldset class="panel">
            <legend class="group-label">{group.label}</legend>
            <div class="grid" style="max-width: {panelMaxWidth(group.commands.length)}">
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
          </fieldset>
        {/each}
      </div>
    {/if}
  </div>
{/key}

<style>
  .board {
    height: 100%;
    overflow-y: auto;
    padding: var(--space-6);
  }

  .panels {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-start;
    gap: var(--space-4);
  }

  /* Module faceplate: a hairline frame with the silkscreen label breaking
     the top border, courtesy of plain fieldset/legend layout (no absolute
     positioning tricks needed — this is the native rendering when legend
     is a fieldset's first child). Sized to its own grid's content, never
     stretched to the row's full width. */
  .panel {
    min-width: 0;
    max-width: 100%;
    margin: 0;
    border: 1px solid var(--line);
    border-radius: var(--radius-panel);
    padding: var(--space-4);
    background: var(--panel);
  }

  .panel > legend {
    margin-inline-start: var(--space-2);
    padding: 0 var(--space-2);
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
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
