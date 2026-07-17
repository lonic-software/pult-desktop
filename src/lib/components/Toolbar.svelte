<script lang="ts">
  interface Props {
    repoName: string | null;
    search: string;
    theme: "system" | "light" | "dark";
    onSearch: (value: string) => void;
    onToggleTheme: () => void;
    onOpenSettings: () => void;
    /** Source / category path for the currently-open command, shown after
     *  the repo name on the details page (design 3a's "repo / Source /
     *  Category" breadcrumb — see `breadcrumbFor` in grouping.ts). Absent
     *  on the board, where the repo name alone is enough context. */
    breadcrumb?: { source: string; category: string } | null;
    /** Present only on the details page (+page.svelte passes the same
     *  `backToBoard` it wires to RunView's Esc handling; the board itself
     *  passes `null`) — renders a bordered "← Board" button as the
     *  toolbar's left-most element, before the breadcrumb. `null` rather
     *  than always-present-but-sometimes-a-no-op so the board never renders
     *  a control with nothing to go back to. */
    onBack?: (() => void) | null;
  }

  let {
    repoName,
    search,
    theme,
    onSearch,
    onToggleTheme,
    onOpenSettings,
    breadcrumb = null,
    onBack = null,
  }: Props = $props();
</script>

<header class="toolbar">
  <div class="drag-region" data-tauri-drag-region></div>
  <div class="content">
    {#if onBack}
      <button type="button" class="back-btn micro" onclick={onBack}>← Board</button>
    {/if}
    <!-- Mounting/switching devices lives in the rack sidebar (design 4a) —
         the toolbar just names what's active. -->
    <span class="repo-name mono"
      >{repoName ?? "No device active"}{#if breadcrumb}<span class="crumb-sep">/</span
        >{breadcrumb.source}<span class="crumb-sep">/</span>{breadcrumb.category}{/if}</span
    >
    <input
      class="search micro"
      type="search"
      placeholder="Search commands"
      value={search}
      oninput={(e) => onSearch((e.target as HTMLInputElement).value)}
    />
    <div class="spacer"></div>
    <button type="button" class="icon-button micro" title="Toggle theme ({theme})" onclick={onToggleTheme}>
      {#if theme === "dark"}
        ●
      {:else if theme === "light"}
        ○
      {:else}
        ◐
      {/if}
    </button>
    <button type="button" class="icon-button micro" title="Settings" onclick={onOpenSettings}>
      ⚙
    </button>
  </div>
</header>

<style>
  .toolbar {
    position: relative;
    height: 46px;
    flex: none;
    border-bottom: 1px solid var(--line);
    background: var(--panel);
    box-shadow: inset 0 1px 0 var(--emboss-light);
  }

  .drag-region {
    position: absolute;
    inset: 0;
  }

  .content {
    position: relative;
    height: 100%;
    display: flex;
    align-items: center;
    gap: var(--space-3);
    /* Inset for macOS traffic lights (overlay title bar). */
    padding: 0 var(--space-3) 0 78px;
  }

  /* Chrome is the plain `button` rule below (border/panel-bg/emboss+shadow)
     — the design reference calls for the same bordered-panel look as every
     other toolbar button, so this only adds what's specific to sitting
     left of the breadcrumb: no shrink, no wrap. */
  .back-btn {
    flex: none;
    white-space: nowrap;
  }

  .repo-name {
    font-size: 12px;
    color: var(--muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 420px;
  }

  .crumb-sep {
    color: var(--line);
    margin: 0 var(--space-2);
  }

  button,
  input {
    position: relative;
  }

  .search {
    flex: 0 1 220px;
    padding: 5px 9px;
    border: 1px solid var(--line);
    border-radius: 5px;
    background: var(--bg);
    color: var(--ink);
    font-family: var(--font-mono);
    font-size: 11.5px;
    /* Recessed, like the LED well — a shallow cut into the panel rather
       than a flat input sitting on top of it. */
    box-shadow: inset 0 1px 2px var(--well-inset, rgba(0, 0, 0, 0.22));
  }

  .spacer {
    flex: 1;
  }

  button {
    border: 1px solid var(--line);
    border-radius: 5px;
    background: var(--panel);
    color: var(--ink);
    padding: 5px 11px;
    font-size: 12px;
    /* Raised, matching the modules/pads: an emboss highlight plus a soft
       outer shadow so buttons read as actual controls, not flat chips. */
    box-shadow:
      inset 0 1px 0 var(--emboss-light),
      0 1px 2px rgba(0, 0, 0, 0.18);
  }

  button:hover {
    border-color: var(--muted);
  }

  .icon-button {
    padding: var(--space-1) var(--space-2);
    width: 28px;
    height: 26px;
    text-align: center;
  }
</style>
