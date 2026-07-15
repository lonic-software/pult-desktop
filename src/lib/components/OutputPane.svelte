<script lang="ts">
  interface OutputLine {
    stream: "stdout" | "stderr" | "exit";
    text: string;
  }

  interface Props {
    lines: OutputLine[];
  }

  let { lines }: Props = $props();
  let containerEl: HTMLDivElement | undefined = $state();

  $effect(() => {
    // Re-run whenever `lines` changes; autoscroll to the newest line.
    lines.length;
    if (containerEl) {
      containerEl.scrollTop = containerEl.scrollHeight;
    }
  });
</script>

<div class="output mono" bind:this={containerEl}>
  {#each lines as line, i (i)}
    <div class="line {line.stream}">{line.text}</div>
  {/each}
</div>

<style>
  .output {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    background: var(--bg);
    border: 1px solid var(--line);
    border-radius: var(--radius-control);
    padding: var(--space-3);
  }

  .line {
    white-space: pre-wrap;
    word-break: break-word;
    line-height: 1.6;
  }

  .line.stderr {
    color: var(--muted);
  }

  .line.exit {
    margin-top: var(--space-2);
    padding-top: var(--space-2);
    border-top: 1px solid var(--line);
    color: var(--ink);
    font-weight: 500;
  }
</style>
