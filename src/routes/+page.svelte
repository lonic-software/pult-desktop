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
    trustRepo,
  } from "$lib/api";
  import type { CommandInfo, DoctorReport, Listing, RunEvent } from "$lib/types";
  import { groupCommands, type CommandGroup, type GroupedListing } from "$lib/grouping";
  import Toolbar from "$lib/components/Toolbar.svelte";
  import Board from "$lib/components/Board.svelte";
  import RunView from "$lib/components/RunView.svelte";
  import TrustModal from "$lib/components/TrustModal.svelte";
  import SettingsModal from "$lib/components/SettingsModal.svelte";
  import EmptyState from "$lib/components/EmptyState.svelte";

  interface OutputLine {
    stream: "stdout" | "stderr" | "exit";
    text: string;
  }

  // One run record per command id. Kept above the board/run-view switch so
  // running strips on the board and the run view's output pane both survive
  // navigating back and forth, and so more than one command can be running
  // at once (each gets its own run_id — see src/lib/types.ts's RunEvent).
  interface RunRecord {
    runId: string;
    running: boolean;
    lines: OutputLine[];
  }

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
    runs = { ...runs, [commandId]: { runId, running: true, lines: [] } };

    function appendLine(line: OutputLine) {
      const current = runs[commandId];
      if (!current || current.runId !== runId) return;
      runs = { ...runs, [commandId]: { ...current, lines: [...current.lines, line] } };
    }

    function finish(line: OutputLine) {
      const current = runs[commandId];
      if (!current || current.runId !== runId) return;
      runs = {
        ...runs,
        [commandId]: { ...current, running: false, lines: [...current.lines, line] },
      };
    }

    try {
      await runCommand(repoPath, commandId, values, runId, (event: RunEvent) => {
        if (event.kind === "line") {
          appendLine({ stream: event.stream, text: event.text });
        } else {
          finish({ stream: "exit", text: `Exit code: ${event.code ?? "unknown"}` });
        }
      });
    } catch (e) {
      finish({ stream: "exit", text: String(e) });
    }
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
          running={selectedRun?.running ?? false}
          outputLines={selectedRun?.lines ?? []}
          onRun={(values) => handleRun(selectedCommand.id, values)}
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
