<script lang="ts">
  import { onMount } from "svelte";
  import {
    doctorCheck,
    getPultPath,
    isMock,
    openRepo,
    pickFolder,
    pultVersion,
    runCommand,
    setPultPath,
    stopRun,
    trustRepo,
  } from "$lib/api";
  import type { CommandInfo, DoctorReport, Listing, OutputLine, RunEvent, RunRecord } from "$lib/types";
  import { breadcrumbFor, groupCommands, type CommandGroup, type GroupedListing } from "$lib/grouping";
  import { formatDuration } from "$lib/time";
  import type { BoardMeterOverride } from "$lib/readiness";
  import { SUCCESS_BLINK_COUNT, STOPPED_BLINK_COUNT, BLINK_PERIOD_MS } from "$lib/meterLiveness";
  import Toolbar from "$lib/components/Toolbar.svelte";
  import Board from "$lib/components/Board.svelte";
  import RunView from "$lib/components/RunView.svelte";
  import TrustModal from "$lib/components/TrustModal.svelte";
  import SettingsModal from "$lib/components/SettingsModal.svelte";
  import EmptyState from "$lib/components/EmptyState.svelte";

  let repoPath: string | null = $state(null);
  let listing: Listing | null = $state.raw<Listing | null>(null);
  let listingError: string | null = $state(null);
  let doctorReport: DoctorReport | null = $state(null);
  let selectedId: string | null = $state(null);
  let view: "board" | "run" = $state("board");
  let showTrustModal = $state(false);
  let trustBusy = $state(false);
  let readOnly = $state(false);
  let showSettings = $state(false);
  let pultPathSetting: string | null = $state(null);
  let versionInfo: string | null = $state(null);
  let search = $state("");
  let theme: "system" | "light" | "dark" = $state("system");
  let runs: Record<string, RunRecord> = $state({});
  // The board's post-run transient/latch overlay per command id — see
  // readiness.ts's `BoardMeterOverride` doc comment for the pure state math
  // this drives; this is the timer/acknowledgment bookkeeping around it,
  // kept alongside `runs` (same session-scoped, per-repo lifetime — reset on
  // `loadRepo`, never persisted). `success`/`stopped` self-clear via
  // `boardOverrideTimers` below, once each has finished its own blink
  // sequence (docs/design-language.md's "Blink is a mode" — see
  // Meter.svelte's `isBlinking`); `run-failed` only clears on acknowledgment
  // (see the `$effect` near the bottom of this block). The two durations
  // below are each an iteration-count × meterLiveness.ts's BLINK_PERIOD_MS,
  // so they exactly cover Meter.svelte's `.well.glow-success`/
  // `.well.glow-stopped` animation-iteration-count — a mismatch here would
  // either clip the last blink or leave a stretch of steady color sitting
  // after the CSS animation already finished.
  let boardOverrides: Record<string, BoardMeterOverride | null> = $state({});
  const boardOverrideTimers = new Map<string, ReturnType<typeof setTimeout>>();
  const SUCCESS_PULSE_MS = SUCCESS_BLINK_COUNT * BLINK_PERIOD_MS;
  const STOP_FLASH_MS = STOPPED_BLINK_COUNT * BLINK_PERIOD_MS;

  function setBoardOverride(commandId: string, override: BoardMeterOverride | null) {
    boardOverrides = { ...boardOverrides, [commandId]: override };
  }

  function scheduleOverrideClear(commandId: string, kind: BoardMeterOverride["kind"], ms: number) {
    const existing = boardOverrideTimers.get(commandId);
    if (existing) clearTimeout(existing);
    const handle = setTimeout(() => {
      boardOverrideTimers.delete(commandId);
      // Only clear if it's still the same transient overlay — a newer run
      // (or a newer overlay from this same run finishing again, though that
      // can't actually happen) must never be clobbered by a stale timer.
      if (boardOverrides[commandId]?.kind === kind) setBoardOverride(commandId, null);
    }, ms);
    boardOverrideTimers.set(commandId, handle);
  }

  // Applies docs/design-language.md's "Only failures latch" rule once a run
  // ends: success blinks green a few times then self-clears, a user stop
  // blinks amber briefly then self-clears, anything else (a real failure, or
  // an invoke-level error before any exit event) latches red — blinking
  // until acknowledged (see the `$effect` below for the "being on/opening
  // the details page" trigger; "starting a new run" is handled directly in
  // `handleRun`).
  function applyBoardOutcome(
    commandId: string,
    outcome: { stopped: boolean; exitCode: number | null; errorText: string | null },
  ) {
    if (outcome.stopped) {
      setBoardOverride(commandId, { kind: "stopped" });
      scheduleOverrideClear(commandId, "stopped", STOP_FLASH_MS);
    } else if (outcome.errorText === null && outcome.exitCode === 0) {
      setBoardOverride(commandId, { kind: "success" });
      scheduleOverrideClear(commandId, "success", SUCCESS_PULSE_MS);
    } else {
      setBoardOverride(commandId, { kind: "run-failed" });
    }
  }

  // Acknowledgment trigger 1 and 2 of 3 (see docs/design-language.md):
  // opening this command's details page, or already being on it the moment
  // it fails. Both are exactly "the selected command's override is
  // run-failed while the run view is showing it" — re-running this effect
  // on either `selectedId`/`view` changing (navigation) or `boardOverrides`
  // changing (a live failure while already on the page) covers both without
  // separate code paths. Trigger 3 (starting a new run) is handled directly
  // in `handleRun`, since that's a state change this effect doesn't
  // otherwise observe as an "acknowledgment".
  $effect(() => {
    if (view === "run" && selectedId && boardOverrides[selectedId]?.kind === "run-failed") {
      setBoardOverride(selectedId, null);
    }
  });

  // Param form values, keyed by command id — lifted up here (rather than
  // living in RunView's own local state) so they survive navigating back to
  // the board and re-opening the same command, same as `runs` above. This
  // map is in-session working state only (reset on `loadRepo`); RunView
  // hydrates it from per-repo disk persistence (loadParamValues) on open and
  // writes non-secret edits back (saveParamValue), which is what the
  // parameters module's "remembered per repo" footer note promises — see
  // RunView.
  let paramValues: Record<string, Record<string, string>> = $state({});
  // `?tooltip=<command-id>` mock-screenshot hook — see Board.svelte's
  // forceTooltipId and CommandCard.svelte's forceTooltip.
  let forceTooltipId: string | null = $state(null);

  // groupCommands runs on the raw (unfiltered) listing so the least-nesting
  // decision (flat vs. nested — see grouping.ts) is made once per listing,
  // not recomputed per keystroke; a search that happens to leave matches in
  // only one source must not flip the board out of nested mode mid-typing.
  // Filtering below only ever removes commands/sub-groups/groups from that
  // fixed shape.
  const grouped: GroupedListing = $derived.by(() => {
    if (!listing) return { nested: false, groups: [] };
    const all = groupCommands(listing);
    const q = search.trim().toLowerCase();
    if (!q) return all;
    const matches = (c: CommandInfo) =>
      c.title.toLowerCase().includes(q) || c.id.toLowerCase().includes(q);
    const groups = all.groups
      .map((g: CommandGroup) => {
        if (!g.subgroups) {
          return { ...g, commands: g.commands.filter(matches) };
        }
        const subgroups = g.subgroups
          .map((sg) => ({ ...sg, commands: sg.commands.filter(matches) }))
          .filter((sg) => sg.commands.length > 0);
        return { ...g, commands: subgroups.flatMap((sg) => sg.commands), subgroups };
      })
      .filter((g: CommandGroup) => g.commands.length > 0);
    return { nested: all.nested, groups };
  });

  const selectedCommand = $derived(
    listing?.commands.find((c) => c.id === selectedId) ?? null,
  );

  const selectedRun = $derived(selectedId ? (runs[selectedId] ?? null) : null);

  // "repo / Source / Category" breadcrumb for the toolbar (design 3a) — only
  // meaningful once a command is selected; the board itself just shows the
  // repo name.
  const selectedBreadcrumb = $derived(
    selectedCommand && listing ? breadcrumbFor(selectedCommand, listing) : null,
  );

  $effect(() => {
    applyTheme(theme);
  });

  function applyTheme(t: "system" | "light" | "dark") {
    const root = document.documentElement;
    if (t === "system") {
      root.removeAttribute("data-theme");
    } else {
      root.setAttribute("data-theme", t);
    }
  }

  function cycleTheme() {
    theme = theme === "system" ? "light" : theme === "light" ? "dark" : "system";
    localStorage.setItem("pult-desktop:theme", theme);
  }

  async function refreshSettingsInfo() {
    pultPathSetting = await getPultPath();
    try {
      versionInfo = await pultVersion();
    } catch (e) {
      versionInfo = String(e);
    }
  }

  async function loadRepo(path: string) {
    repoPath = path;
    listingError = null;
    listing = null;
    doctorReport = null;
    selectedId = null;
    view = "board";
    runs = {};
    boardOverrides = {};
    for (const t of boardOverrideTimers.values()) clearTimeout(t);
    boardOverrideTimers.clear();
    paramValues = {};

    try {
      const result = await openRepo(path);
      listing = result;
      if (result.trusted) {
        readOnly = false;
        showTrustModal = false;
        await loadDoctor(path);
      } else if (!readOnly) {
        showTrustModal = true;
      }
    } catch (e) {
      listingError = String(e);
    }
  }

  async function loadDoctor(path: string) {
    try {
      doctorReport = await doctorCheck(path);
    } catch (e) {
      // Readiness is a nice-to-have overlay; a failure here shouldn't block
      // browsing or running commands. Lamps just stay unlit ("No check").
      console.error(e);
    }
  }

  async function handleOpenRepo() {
    const path = await pickFolder();
    if (!path) return;
    readOnly = false;
    await loadRepo(path);
  }

  async function handleTrust() {
    if (!repoPath) return;
    trustBusy = true;
    try {
      await trustRepo(repoPath);
      showTrustModal = false;
      readOnly = false;
      await loadRepo(repoPath);
    } catch (e) {
      listingError = String(e);
      showTrustModal = false;
    }
    trustBusy = false;
  }

  function handleNotNow() {
    showTrustModal = false;
    readOnly = true;
  }

  function selectCommand(id: string) {
    selectedId = id;
    view = "run";
  }

  function backToBoard() {
    view = "board";
  }

  async function handleRun(commandId: string, values: Record<string, string>) {
    if (!repoPath) return;
    const runId = crypto.randomUUID();
    // Acknowledgment trigger 3 of 3 (see docs/design-language.md): starting
    // a new run clears any latched/transient overlay left over from the
    // previous one outright, rather than leaving it to `boardMeterFor`
    // (readiness.ts) to merely out-prioritize while running — that function
    // treats `running` as always winning, but the overlay would still be
    // sitting there ready to reappear if this run somehow ended without
    // setting a new one (it can't, `finish` below always does, but clearing
    // here is the actual acknowledgment moment, not a rendering detail).
    const existingTimer = boardOverrideTimers.get(commandId);
    if (existingTimer) clearTimeout(existingTimer);
    boardOverrideTimers.delete(commandId);
    setBoardOverride(commandId, null);
    runs = {
      ...runs,
      [commandId]: {
        runId,
        running: true,
        lines: [],
        step: null,
        stepHistory: [],
        progress: null,
        status: null,
        stopped: false,
        exitCode: null,
        startedAt: Date.now(),
        endedAt: null,
      },
    };

    function appendLine(line: OutputLine) {
      const current = runs[commandId];
      if (!current || current.runId !== runId) return;
      runs = { ...runs, [commandId]: { ...current, lines: [...current.lines, line] } };
    }

    function updateRecord(patch: Partial<Pick<RunRecord, "step" | "stepHistory" | "progress" | "status">>) {
      const current = runs[commandId];
      if (!current || current.runId !== runId) return;
      runs = { ...runs, [commandId]: { ...current, ...patch } };
    }

    // Terminal states share one summary-line shape (the output module's
    // final "✓ done in M:SS" / "✗ exited with code N after M:SS" / "■
    // stopped after M:SS" line — see RunView/OutputPane) so both a clean
    // exit and an invoke-level failure (network hiccup, backend error —
    // never a normal process exit) go through the same bookkeeping.
    function finish(exitCode: number | null, stopped: boolean, errorText: string | null) {
      const current = runs[commandId];
      if (!current || current.runId !== runId) return;
      const dur = formatDuration(Date.now() - current.startedAt);
      let text: string;
      let outcome: "success" | "error" | "stopped";
      if (errorText !== null) {
        text = errorText;
        outcome = "error";
      } else if (stopped) {
        text = `■ stopped after ${dur}`;
        outcome = "stopped";
      } else if (exitCode === 0) {
        text = `✓ done in ${dur}`;
        outcome = "success";
      } else {
        text = `✗ exited with code ${exitCode ?? "unknown"} after ${dur}`;
        outcome = "error";
      }
      runs = {
        ...runs,
        [commandId]: {
          ...current,
          running: false,
          stopped,
          exitCode,
          endedAt: Date.now(),
          lines: [...current.lines, { stream: "exit", text, outcome }],
        },
      };
      applyBoardOutcome(commandId, { stopped, exitCode, errorText });
    }

    try {
      await runCommand(repoPath, commandId, values, runId, (event: RunEvent) => {
        switch (event.kind) {
          case "line":
            appendLine({ stream: event.stream, text: event.text });
            break;
          case "step": {
            const current = runs[commandId];
            if (!current || current.runId !== runId) break;
            const step = { k: event.k, n: event.n, name: event.name, at: Date.now() };
            updateRecord({ step, stepHistory: [...current.stepHistory, step] });
            break;
          }
          case "progress":
            updateRecord({ progress: { pct: event.pct, text: event.text } });
            break;
          case "status":
            updateRecord({ status: event.text });
            break;
          case "exit":
            finish(event.code, event.stopped, null);
            break;
        }
      });
    } catch (e) {
      finish(null, false, String(e));
    }
  }

  function handleStop(runId: string) {
    void stopRun(runId);
  }

  function handleValuesChange(commandId: string, values: Record<string, string>) {
    paramValues = { ...paramValues, [commandId]: values };
  }

  function openSettings() {
    refreshSettingsInfo();
    showSettings = true;
  }

  async function saveSettings(path: string) {
    await setPultPath(path);
    showSettings = false;
    await refreshSettingsInfo();
  }

  onMount(() => {
    const saved = localStorage.getItem("pult-desktop:theme");
    if (saved === "light" || saved === "dark" || saved === "system") {
      theme = saved;
    }

    // Mock-only screenshot helpers: `?theme=dark` forces a theme, and
    // `?mockstate=modal|trusted` drives the app straight to a given state
    // without manual clicking — used to script the light/dark/trust-modal
    // screenshots in a plain headless-Chrome pass (see README's mock mode
    // section). Never active outside VITE_MOCK=1.
    if (isMock) {
      const params = new URLSearchParams(window.location.search);
      const forcedTheme = params.get("theme");
      if (forcedTheme === "light" || forcedTheme === "dark") {
        theme = forcedTheme;
      }
      const mockState = params.get("mockstate");
      const forcedSelect = params.get("select");
      const forcedSearch = params.get("search");
      const forcedRun = params.get("run");
      const forcedTooltip = params.get("tooltip");
      // `?stopAfter=<ms>` — only meaningful alongside `?run=`: schedules an
      // automatic Stop click `ms` after the run starts, purely so the stop
      // flow (brief amber, no latch — see docs/design-language.md) can be
      // screenshotted from a one-shot headless render instead of needing
      // real interactive clicking.
      const stopAfter = params.get("stopAfter");
      if (forcedTooltip) forceTooltipId = forcedTooltip;
      if (mockState === "modal" || mockState === "trusted" || mockState === "untrusted") {
        void (async () => {
          await handleOpenRepo();
          if (mockState === "trusted") {
            await handleTrust();
            if (forcedSelect) selectCommand(forcedSelect);
            if (forcedSearch) search = forcedSearch;
            // `?run=<command-id>` kicks off a mock run without navigating
            // into its run view, so a board screenshot can show a card
            // mid-run (running strip + amber meter) — see "Mock mode" in
            // the README.
            if (forcedRun) void handleRun(forcedRun, {});
            if (forcedRun && stopAfter) {
              const ms = Number(stopAfter);
              if (Number.isFinite(ms)) {
                setTimeout(() => {
                  const active = runs[forcedRun];
                  if (active?.running) handleStop(active.runId);
                }, ms);
              }
            }
          } else if (mockState === "untrusted") {
            // Dismiss the trust modal (as if the user clicked "Not now")
            // to land on the read-only board itself, all dark — the modal
            // would otherwise sit on top of it for every screenshot.
            handleNotNow();
          }
        })();
      }
    }
  });
</script>

<svelte:head>
  <title>{listing?.name ?? "pult-desktop"}</title>
</svelte:head>

<div class="app">
  <Toolbar
    repoName={listing?.name ?? null}
    {search}
    {theme}
    breadcrumb={view === "run" ? selectedBreadcrumb : null}
    onBack={view === "run" ? backToBoard : null}
    onOpenRepo={handleOpenRepo}
    onSearch={(v) => (search = v)}
    onToggleTheme={cycleTheme}
    onOpenSettings={openSettings}
  />

  <div class="body">
    {#if !listing}
      <div class="fill">
        <EmptyState
          message={listingError ?? "Open a repository to see its commands."}
          onOpenRepo={handleOpenRepo}
        />
      </div>
    {:else if view === "run" && selectedCommand}
      <main class="content-pane">
        <RunView
          command={selectedCommand}
          path={repoPath ?? ""}
          trusted={listing.trusted}
          {doctorReport}
          run={selectedRun}
          initialValues={paramValues[selectedCommand.id] ?? null}
          onRun={(values) => handleRun(selectedCommand.id, values)}
          onStop={() => selectedRun && handleStop(selectedRun.runId)}
          onValuesChange={(values) => handleValuesChange(selectedCommand.id, values)}
          onBack={backToBoard}
        />
      </main>
    {:else}
      <main class="content-pane">
        <Board
          groups={grouped.groups}
          trusted={listing.trusted}
          {doctorReport}
          {runs}
          overrides={boardOverrides}
          {search}
          {forceTooltipId}
          onSelect={selectCommand}
        />
      </main>
    {/if}
  </div>

  {#if showTrustModal && listing}
    <TrustModal {listing} busy={trustBusy} onTrust={handleTrust} onNotNow={handleNotNow} />
  {/if}

  {#if showSettings}
    <SettingsModal
      currentPath={pultPathSetting}
      {versionInfo}
      onSave={saveSettings}
      onClose={() => (showSettings = false)}
    />
  {/if}

  {#if isMock}
    <div class="mock-badge mono">MOCK</div>
  {/if}
</div>

<style>
  .app {
    height: 100vh;
    display: flex;
    flex-direction: column;
  }

  .body {
    flex: 1;
    min-height: 0;
    display: flex;
  }

  .fill {
    flex: 1;
    min-width: 0;
    height: 100%;
  }

  .content-pane {
    flex: 1;
    min-width: 0;
    min-height: 0;
    background: var(--bg);
  }

  .mock-badge {
    position: fixed;
    bottom: var(--space-2);
    right: var(--space-2);
    font-size: 10px;
    padding: 2px 6px;
    border-radius: 4px;
    background: var(--accent);
    color: var(--accent-ink);
    opacity: 0.85;
    pointer-events: none;
  }
</style>
