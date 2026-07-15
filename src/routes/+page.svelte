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
  import { groupCommands, type CommandGroup } from "$lib/grouping";
  import Toolbar from "$lib/components/Toolbar.svelte";
  import Sidebar from "$lib/components/Sidebar.svelte";
  import CommandDetail from "$lib/components/CommandDetail.svelte";
  import TrustModal from "$lib/components/TrustModal.svelte";
  import SettingsModal from "$lib/components/SettingsModal.svelte";
  import EmptyState from "$lib/components/EmptyState.svelte";

  interface OutputLine {
    stream: "stdout" | "stderr" | "exit";
    text: string;
  }

  let repoPath: string | null = $state(null);
  let listing: Listing | null = $state.raw<Listing | null>(null);
  let listingError: string | null = $state(null);
  let doctorReport: DoctorReport | null = $state(null);
  let selectedId: string | null = $state(null);
  let showTrustModal = $state(false);
  let trustBusy = $state(false);
  let readOnly = $state(false);
  let showSettings = $state(false);
  let pultPathSetting: string | null = $state(null);
  let versionInfo: string | null = $state(null);
  let search = $state("");
  let theme: "system" | "light" | "dark" = $state("system");
  let running = $state(false);
  let outputLines: OutputLine[] = $state([]);

  const groups: CommandGroup[] = $derived.by(() => {
    if (!listing) return [];
    const all = groupCommands(listing);
    const q = search.trim().toLowerCase();
    if (!q) return all;
    return all
      .map((g: CommandGroup) => ({
        ...g,
        commands: g.commands.filter(
          (c: CommandInfo) => c.title.toLowerCase().includes(q) || c.id.toLowerCase().includes(q),
        ),
      }))
      .filter((g: CommandGroup) => g.commands.length > 0);
  });

  const selectedCommand = $derived(
    listing?.commands.find((c) => c.id === selectedId) ?? null,
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
    outputLines = [];

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
    outputLines = [];
  }

  async function handleRun(values: Record<string, string>) {
    if (!repoPath || !selectedCommand) return;
    running = true;
    outputLines = [];
    try {
      await runCommand(repoPath, selectedCommand.id, values, (event: RunEvent) => {
        if (event.kind === "line") {
          outputLines = [...outputLines, { stream: event.stream, text: event.text }];
        } else {
          outputLines = [
            ...outputLines,
            { stream: "exit", text: `Exit code: ${event.code ?? "unknown"}` },
          ];
        }
      });
    } catch (e) {
      outputLines = [...outputLines, { stream: "exit", text: String(e) }];
    }
    running = false;
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
      if (mockState === "modal" || mockState === "trusted") {
        void (async () => {
          await handleOpenRepo();
          if (mockState === "trusted") {
            await handleTrust();
            if (forcedSelect) selectCommand(forcedSelect);
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
    {:else}
      <aside class="sidebar-pane">
        <Sidebar
          {groups}
          {selectedId}
          trusted={listing.trusted}
          {doctorReport}
          onSelect={selectCommand}
        />
      </aside>
      <main class="detail-pane">
        {#if selectedCommand}
          <CommandDetail
            command={selectedCommand}
            trusted={listing.trusted}
            {doctorReport}
            {running}
            {outputLines}
            onRun={handleRun}
          />
        {:else}
          <EmptyState message="Select a command from the sidebar." />
        {/if}
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

  .sidebar-pane {
    width: 260px;
    flex: none;
    border-right: 1px solid var(--line);
    background: var(--panel);
    min-height: 0;
  }

  .detail-pane {
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
