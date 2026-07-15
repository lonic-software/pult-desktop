<script lang="ts">
  interface Props {
    repoName: string | null;
    search: string;
    theme: "system" | "light" | "dark";
    onOpenRepo: () => void;
    onSearch: (value: string) => void;
    onToggleTheme: () => void;
    onOpenSettings: () => void;
  }

  let { repoName, search, theme, onOpenRepo, onSearch, onToggleTheme, onOpenSettings }: Props =
    $props();
</script>

<header class="toolbar">
  <div class="drag-region" data-tauri-drag-region></div>
  <div class="content">
    <span class="repo-name mono">{repoName ?? "No repository open"}</span>
    <button type="button" class="micro" onclick={onOpenRepo}>Open repository…</button>
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
    height: 44px;
    flex: none;
    border-bottom: 1px solid var(--line);
    background: var(--panel);
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

  .repo-name {
    font-size: 12px;
    color: var(--muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 220px;
  }

  button,
  input {
    position: relative;
  }

  .search {
    flex: 0 1 260px;
    padding: var(--space-1) var(--space-2);
    border: 1px solid var(--line);
    border-radius: var(--radius-control);
    background: var(--bg);
    color: var(--ink);
  }

  .spacer {
    flex: 1;
  }

  button {
    border: 1px solid var(--line);
    border-radius: var(--radius-control);
    background: var(--panel);
    color: var(--ink);
    padding: var(--space-1) var(--space-3);
    font-size: 12px;
  }

  button:hover {
    border-color: var(--muted);
  }

  .icon-button {
    padding: var(--space-1) var(--space-2);
    min-width: 28px;
    text-align: center;
  }
</style>
