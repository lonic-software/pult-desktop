<script lang="ts">
  interface Props {
    message: string;
    onMountDevice?: () => void;
  }

  let { message, onMountDevice }: Props = $props();
</script>

<div class="empty">
  <p>{message}</p>
  {#if onMountDevice}
    <button type="button" class="micro" onclick={onMountDevice}>Mount device…</button>
  {/if}
</div>

<style>
  .empty {
    height: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-4);
    text-align: center;
    padding: var(--space-6);
    color: var(--muted);
  }

  p {
    max-width: 42ch;
    margin: 0;
  }

  button {
    border: 1px solid var(--line);
    border-radius: var(--radius-control);
    background: var(--panel);
    color: var(--ink);
    padding: var(--space-2) var(--space-4);
  }

  button:hover {
    border-color: var(--accent);
  }
</style>
