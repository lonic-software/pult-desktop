<script lang="ts">
  interface OutputLine {
    stream: "stdout" | "stderr" | "exit";
    text: string;
    /** Only set on the `exit` stream's line — which of the three summary
     *  forms it is, so the caller (RunView) doesn't have to re-derive it
     *  from text. Colors the line; see the `.exit` rules below. */
    outcome?: "success" | "error" | "stopped";
  }

  interface Props {
    lines: OutputLine[];
    /** Shows the blinking accent cursor after the last line. */
    running?: boolean;
    /** Idle, showing a previous run's output rather than a live one — dims
     *  stdout/stderr text (the exit summary keeps its outcome color; it's
     *  still the meaningful "how did it end" answer even in replay). */
    dim?: boolean;
  }

  let { lines, running = false, dim = false }: Props = $props();
  let containerEl: HTMLDivElement | undefined = $state();

  // Auto-scroll to the newest line, but only if the user hasn't scrolled up
  // to read earlier output — otherwise a fast-scrolling run would keep
  // yanking them back to the bottom mid-read. "Near the bottom" allows a few
  // px of slack for sub-pixel scroll rounding.
  const BOTTOM_SLACK_PX = 4;
  let stickToBottom = true;

  function onScroll() {
    if (!containerEl) return;
    const { scrollTop, scrollHeight, clientHeight } = containerEl;
    stickToBottom = scrollHeight - scrollTop - clientHeight <= BOTTOM_SLACK_PX;
  }

  $effect(() => {
    // Re-run whenever `lines` changes; autoscroll only if already at bottom.
    lines.length;
    if (containerEl && stickToBottom) {
      containerEl.scrollTop = containerEl.scrollHeight;
    }
  });
</script>

<div class="output mono pult-screen pult-crt-glow" class:dim bind:this={containerEl} onscroll={onScroll}>
  {#each lines as line, i (i)}
    <span class="line {line.stream}" class:outcome-success={line.outcome === "success"} class:outcome-error={line.outcome === "error"} class:outcome-stopped={line.outcome === "stopped"}
      >{line.text}</span
    >
  {/each}
  {#if running}
    <span class="cursor" aria-hidden="true">▌</span>
  {/if}
</div>

<style>
  /* CRT phosphor restyle (design variant 3c) — this pane's root *is* the
     `.pult-screen` inside RunView's `.pult-crt` output wrapper (see
     RunView's output-module comment): the scrolling element and the
     component that owns the scroll bookkeeping above are the same node, so
     the classes go straight on it rather than through an extra wrapper.
     `.pult-crt`/`.pult-screen`'s shared rules (scan lines, scrollbar,
     phosphor custom properties) live in crt.css; only the padding/flex-fill
     and text colors below are specific to this pane. Auto-scroll logic
     above is untouched — CSS only. */
  .output {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 13px 15px;
  }

  .line {
    white-space: pre-wrap;
    word-break: break-word;
    line-height: 1.5;
    font-size: 11.5px;
  }

  /* Phosphor palette (crt.css's `--crt-*` custom properties, inherited from
     the ancestor `.pult-crt` regardless of Svelte's per-component scoping —
     see crt.css's file comment) with a literal fallback in case this pane
     is ever rendered outside that ancestor. */
  .line.stdout {
    color: var(--crt-ink, #a9c9ab);
  }

  .line.stderr {
    color: var(--crt-red, #e88a7a);
  }

  .line.exit {
    margin-top: var(--space-1);
    padding-top: var(--space-2);
    border-top: 1px solid var(--crt-line, rgba(158, 214, 160, 0.16));
    color: var(--crt-ink, #a9c9ab);
    font-weight: 500;
  }

  .line.exit.outcome-success {
    color: var(--crt-green, #7ed492);
    border-top-color: transparent;
  }

  .line.exit.outcome-error {
    color: var(--crt-red, #e88a7a);
    border-top-color: transparent;
  }

  .line.exit.outcome-stopped {
    color: var(--crt-amber, #e0c274);
    border-top-color: transparent;
  }

  /* Replayed (idle, prior-run) output — stdout/stderr text dims; the exit
     summary keeps its outcome color, see the Props comment above. */
  .output.dim .line.stdout,
  .output.dim .line.stderr {
    color: var(--crt-dim, #5f7563);
  }

  .cursor {
    color: var(--crt-green, #7ed492);
    font-size: 11.5px;
    animation: cursor-blink 1s steps(1, end) infinite;
  }

  @keyframes cursor-blink {
    0%,
    49% {
      opacity: 1;
    }
    50%,
    100% {
      opacity: 0;
    }
  }
</style>
