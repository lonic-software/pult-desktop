<script lang="ts">
  import { untrack } from "svelte";
  import type { Param } from "../types";
  import { resolvePickSource } from "../api";

  interface Props {
    param: Param;
    value: string;
    onChange: (value: string) => void;
    /** Repo path + command id — needed to resolve a dynamic `pick.source`. */
    path: string;
    commandId: string;
    /** Every param's current value in this form, so a `depends_on` param
     *  can be read without RunView threading each one through separately. */
    values: Record<string, string>;
    /** Whether the open repo is trusted. A `pick.source` is manifest-authored
     *  shell code, same as a `check:` — resolving it is gated on trust the
     *  same way `pult doctor` is, so we don't even attempt it (and don't show
     *  a resolve error) until the repo is trusted. */
    trusted: boolean;
  }

  let { param, value, onChange, path, commandId, values, trusted }: Props = $props();

  const isPickOptions = $derived(param.kind === "pick" && !!param.options);
  const isPickSource = $derived(param.kind === "pick" && !!param.source && !param.options);
  const dependsOn = $derived(param.depends_on ?? []);
  const missingDeps = $derived(dependsOn.filter((name) => !(values[name] ?? "").trim()));
  const dependsOnReady = $derived(missingDeps.length === 0);
  // A snapshot of just the depends_on values, so the effect below can tell
  // "still ready" from "ready with a changed value" without re-resolving on
  // every keystroke elsewhere in the form.
  const dependsOnKey = $derived(JSON.stringify(dependsOn.map((name) => values[name] ?? "")));

  let options: string[] = $state([]);
  let loading = $state(false);
  let resolveError: string | null = $state(null);

  // Internal bookkeeping, not reactive state — used only to tell "the first
  // time this param became resolvable" (resolve immediately) from "a
  // depends_on value changed while already resolvable" (debounce ~300ms)
  // per the spec, and to invalidate a stale value when depends_on changes.
  let wasReady = false;
  let lastDependsOnKey: string | null = null;
  let debounceHandle: ReturnType<typeof setTimeout> | undefined;
  let requestSeq = 0;

  $effect(() => {
    if (!isPickSource || !trusted) return;

    const ready = dependsOnReady;
    const key = dependsOnKey;

    if (!ready) {
      options = [];
      resolveError = null;
      loading = false;
      // `value` is read via `untrack` so clearing it here doesn't itself
      // retrigger this effect (it isn't a `depends_on` of this param, so a
      // change to it should never re-schedule a resolve) — it would
      // otherwise cancel its own pending debounce/resolve below by
      // re-running mid-flight.
      if (wasReady && untrack(() => value)) onChange(""); // depends_on regressed — stale value can't stand
      wasReady = false;
      lastDependsOnKey = key;
      return;
    }

    const isFirstResolve = !wasReady;
    const changed = key !== lastDependsOnKey;
    wasReady = true;
    lastDependsOnKey = key;
    if (!isFirstResolve && !changed) return; // nothing actually changed

    // A depends_on value changed underneath an already-picked value: clear
    // it immediately (not just once the new resolution lands) so a stale
    // choice can never be submitted mid-debounce/mid-resolve. See the
    // `untrack` note above — same reasoning applies here.
    if (changed && !isFirstResolve && untrack(() => value)) onChange("");

    const depValues: Record<string, string> = {};
    for (const name of dependsOn) depValues[name] = values[name] ?? "";

    if (debounceHandle) clearTimeout(debounceHandle);
    loading = true;
    resolveError = null;
    const mySeq = ++requestSeq;

    const resolve = async () => {
      try {
        const resolved = await resolvePickSource(path, commandId, param.name, depValues);
        if (mySeq !== requestSeq) return; // superseded by a newer request
        options = resolved;
        loading = false;
        // Preserve a manually-typed/previously-picked value if it's still
        // among the resolved options (the first-resolve case, where nothing
        // was cleared above); otherwise it no longer means anything.
        if (value && !resolved.includes(value)) onChange("");
      } catch (e) {
        if (mySeq !== requestSeq) return;
        resolveError = String(e);
        loading = false;
        options = [];
      }
    };

    if (isFirstResolve) {
      void resolve();
    } else {
      debounceHandle = setTimeout(() => void resolve(), 300);
    }

    return () => {
      if (debounceHandle) clearTimeout(debounceHandle);
    };
  });
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
  {:else if isPickSource && !trusted}
    <select id="param-{param.name}" disabled>
      <option value="">Choose…</option>
    </select>
    <p class="helper">trust this repository first</p>
  {:else if isPickSource && !dependsOnReady}
    <select id="param-{param.name}" disabled>
      <option value="">Choose…</option>
    </select>
    <p class="helper">fill {missingDeps.join(", ")} first</p>
  {:else if isPickSource && loading}
    <select id="param-{param.name}" disabled>
      <option value="">Loading…</option>
    </select>
    <p class="helper">resolving options…</p>
  {:else if isPickSource && resolveError}
    <input
      id="param-{param.name}"
      type="text"
      value={value}
      oninput={(e) => onChange((e.target as HTMLInputElement).value)}
      placeholder={param.default ?? ""}
    />
    <p class="helper">options come from the repository at prompt time</p>
    <p class="helper helper-error">couldn't resolve options: {resolveError}</p>
  {:else if isPickSource}
    <select id="param-{param.name}" value={value} onchange={(e) => onChange((e.target as HTMLSelectElement).value)}>
      <option value="" disabled selected={value === ""}>Choose…</option>
      {#each options as opt (opt)}
        <option value={opt}>{opt}</option>
      {/each}
    </select>
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

  .helper-error {
    color: var(--lamp-red);
  }
</style>
