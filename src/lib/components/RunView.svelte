<!-- Command details page — the "SIGNAL tower" design (see
     docs/design-language.md and the approved design reference cited there).
     Fixed-rack layout: this page never document-scrolls. It fills the
     content pane exactly; the tower and header are always fully visible,
     and only the params/stages/output *screens* inside their modules scroll
     internally — see the .run-view/.right-col/.* module rules at the bottom
     for how each module is capped and where its own overflow-y lives.

     Those three screens (design variant 3c) are CRT phosphor surfaces: a
     non-scrolling `.pult-crt` wrapper (background + scan-line/sheen/
     vignette overlay, see crt.css) around a scrolling `.pult-screen` child.
     That nesting is load-bearing — the scan lines are pinned to the
     wrapper, never the scrolled content, see crt.css's file comment — so
     every module below keeps `.pult-crt > .pult-screen`, never merges the
     two onto one element. Everything else on the page (tower, header,
     toolbar, panel chrome) is the unchanged 1b faceplate language. -->
<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import type { CommandInfo, DoctorReport, Param, RunRecord } from "../types";
  import { readinessFor } from "../readiness";
  import { towerStateFor, towerDisplay, type TowerRunInput, type TowerBlinkOverride } from "../tower";
  import { SUCCESS_BLINK_COUNT, STOPPED_BLINK_COUNT, TOWER_FAILURE_BLINK_COUNT, BLINK_PERIOD_MS } from "../meterLiveness";
  import { deriveStages, stagesVisible } from "../stages";
  import { formatClock, formatDuration, formatRelative } from "../time";
  import { loadParamValues, saveParamValue } from "../api";
  import Tower from "./Tower.svelte";
  import ParamField from "./ParamField.svelte";
  import OutputPane from "./OutputPane.svelte";

  interface Props {
    command: CommandInfo;
    path: string;
    trusted: boolean;
    doctorReport: DoctorReport | null;
    run: RunRecord | null;
    /** Values this session already resolved for this command (see
     *  +page.svelte's `paramValues` lift) — when present, skips the
     *  per-repo store read entirely (see the values-loading effect below). */
    initialValues: Record<string, string> | null;
    onRun: (values: Record<string, string>) => void;
    onStop: () => void;
    onValuesChange: (values: Record<string, string>) => void;
    /** Same handler +page.svelte wires to the toolbar's "← Board" button
     *  (see Toolbar.svelte's `onBack`) — this component no longer renders
     *  its own back control, but still needs the callback for the Esc
     *  shortcut below. */
    onBack: () => void;
  }

  let { command, path, trusted, doctorReport, run, initialValues, onRun, onStop, onValuesChange, onBack }: Props =
    $props();

  // ---------------------------------------------------------------------
  // Param values: session lift (initialValues/onValuesChange, owned by
  // +page.svelte so values survive navigating back to the board) layered
  // over per-repo disk persistence (loadParamValues/saveParamValue — see
  // ../paramStore's real/mock split). Secret params are seeded/edited the
  // same as any other field but NEVER handed to `saveParamValue` — see
  // `handleChange` below.
  // ---------------------------------------------------------------------
  let values: Record<string, string> = $state({});
  const saveTimers = new Map<string, ReturnType<typeof setTimeout>>();

  $effect(() => {
    const cmd = command;
    const initial = initialValues;

    if (initial) {
      // Already resolved earlier this session (see +page.svelte's
      // paramValues lift) — trust it outright, no need to re-hit the store.
      values = { ...initial };
      return;
    }

    // First time this session for this command: seed synchronously from
    // declared defaults so fields never flash empty, then upgrade
    // asynchronously from the per-repo store once it resolves.
    const seeded: Record<string, string> = {};
    for (const p of cmd.params) seeded[p.name] = p.default ?? "";
    values = seeded;

    let cancelled = false;
    void (async () => {
      let persisted: Record<string, string> = {};
      try {
        persisted = await loadParamValues(path, cmd.id);
      } catch {
        // Best-effort — a store read failure just means fields stay at
        // their declared defaults, never a hard error blocking the page.
      }
      if (cancelled) return;
      const next: Record<string, string> = {};
      for (const p of cmd.params) next[p.name] = persisted[p.name] ?? p.default ?? "";
      values = next;
      onValuesChange(next);
    })();
    return () => {
      cancelled = true;
    };
  });

  function handleChange(param: Param, value: string) {
    values = { ...values, [param.name]: value };
    onValuesChange(values);
    if (param.secret) return; // never persisted — see the Props doc comment
    const existing = saveTimers.get(param.name);
    if (existing) clearTimeout(existing);
    saveTimers.set(
      param.name,
      setTimeout(() => {
        void saveParamValue(path, command.id, param.name, value);
      }, 400),
    );
  }

  onDestroy(() => {
    for (const t of saveTimers.values()) clearTimeout(t);
  });

  // ---------------------------------------------------------------------
  // Params module folding (large param counts) — see docs/design-language.md
  // and the layout addendum: a param with a declared default is treated as
  // optional ("default present ≈ optional"); required (no-default) params
  // always render, optional ones collapse behind a fold once the command
  // has enough params that showing all of them at once would be unwieldy.
  // Session-scoped per command (resets whenever `command` changes — which
  // for RunView also means a fresh mount, see the tower section below, but
  // the effect covers it explicitly regardless).
  // ---------------------------------------------------------------------
  const FOLD_THRESHOLD = 6; // at/under this, never fold — the fold is for
  // the long tail; a normal command's full param list (the mock's largest
  // "normal" command has 2) should never trigger it, and even a fairly
  // busy 6-param command still renders unfolded.

  const requiredParams = $derived(command.params.filter((p) => p.default == null));
  const optionalParams = $derived(command.params.filter((p) => p.default != null));
  const shouldFold = $derived(command.params.length > FOLD_THRESHOLD && optionalParams.length > 0);
  let foldExpanded = $state(false);
  $effect(() => {
    command.id;
    foldExpanded = false;
  });
  const visibleOptionalParams = $derived(shouldFold && !foldExpanded ? [] : optionalParams);

  // ---------------------------------------------------------------------
  // Params/stages CRT screens: "scroll ↕"/"scroll ↔" header hints (design
  // reference) only when the screen actually overflows — an always-on hint
  // would be noise on a short param list or a 2-3 stage run. `clientHeight`/
  // `clientWidth` are bound (Svelte wires these to a ResizeObserver, see
  // Board.svelte's `rackWidth` for the same pattern), and each effect below
  // also depends on the row/card count and fold state so it re-measures the
  // instant content changes, not just on resize — `scrollHeight`/
  // `scrollWidth` themselves aren't bindable, so they're read imperatively
  // off the element once those dependencies fire.
  // ---------------------------------------------------------------------
  let paramsScreenEl: HTMLDivElement | undefined = $state();
  let paramsScreenH = $state(0);
  let paramsOverflow = $state(false);
  $effect(() => {
    requiredParams.length;
    visibleOptionalParams.length;
    paramsScreenH;
    paramsOverflow = !!paramsScreenEl && paramsScreenEl.scrollHeight > paramsScreenEl.clientHeight + 1;
  });

  const paramMeta = $derived.by(() => {
    const total = command.params.length;
    if (total === 0) return "";
    const parts = [`${total} param${total === 1 ? "" : "s"}`];
    if (requiredParams.length > 0 && requiredParams.length < total) {
      parts.push(`${requiredParams.length} required`);
    }
    parts.push("remembered per repo");
    return parts.join(" · ") + (paramsOverflow ? " · scroll ↕" : "");
  });

  function attemptRun() {
    // Defensive safeguard (see the layout addendum): folding is only ever
    // applied to params that already carry a usable default (see
    // `optionalParams` above), so this should be unreachable in normal
    // operation — a folded param never actually blocks a run. Kept anyway
    // so a folded field that somehow ended up blank surfaces itself instead
    // of silently sending an empty value.
    if (shouldFold && !foldExpanded && optionalParams.some((p) => !values[p.name]?.trim())) {
      foldExpanded = true;
      return;
    }
    onRun(values);
  }

  // ---------------------------------------------------------------------
  // Trust gating + interactive refusal — unchanged behavior from the
  // previous layout, just re-surfaced under the new Run/Stop control.
  // ---------------------------------------------------------------------
  const disabledReason = $derived.by(() => {
    if (command.interactive) return `Needs a real terminal — run \`pult ${command.id}\` in one.`;
    if (!trusted) return "Trust this repository to run commands.";
    return null;
  });

  // ---------------------------------------------------------------------
  // Tower state. `finishedDuringVisit` still implements the "until you
  // leave the page" half of the last-run-line wording below (see
  // `lastRunLine`) — it only ever becomes true by observing a
  // running->finished transition while THIS RunView instance has been
  // mounted, and RunView is destroyed and recreated every time the user
  // leaves the details page (+page.svelte only renders it inside
  // `{#if view === "run" && selectedCommand}`), so a component-local flag
  // resets exactly when the rule requires. It no longer drives the tower
  // itself, though — the tower doesn't stand outcomes anymore (see
  // tower.ts's doc comment), it blinks. `towerBlink` is that overlay,
  // timed here the same way +page.svelte times the board's
  // `BoardMeterOverride`: set on the same running->finished edge, cleared
  // by a `setTimeout` sized to the blink's own iteration-count
  // (meterLiveness.ts's SUCCESS_BLINK_COUNT/STOPPED_BLINK_COUNT/
  // TOWER_FAILURE_BLINK_COUNT × BLINK_PERIOD_MS) so the override outlives
  // the CSS animation by exactly as long as it takes to finish blinking,
  // then the tower slews back to mirroring readiness like any other
  // transition (see Tower.svelte's level chase).
  // ---------------------------------------------------------------------
  const readiness = $derived(readinessFor(command, trusted, doctorReport));

  let finishedDuringVisit = $state(false);
  let towerBlink: TowerBlinkOverride | null = $state(null);
  let towerBlinkTimer: ReturnType<typeof setTimeout> | undefined;
  let prevRunning = false;
  $effect(() => {
    const running = run?.running ?? false;
    if (prevRunning && !running) {
      finishedDuringVisit = true;
      if (towerBlinkTimer) clearTimeout(towerBlinkTimer);
      // Crashed is checked first, but is mutually exclusive with `stopped`
      // by construction anyway (see RunRecord's `crashed` doc comment) —
      // either way it gets the tower's ordinary failure blink, same
      // red-family treatment as any other failed run (see tower.ts's doc
      // comment on why there's no separate crashed blink kind).
      const kind: TowerBlinkOverride["kind"] = run?.crashed
        ? "run-failed"
        : run?.stopped
          ? "run-stopped"
          : run?.exitCode === 0
            ? "success"
            : "run-failed";
      const count =
        kind === "success"
          ? SUCCESS_BLINK_COUNT
          : kind === "run-stopped"
            ? STOPPED_BLINK_COUNT
            : TOWER_FAILURE_BLINK_COUNT;
      towerBlink = { kind };
      towerBlinkTimer = setTimeout(() => {
        towerBlink = null;
      }, count * BLINK_PERIOD_MS);
    }
    prevRunning = running;
  });
  onDestroy(() => {
    if (towerBlinkTimer) clearTimeout(towerBlinkTimer);
  });

  const towerRunInput = $derived<TowerRunInput | null>(
    run
      ? {
          running: run.running,
          progressPct: run.progress?.pct ?? null,
          stopped: run.stopped,
          exitCode: run.exitCode,
        }
      : null,
  );
  const towerState = $derived(towerStateFor(readiness, towerRunInput, towerBlink));
  const towerDisplayValue = $derived(towerDisplay(towerState, run?.progress?.pct ?? null));

  // ---------------------------------------------------------------------
  // A 1s ticking clock, only while mounted — drives the "elapsed M:SS"
  // meta (ticks while running) and keeps "last run Nm ago" from going
  // stale on a long-open details page. Cheap: this component only exists
  // while its page is open.
  // ---------------------------------------------------------------------
  let nowTick = $state(Date.now());
  let tickHandle: ReturnType<typeof setInterval> | undefined;
  onMount(() => {
    tickHandle = setInterval(() => {
      nowTick = Date.now();
    }, 1000);
  });
  onDestroy(() => {
    if (tickHandle) clearInterval(tickHandle);
  });

  const lastRunLine = $derived.by(() => {
    if (!run) return null;
    if (run.running) return `started ${formatClock(run.startedAt)}`;
    if (!run.endedAt) return null;
    if (finishedDuringVisit) {
      if (run.crashed) return `crashed ${formatClock(run.endedAt)}`;
      return run.stopped
        ? `stopped ${formatClock(run.endedAt)}`
        : `finished ${formatClock(run.endedAt)}`;
    }
    const outcome = run.crashed
      ? "crashed"
      : run.stopped
        ? "stopped"
        : run.exitCode === 0
          ? "passed"
          : "failed";
    return `last run ${formatRelative(run.endedAt, nowTick)} · ${outcome}`;
  });

  // ---------------------------------------------------------------------
  // Stages module.
  // ---------------------------------------------------------------------
  const stages = $derived(deriveStages(command, run));
  const showStages = $derived(stagesVisible(command, run));

  // Horizontal counterpart of the params overflow check above — see that
  // comment for why the effect depends on both the measured box and the
  // content that can change it.
  let stagesScreenEl: HTMLDivElement | undefined = $state();
  let stagesScreenW = $state(0);
  let stagesOverflow = $state(false);
  $effect(() => {
    stages.length;
    stagesScreenW;
    stagesOverflow = !!stagesScreenEl && stagesScreenEl.scrollWidth > stagesScreenEl.clientWidth + 1;
  });

  const stagesMeta = $derived.by(() => {
    const done = stages.filter((s) => s.kind === "done").length;
    return `${done}/${stages.length} done` + (stagesOverflow ? " · scroll ↔" : "");
  });

  // ---------------------------------------------------------------------
  // Output module.
  // ---------------------------------------------------------------------
  const outputTitle = $derived(!run?.running && run ? "OUTPUT — LAST RUN" : "OUTPUT");
  const outputMeta = $derived.by(() => {
    if (!run) return "";
    if (run.running) return `elapsed ${formatDuration(nowTick - run.startedAt)}`;
    if (run.endedAt) return `total ${formatDuration(run.endedAt - run.startedAt)}`;
    return "";
  });

  function onKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") onBack();
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div class="run-view">
  <div class="module tower-module">
    <span class="screw screw-tl" aria-hidden="true"></span>
    <span class="screw screw-tr" aria-hidden="true"></span>
    <span class="screw screw-bl" aria-hidden="true"></span>
    <span class="screw screw-br" aria-hidden="true"></span>
    <Tower display={towerDisplayValue} seed={command.id} />
  </div>

  <div class="right-col">
    <section class="module header-module">
      <span class="screw screw-tr" aria-hidden="true"></span>
      <div class="header-row">
        <div class="title-block">
          <div class="title-line">
            <h1 class="title">{command.title}</h1>
            <code class="id mono">{command.id}</code>
          </div>
          {#if command.description}
            <p class="description">{command.description}</p>
          {/if}
        </div>

        <div class="controls">
          {#if run?.running}
            <button type="button" class="stop-btn micro" onclick={onStop}>■ Stop run</button>
          {:else}
            <button
              type="button"
              class="run-btn micro"
              disabled={!!disabledReason}
              onclick={attemptRun}
            >
              ▶ Run {command.title}
            </button>
          {/if}
          {#if lastRunLine}
            <span class="last-run mono">{lastRunLine}</span>
          {/if}
          {#if disabledReason}
            <span class="hint mono">{disabledReason}</span>
          {/if}
        </div>
      </div>
    </section>

    {#if command.params.length > 0}
      <section class="module params-module">
        <span class="screw screw-tr" aria-hidden="true"></span>
        <div class="module-header">
          <span class="module-title mono">PARAMETERS</span>
          <span class="module-meta mono">{paramMeta}</span>
        </div>
        <div class="pult-crt params-crt">
          <div
            class="pult-screen pult-crt-glow params-screen"
            bind:this={paramsScreenEl}
            bind:clientHeight={paramsScreenH}
          >
            {#each requiredParams as param (param.name)}
              <ParamField
                {param}
                {path}
                {trusted}
                commandId={command.id}
                value={values[param.name] ?? ""}
                {values}
                onChange={(v) => handleChange(param, v)}
              />
            {/each}
            {#each visibleOptionalParams as param (param.name)}
              <ParamField
                {param}
                {path}
                {trusted}
                commandId={command.id}
                value={values[param.name] ?? ""}
                {values}
                optional
                onChange={(v) => handleChange(param, v)}
              />
            {/each}
            {#if shouldFold}
              <button type="button" class="fold-row mono" onclick={() => (foldExpanded = !foldExpanded)}>
                {#if foldExpanded}
                  Hide {optionalParams.length} optional
                {:else}
                  {optionalParams.length} more · optional, using defaults
                {/if}
              </button>
            {/if}
          </div>
        </div>
      </section>
    {/if}

    {#if showStages}
      <section class="module stages-module">
        <span class="screw screw-tr" aria-hidden="true"></span>
        <div class="module-header">
          <span class="module-title mono">STAGES</span>
          <span class="module-meta mono">{stagesMeta}</span>
        </div>
        <div class="pult-crt stages-crt">
          <div
            class="pult-screen pult-crt-glow stages-screen"
            bind:this={stagesScreenEl}
            bind:clientWidth={stagesScreenW}
          >
            <div class="stages-strip">
              {#each stages as stage, i (stage.key)}
                <div class="stage-card kind-{stage.kind}">
                  <div class="stage-row">
                    <span class="lamp lamp-{stage.kind}" aria-hidden="true"></span>
                    <span class="stage-meta mono">{stage.meta}</span>
                  </div>
                  <div class="stage-name">{stage.name}</div>
                </div>
                {#if i < stages.length - 1}
                  <span class="stage-link" aria-hidden="true"></span>
                {/if}
              {/each}
            </div>
          </div>
        </div>
      </section>
    {/if}

    <section class="module output-module">
      <span class="screw screw-tr" aria-hidden="true"></span>
      <div class="module-header">
        <span class="module-title mono">{outputTitle}</span>
        <span class="module-meta mono">{outputMeta}</span>
      </div>
      <div class="pult-crt output-crt">
        <OutputPane
          lines={run?.lines ?? []}
          running={run?.running ?? false}
          dim={!run?.running && !!run}
          interactive={run?.interactive ?? false}
        />
      </div>
    </section>
  </div>
</div>

<style>
  .run-view {
    height: 100%;
    min-height: 0;
    display: grid;
    grid-template-columns: 132px 1fr;
    gap: 16px;
    padding: 20px;
    align-items: stretch;
    /* No document scroll on this page — see the file-level comment. Every
       inner module caps itself and scrolls its own content instead. */
    overflow: hidden;
  }

  /* Module faceplate — same visual language as Board.svelte's `.module`
     (hairline frame, emboss + drop shadow, corner screws), reproduced here
     rather than extracted into a shared component: Board's module owns a
     centered divider-style header for its group label, while these modules
     use a left/right header row (label + meta) per the design reference, so
     the two aren't quite the same component underneath despite the matching
     chrome. */
  .module {
    position: relative;
    min-width: 0;
    min-height: 0;
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

  .tower-module {
    display: flex;
    flex-direction: column;
  }

  .right-col {
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 16px;
    overflow: hidden;
  }

  /* --- Header module --------------------------------------------------- */
  .header-module {
    flex: none;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .header-row {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-4);
  }

  .title-block {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 0;
    flex: 1;
  }

  .title-line {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    min-width: 0;
  }

  .title {
    margin: 0;
    font-size: 21px;
    font-weight: 600;
    letter-spacing: -0.01em;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .id {
    flex: none;
    font-size: 12px;
    color: var(--muted);
  }

  .description {
    margin: 0;
    max-width: 520px;
    color: var(--muted);
    font-size: 13px;
    line-height: 1.5;
  }

  .controls {
    flex: none;
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 6px;
  }

  .run-btn {
    border: 1px solid color-mix(in srgb, var(--accent) 55%, var(--line));
    background: var(--accent);
    color: var(--accent-ink);
    border-radius: 6px;
    padding: 11px 24px;
    font-size: 13px;
    font-weight: 600;
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.25);
    white-space: nowrap;
  }

  .run-btn:hover:not(:disabled) {
    filter: brightness(1.05);
  }

  .run-btn:disabled {
    background: var(--line);
    border-color: var(--line);
    color: var(--muted);
    opacity: 0.8;
  }

  .stop-btn {
    border: 1px solid var(--lamp-red);
    background: var(--pad);
    color: var(--lamp-red);
    border-radius: 6px;
    padding: 11px 24px;
    font-size: 13px;
    font-weight: 600;
    white-space: nowrap;
  }

  .stop-btn:hover {
    background: color-mix(in srgb, var(--lamp-red) 10%, var(--pad));
  }

  .last-run,
  .hint {
    font-size: 11px;
    color: var(--muted);
    white-space: nowrap;
  }

  /* --- Shared module-header row (params/stages/output) ----------------- */
  .module-header {
    flex: none;
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space-3);
    padding-bottom: 8px;
    margin-bottom: 10px;
    border-bottom: 1px solid var(--line);
  }

  .module-title {
    font-size: 10.5px;
    font-weight: 600;
    letter-spacing: 0.2em;
    text-transform: uppercase;
    color: var(--engrave);
    text-shadow: 0 1px 0 var(--emboss-light);
  }

  .module-meta {
    font-size: 10.5px;
    color: var(--muted);
    white-space: nowrap;
  }

  /* --- Params module: a CRT screen, capped short with internal scroll as
     the backstop (design reference: 168px) rather than the old 33vh — the
     "scroll ↕" header hint (see `paramsOverflow` above) is what tells the
     operator there's more below. `.pult-crt`/`.pult-screen`'s shared rules
     (scan lines, phosphor palette, scrollbar) live in crt.css; ParamField's
     own rows (see ParamField.svelte) supply the 172px/1fr/auto column
     layout, so `.params-fields` from the old auto-fill grid is gone —
     nothing left for it to do once each row lays out its own columns. */
  .params-module {
    flex: none;
    display: flex;
    flex-direction: column;
  }

  .params-screen {
    max-height: 168px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
  }

  .fold-row {
    width: 100%;
    /* Reset the UA default button chrome (global.css only resets `cursor`,
       not appearance/background — invisible against the old light panel,
       glaring against a near-black CRT well) so this reads as a screen row,
       not a button sitting on top of one. */
    appearance: none;
    background: transparent;
    border: none;
    text-align: left;
    padding: 9px 15px;
    border-top: 1px solid var(--crt-divider, rgba(158, 214, 160, 0.08));
    color: var(--crt-muted, #6d8a72);
    font-family: var(--font-mono);
    font-size: 11px;
  }

  .fold-row:hover {
    color: var(--crt-ink, #a9c9ab);
  }

  /* --- Stages module: a horizontal CRT strip, joined by connector links -
     fixed-width phosphor cards (no card chrome — see the design reference,
     just lamp/meta/name) scroll sideways instead of wrapping, so a long
     stage ladder stays one legible row instead of shrinking every card to
     fit. The "scroll ↔" header hint mirrors `stagesOverflow` above. */
  .stages-module {
    flex: none;
    display: flex;
    flex-direction: column;
  }

  .stages-screen {
    overflow-x: auto;
    overflow-y: hidden;
  }

  .stages-strip {
    display: flex;
    align-items: stretch;
    min-width: max-content;
    padding: 12px 6px;
  }

  .stage-card {
    flex: none;
    width: 128px;
    display: flex;
    flex-direction: column;
    gap: 5px;
  }

  .stage-link {
    flex: none;
    align-self: center;
    width: 18px;
    height: 1px;
    background: var(--crt-line, rgba(158, 214, 160, 0.16));
  }

  .stage-row {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .lamp {
    flex: none;
    width: 9px;
    height: 9px;
    border-radius: 50%;
    box-shadow: inset 0 1px 1px rgba(0, 0, 0, 0.5);
  }

  .lamp-done {
    background: var(--crt-green, #7ed492);
    box-shadow:
      inset 0 1px 1px rgba(0, 0, 0, 0.5),
      0 0 8px color-mix(in srgb, var(--crt-green, #7ed492) 70%, transparent);
  }

  .lamp-active {
    background: var(--crt-amber, #e0c274);
    box-shadow:
      inset 0 1px 1px rgba(0, 0, 0, 0.5),
      0 0 8px color-mix(in srgb, var(--crt-amber, #e0c274) 70%, transparent);
  }

  .lamp-failed {
    background: var(--crt-red, #e88a7a);
    box-shadow:
      inset 0 1px 1px rgba(0, 0, 0, 0.5),
      0 0 8px color-mix(in srgb, var(--crt-red, #e88a7a) 70%, transparent);
  }

  .lamp-pending {
    background: var(--crt-off, #2c3a2f);
  }

  .stage-meta {
    font-size: 10px;
    color: var(--crt-muted, #6d8a72);
  }

  .kind-active .stage-meta {
    color: var(--crt-amber, #e0c274);
  }

  .kind-failed .stage-meta {
    color: var(--crt-red, #e88a7a);
  }

  .stage-name {
    font-size: 12px;
    font-weight: 500;
    line-height: 1.3;
    color: var(--crt-ink, #a9c9ab);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .kind-pending .stage-name {
    color: var(--crt-muted, #6d8a72);
  }

  /* --- Output module: fills remaining height, screen scrolls internally -
     the mock's max-height:220px preview cap does NOT apply here — this
     module keeps its flex:1 fill, same as before the restyle. `.pult-crt`'s
     chrome (background, scan lines) comes from crt.css; OutputPane's own
     root is the `.pult-screen` inside it (see OutputPane.svelte). */
  .output-module {
    flex: 1 1 auto;
    min-height: 140px;
    display: flex;
    flex-direction: column;
  }

  .output-crt {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
</style>
