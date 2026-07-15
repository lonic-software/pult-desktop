<script lang="ts">
  interface Props {
    currentPath: string | null;
    versionInfo: string | null;
    onSave: (path: string) => void;
    onClose: () => void;
  }

  let { currentPath, versionInfo, onSave, onClose }: Props = $props();

  let path = $state("");

  $effect(() => {
    path = currentPath ?? "";
  });
</script>

<div class="scrim">
  <div class="modal" role="dialog" aria-modal="true" aria-labelledby="settings-title">
    <h2 id="settings-title">Settings</h2>

    <div class="field">
      <span class="field-label">pult binary path</span>
      <input class="mono" type="text" bind:value={path} placeholder="/usr/local/bin/pult (leave blank to use PATH)" />
      <p class="helper">
        Sidecar bundling isn't wired up in v0 — pult must be installed separately, either on your
        PATH or pointed at here.
      </p>
    </div>

    {#if versionInfo}
      <p class="version mono">{versionInfo}</p>
    {/if}

    <div class="actions">
      <button type="button" class="secondary micro" onclick={onClose}>Close</button>
      <button type="button" class="primary micro" onclick={() => onSave(path)}>Save</button>
    </div>
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    background: color-mix(in srgb, black 40%, transparent);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
  }

  .modal {
    width: min(420px, calc(100vw - 48px));
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: var(--radius-panel);
    padding: var(--space-6);
  }

  h2 {
    margin: 0 0 var(--space-4);
    font-size: 15px;
    font-weight: 600;
    letter-spacing: -0.01em;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    margin-bottom: var(--space-4);
  }

  .field-label {
    font-size: 11px;
    font-weight: 500;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--muted);
  }

  input {
    padding: var(--space-2);
    border: 1px solid var(--line);
    border-radius: var(--radius-control);
    background: var(--bg);
    color: var(--ink);
  }

  .helper {
    margin: 0;
    font-size: 12px;
    color: var(--muted);
  }

  .version {
    font-size: 12px;
    color: var(--muted);
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    margin-top: var(--space-4);
  }

  button {
    border-radius: var(--radius-control);
    padding: var(--space-2) var(--space-4);
    font-size: 13px;
    font-weight: 500;
  }

  .secondary {
    border: 1px solid var(--line);
    background: var(--panel);
    color: var(--ink);
  }

  .primary {
    border: 1px solid var(--accent);
    background: var(--accent);
    color: var(--accent-ink);
  }
</style>
