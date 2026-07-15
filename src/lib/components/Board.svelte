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

  // Row-major power-on stagger across the whole board (not per-module) —
  // now staggers the meters' appearance rather than the old round lamp's.
  // Capped so a long listing doesn't leave the last cards waiting seconds
  // to light up.
  const MAX_STAGGER_ROWS = 24;
  const flat = $derived(groups.flatMap((g) => g.commands));
  const flatIndexOf = $derived(new Map(flat.map((c: CommandInfo, i: number) => [c.id, i])));

  function staggerDelay(id: string): string {
    const i = Math.min(flatIndexOf.get(id) ?? 0, MAX_STAGGER_ROWS);
    return `${i * 40}ms`;
  }

  // Rack layout (from the design template): the board is a rack of 150px
  // columns, and each module spans up to 3 of them — one per card, capped —
  // so a 1-command group is a single narrow slot and a 7-command group
  // spans the full 3 with extra internal rows, sitting side by side like
  // rack modules rather than stretching to fill the row.
  //
  // `grid-column: span N` is a hard request CSS Grid won't shrink on its
  // own: on a narrow window a 3-wide module would force extra implicit
  // columns and blow out the board horizontally (the exact failure the
  // previous fieldset-based layout hit at 760px, fixed there with a
  // max-width cap). The template's literal rack grid doesn't have an
  // equivalent auto-fit escape hatch, so here we measure the rack's actual
  // rendered width (via bind:clientWidth, backed by Svelte's own
  // ResizeObserver — no manual listener needed) and clamp every module's
  // span to however many 150px columns actually fit, so narrow windows
  // reflow instead of overflowing. Verified at 760px.
  const RACK_UNIT_PX = 150;
  const RACK_GAP_PX = 16;
  const MAX_MODULE_COLUMNS = 3;
  let rackWidth = $state(0);
  const rackColumns = $derived(
    Math.max(1, Math.floor((rackWidth + RACK_GAP_PX) / (RACK_UNIT_PX + RACK_GAP_PX))),
  );

  function moduleSpan(count: number): number {
    return Math.max(1, Math.min(count, MAX_MODULE_COLUMNS, rackColumns));
  }
</script>

{#key doctorReport}
  <div class="board">
    {#if groups.length === 0}
      <div class="no-matches">
        <p>No commands match{search.trim() ? ` "${search.trim()}"` : ""}.</p>
      </div>
    {:else}
      <div class="rack" bind:clientWidth={rackWidth}>
        {#each groups as group (group.key)}
          {@const span = moduleSpan(group.commands.length)}
          <div class="module" style="grid-column: span {span}">
            <span class="screw screw-tl" aria-hidden="true"></span>
            <span class="screw screw-tr" aria-hidden="true"></span>
            <span class="screw screw-bl" aria-hidden="true"></span>
            <span class="screw screw-br" aria-hidden="true"></span>

            <div class="module-label-row">
              <span class="module-label">{group.label}</span>
            </div>

            <div class="module-grid" style="grid-template-columns: repeat({span}, 1fr)">
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
          </div>
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
    /* Subtle vertical pinstripe from the design template — reads as a
       machined panel surface rather than a flat page background. */
    background-image: repeating-linear-gradient(90deg, rgba(128, 128, 128, 0.045) 0 1px, transparent 1px 3px);
  }

  .rack {
    display: grid;
    grid-template-columns: repeat(auto-fill, 150px);
    gap: var(--space-4);
    align-items: start;
    justify-content: start;
  }

  /* Module faceplate: hairline frame, emboss + drop shadow, four corner
     screws, and a centered engraved label breaking a hairline divider —
     this replaces the previous fieldset/legend construction. */
  .module {
    position: relative;
    min-width: 0;
    margin: 0;
    border: 1px solid var(--line);
    border-radius: 8px;
    padding: 13px 15px 15px;
    background: var(--panel);
    box-shadow:
      inset 0 1px 0 var(--emboss-light),
      0 2px 5px rgba(0, 0, 0, 0.22);
  }

  .screw {
    position: absolute;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--screw);
    box-shadow:
      inset 0 1px 1.5px rgba(0, 0, 0, 0.5),
      0 1px 0 var(--emboss-light);
  }

  .screw-tl {
    top: 8px;
    left: 8px;
  }

  .screw-tr {
    top: 8px;
    right: 8px;
  }

  .screw-bl {
    bottom: 8px;
    left: 8px;
  }

  .screw-br {
    bottom: 8px;
    right: 8px;
  }

  .module-label-row {
    display: flex;
    justify-content: center;
    margin: 1px 0 12px;
    padding-bottom: 9px;
    border-bottom: 1px solid var(--line);
  }

  .module-label {
    font-family: var(--font-mono);
    font-size: 10.5px;
    font-weight: 600;
    letter-spacing: 0.22em;
    text-transform: uppercase;
    color: var(--engrave);
    text-shadow: 0 1px 0 var(--emboss-light);
  }

  .module-grid {
    display: grid;
    gap: var(--space-3);
    align-items: start;
  }

  .no-matches {
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--muted);
    text-align: center;
  }
</style>
