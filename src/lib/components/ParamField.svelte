<script lang="ts">
  import type { Param } from "../types";

  interface Props {
    param: Param;
    value: string;
    onChange: (value: string) => void;
  }

  let { param, value, onChange }: Props = $props();

  const isPickOptions = $derived(param.kind === "pick" && !!param.options);
  const isPickSource = $derived(param.kind === "pick" && !!param.source && !param.options);
</script>

<div class="field">
  <label class="label mono" for="param-{param.name}">{param.name}</label>

  {#if isPickOptions}
    <select id="param-{param.name}" value={value} onchange={(e) => onChange((e.target as HTMLSelectElement).value)}>
      <option value="" disabled selected={value === ""}>Choose…</option>
      {#each param.options ?? [] as opt (opt)}
        <option value={opt}>{opt}</option>
      {/each}
    </select>
  {:else if isPickSource}
    <input
      id="param-{param.name}"
      type="text"
      value={value}
      oninput={(e) => onChange((e.target as HTMLInputElement).value)}
      placeholder={param.default ?? ""}
    />
    <p class="helper">options come from the repository at prompt time</p>
  {:else if param.secret}
    <input
      id="param-{param.name}"
      type="password"
      value={value}
      oninput={(e) => onChange((e.target as HTMLInputElement).value)}
      autocomplete="off"
    />
  {:else}
    <input
      id="param-{param.name}"
      type="text"
      value={value}
      oninput={(e) => onChange((e.target as HTMLInputElement).value)}
      placeholder={param.default ?? ""}
    />
  {/if}
</div>

<style>
  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    margin-bottom: var(--space-3);
  }

  .label {
    font-size: 12px;
    color: var(--muted);
  }

  input,
  select {
    padding: var(--space-2) var(--space-2);
    border: 1px solid var(--line);
    border-radius: var(--radius-control);
    background: var(--bg);
    color: var(--ink);
    font-size: 13px;
    max-width: 360px;
  }

  .helper {
    margin: 0;
    font-size: 12px;
    color: var(--muted);
  }
</style>
