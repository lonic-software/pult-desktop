<script lang="ts">
  import type { CommandGroup } from "../grouping";
  import type { CommandInfo, DoctorReport, RunRecord } from "../types";
  import { readinessFor, type BoardMeterOverride } from "../readiness";
  import CommandCard from "./CommandCard.svelte";

  interface Props {
    groups: CommandGroup[];
    trusted: boolean;
    doctorReport: DoctorReport | null;
    runs: Record<string, RunRecord>;
    /** Post-run transient/latch overlay per command id — see readiness.ts's
     *  `BoardMeterOverride`; owned and timed centrally in +page.svelte. */
    overrides: Record<string, BoardMeterOverride | null>;
    search: string;
    onSelect: (id: string) => void;
    /** Mock-screenshot hook only (`?tooltip=<command-id>`, see routes'
     *  onMount): forces that one card's description tooltip open on mount,
     *  instead of requiring a real 500ms hover to script the shot. Inert
     *  (undefined) outside VITE_MOCK. */
    forceTooltipId?: string | null;
  }

  let { groups, trusted, doctorReport, runs, overrides, search, onSelect, forceTooltipId = null }: Props =
    $props();

  /** A running command's progress fraction (0..1), or `undefined` for an
   *  indeterminate run (no `progress.pct` data yet) — passed straight
   *  through to CommandCard/Meter's `level` prop. */
  function levelFor(id: string): number | undefined {
    const pct = runs[id]?.progress?.pct;
    return pct == null ? undefined : Math.max(0, Math.min(100, pct)) / 100;
  }

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

  // Rack layout (from the design template): the board is a rack of
  // RACK_UNIT_PX-wide columns, and each module spans up to 3 of them — one
  // per card, capped — so a 1-command group is a single narrow slot and a
  // 7-command group spans the full 3 with extra internal rows, sitting side
  // by side like rack modules rather than stretching to fill the row.
  //
  // The template's own mock used a 150px unit, but real command titles/ids
  // are longer than the mock's placeholders — at 150px nearly every title
  // and footer truncated to unreadable fragments. Widened to 184px so the
  // board's actual content reads at a glance; real content wins over mock
  // fidelity here.
  //
  // `grid-column: span N` is a hard request CSS Grid won't shrink on its
  // own: on a narrow window a 3-wide module would force extra implicit
  // columns and blow out the board horizontally (the exact failure the
  // previous fieldset-based layout hit at 760px, fixed there with a
  // max-width cap). The template's literal rack grid doesn't have an
  // equivalent auto-fit escape hatch, so here we measure the rack's actual
  // rendered width (via bind:clientWidth, backed by Svelte's own
  // ResizeObserver — no manual listener needed) and clamp every module's
  // span to however many columns actually fit, so narrow windows reflow
  // instead of overflowing. Verified at 760px.
  const RACK_UNIT_PX = 200;
  const RACK_GAP_PX = 16;
  const MAX_MODULE_COLUMNS = 3;
  let rackWidth = $state(0);
  const rackColumns = $derived(
    Math.max(1, Math.floor((rackWidth + RACK_GAP_PX) / (RACK_UNIT_PX + RACK_GAP_PX))),
  );

  // Flat groups span one column per card, capped at MAX_MODULE_COLUMNS. A
  // nested group (one panel per source, category sub-groups inside — see
  // grouping.ts's least-nesting rule) spans by its *widest* sub-group
  // instead of its total card count, so a source with two 2-card
  // categories gets a 2-wide panel with two rows of sub-groups, not a
  // needlessly-wide 4-across one.
  function moduleSpan(group: CommandGroup): number {
    const count = group.subgroups
      ? Math.max(...group.subgroups.map((sg) => sg.commands.length))
      : group.commands.length;
    return Math.max(1, Math.min(count, MAX_MODULE_COLUMNS, rackColumns));
  }
</script>

<div class="board">
  {#if groups.length === 0}
    <div class="no-matches">
      <p>No commands match{search.trim() ? ` "${search.trim()}"` : ""}.</p>
    </div>
  {:else}
    <div
      class="rack"
      style="grid-template-columns: repeat(auto-fill, {RACK_UNIT_PX}px)"
      bind:clientWidth={rackWidth}
    >
      {#each groups as group (group.key)}
        {@const span = moduleSpan(group)}
        <div class="module" style="grid-column: span {span}">
          <span class="screw screw-tl" aria-hidden="true"></span>
          <span class="screw screw-tr" aria-hidden="true"></span>
          <span class="screw screw-bl" aria-hidden="true"></span>
          <span class="screw screw-br" aria-hidden="true"></span>

          {#if group.subgroups}
            <!-- Nested (two-level) rendering — see grouping.ts's
                 least-nesting rule. This branch only ever runs when the
                 whole board is nested, so the flat branch below stays
                 exactly as it was before nesting existed. -->
            <div class="module-label-row module-label-row--source">
              <span class="module-label module-label--source">{group.label}</span>
            </div>

            <div class="subgroups">
              {#each group.subgroups as subgroup (subgroup.key)}
                <div>
                  <div class="subgroup-label-row">
                    <span class="subgroup-label">{subgroup.label}</span>
                    <span class="subgroup-rule" aria-hidden="true"></span>
                  </div>
                  <div class="module-grid" style="grid-template-columns: repeat({span}, 1fr)">
                    {#each subgroup.commands as cmd (cmd.id)}
                      {@const state = readinessFor(cmd, trusted, doctorReport)}
                      <CommandCard
                        command={cmd}
                        {state}
                        running={runs[cmd.id]?.running ?? false}
                        level={levelFor(cmd.id)}
                        override={overrides[cmd.id] ?? null}
                        staggerDelay={staggerDelay(cmd.id)}
                        forceTooltip={forceTooltipId === cmd.id}
                        onSelect={() => onSelect(cmd.id)}
                      />
                    {/each}
                  </div>
                </div>
              {/each}
            </div>
          {:else}
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
                  level={levelFor(cmd.id)}
                  override={overrides[cmd.id] ?? null}
                  staggerDelay={staggerDelay(cmd.id)}
                  forceTooltip={forceTooltipId === cmd.id}
                  onSelect={() => onSelect(cmd.id)}
                />
              {/each}
            </div>
          {/if}
        </div>
      {/each}
    </div>
  {/if}
</div>

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
    /* grid-template-columns is set inline from RACK_UNIT_PX — single source
       of truth with the JS column-fit math above. */
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

  /* Nested (source) panel label — same engraved-divider treatment as
     .module-label-row/.module-label above, just a touch larger/looser per
     the "2a nested faceplate" design (11px / 0.24em vs. the flat module
     header's 10.5px / 0.22em) and a tighter bottom margin, since the
     sub-group headers below add their own spacing. Applied only when a
     group has sub-groups, so the flat (single-level) board is untouched. */
  .module-label-row--source {
    margin-bottom: 4px;
  }

  .module-label--source {
    font-size: 11px;
    letter-spacing: 0.24em;
  }

  .subgroups {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  /* Engraved rule header for a category sub-group: a small muted label
     breaking a hairline that runs the rest of the row's width. */
  .subgroup-label-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin: 8px 1px 10px;
  }

  .subgroup-label {
    font-family: var(--font-mono);
    font-size: 9.5px;
    font-weight: 500;
    letter-spacing: 0.2em;
    text-transform: uppercase;
    color: var(--muted);
  }

  .subgroup-rule {
    flex: 1;
    height: 1px;
    background: var(--line);
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
