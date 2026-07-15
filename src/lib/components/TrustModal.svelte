<script lang="ts">
  import type { Listing } from "../types";

  interface Props {
    listing: Listing;
    busy: boolean;
    onTrust: () => void;
    onNotNow: () => void;
  }

  let { listing, busy, onTrust, onNotNow }: Props = $props();
</script>

<div class="scrim">
  <div class="modal" role="dialog" aria-modal="true" aria-labelledby="trust-title">
    <h2 id="trust-title">Trust this repository?</h2>
    <p class="explain">
      Trusting means the commands below may run as you, with your permissions. Review where they
      come from before continuing.
    </p>

    <div class="field">
      <span class="field-label">Manifest</span>
      <code class="mono">{listing.manifest}</code>
    </div>

    {#if listing.includes.length > 0}
      <div class="field">
        <span class="field-label">Includes</span>
        <ul class="includes">
          {#each listing.includes as include (include.source)}
            <li>
              <code class="mono">{include.source}</code>
              {#if include.rev}
                <span class="rev mono">@ {include.rev}</span>
              {/if}
              {#if include.name}
                <span class="include-name">— {include.name}</span>
              {/if}
            </li>
          {/each}
        </ul>
      </div>
    {/if}

    <div class="actions">
      <button type="button" class="secondary micro" onclick={onNotNow} disabled={busy}>
        Not now
      </button>
      <button type="button" class="primary micro" onclick={onTrust} disabled={busy}>
        {busy ? "Trusting…" : "Trust and continue"}
      </button>
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
    width: min(480px, calc(100vw - 48px));
    max-height: calc(100vh - 96px);
    overflow-y: auto;
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: var(--radius-panel);
    padding: var(--space-6);
    /* The one deliberate exception to "no shadows". */
    box-shadow: 0 16px 48px color-mix(in srgb, black 28%, transparent);
  }

  h2 {
    margin: 0 0 var(--space-3);
    font-size: 15px;
    font-weight: 600;
    letter-spacing: -0.01em;
  }

  .explain {
    margin: 0 0 var(--space-5);
    color: var(--muted);
    line-height: 1.5;
  }

  .field {
    margin-bottom: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .field-label {
    font-size: 11px;
    font-weight: 500;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--muted);
  }

  code.mono {
    word-break: break-all;
  }

  .includes {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .includes li {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1) var(--space-2);
    align-items: baseline;
  }

  .rev {
    color: var(--muted);
  }

  .include-name {
    color: var(--muted);
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    margin-top: var(--space-6);
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

  .secondary:hover {
    border-color: var(--muted);
  }

  .primary {
    border: 1px solid var(--accent);
    background: var(--accent);
    color: var(--accent-ink);
  }

  .primary:hover {
    filter: brightness(1.05);
  }

  .primary:disabled,
  .secondary:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }
</style>
