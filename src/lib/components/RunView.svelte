<script lang="ts">
  import type { CommandInfo, DoctorReport } from "../types";
  import { meterStateFor, readinessFor, readinessLabel } from "../readiness";
  import Meter from "./Meter.svelte";
  import ParamField from "./ParamField.svelte";
  import OutputPane from "./OutputPane.svelte";

  interface OutputLine {
    stream: "stdout" | "stderr" | "exit";
    text: string;
  }

  interface Props {
    command: CommandInfo;
    trusted: boolean;
    doctorReport: DoctorReport | null;
    running: boolean;
    outputLines: OutputLine[];
    onRun: (values: Record<string, string>) => void;
    onBack: () => void;
  }

  let { command, trusted, doctorReport, running, outputLines, onRun, onBack }: Props = $props();

  let values: Record<string, string> = $state({});

  $effect(() => {
    // Reset the form whenever the selected command changes.
    const initial: Record<string, string> = {};
    for (const p of command.params) {
      initial[p.name] = p.default ?? "";
    }
    values = initial;
  });

  const readiness = $derived(readinessFor(command, trusted, doctorReport));
  const label = $derived(readinessLabel(readiness));
  const meterState = $derived(meterStateFor(readiness, running));

  const disabledReason = $derived.by(() => {
    if (command.interactive) return `Needs a real terminal — run \`pult ${command.id}\` in one.`;
    if (!trusted) return "Trust this repository to run commands.";
    if (running) return "Already running.";
    return null;
  });

  function onKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") onBack();
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div class="run-view">
  <button type="button" class="back micro" onclick={onBack}>← Board</button>

  <header class="header">
    <div class="title-row">
      <Meter state={meterState} size="lg" seed={command.id} />
      <h1 class="title">{command.title}</h1>
    </div>
    {#if command.description}
      <p class="description">{command.description}</p>
    {/if}
    <div class="meta">
      <span class="status">{label}</span>
      <span class="dot">·</span>
      <code class="id mono">{command.id}</code>
      {#if command.category}
        <span class="dot">·</span>
        <span class="category">{command.category}</span>
      {/if}
    </div>
  </header>

  {#if readiness === "failed"}
    <div class="check-failed">
      <span class="check-label">Check failed</span>
      <code class="mono">{command.check}</code>
    </div>
  {/if}

  <div class="body">
    {#if command.params.length > 0}
      <form class="params">
        {#each command.params as param (param.name)}
          <ParamField
            {param}
            value={values[param.name] ?? ""}
            onChange={(v) => (values = { ...values, [param.name]: v })}
          />
        {/each}
      </form>
    {/if}

    <div class="run-row">
      <button
        type="button"
        class="run micro"
        disabled={!!disabledReason}
        onclick={() => onRun(values)}
      >
        {running ? "Running…" : `Run ${command.title}`}
      </button>
      {#if disabledReason}
        <span class="hint">{disabledReason}</span>
      {/if}
    </div>

    {#if outputLines.length > 0}
      <OutputPane lines={outputLines} />
    {/if}
  </div>
</div>

<style>
  .run-view {
    height: 100%;
    display: flex;
    flex-direction: column;
    padding: var(--space-6);
    gap: var(--space-4);
    overflow-y: auto;
  }

  .back {
    align-self: flex-start;
    border: 1px solid var(--line);
    background: var(--panel);
    color: var(--ink);
    border-radius: var(--radius-control);
    padding: var(--space-1) var(--space-3);
    font-size: 12px;
  }

  .back:hover {
    border-color: var(--muted);
  }

  .header {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .title-row {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }

  .title {
    margin: 0;
    font-size: 15px;
    font-weight: 600;
    letter-spacing: -0.01em;
  }

  .description {
    margin: 0;
    color: var(--muted);
    max-width: 64ch;
    line-height: 1.5;
  }

  .meta {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--muted);
    font-size: 12px;
  }

  .dot {
    color: var(--line);
  }

  .check-failed {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    padding: var(--space-3);
    border: 1px solid var(--lamp-red);
    border-radius: var(--radius-control);
    background: color-mix(in srgb, var(--lamp-red) 8%, transparent);
    max-width: 560px;
  }

  .check-label {
    font-size: 12px;
    font-weight: 500;
    color: var(--lamp-red);
  }

  .body {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    flex: 1;
    min-height: 0;
  }

  .params {
    display: flex;
    flex-direction: column;
  }

  .run-row {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }

  .run {
    border: 1px solid var(--accent);
    background: var(--accent);
    color: var(--accent-ink);
    border-radius: var(--radius-control);
    padding: var(--space-2) var(--space-4);
    font-size: 13px;
    font-weight: 500;
  }

  .run:hover:not(:disabled) {
    filter: brightness(1.05);
  }

  .run:disabled {
    background: var(--line);
    border-color: var(--line);
    color: var(--muted);
    opacity: 0.8;
  }

  .hint {
    color: var(--muted);
    font-size: 12px;
  }
</style>
