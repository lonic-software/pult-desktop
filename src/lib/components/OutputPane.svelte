<script lang="ts">
  import { parseAnsiLine, type Segment } from "../ansi";

  interface OutputLine {
    stream: "stdout" | "stderr" | "exit";
    text: string;
    /** Only set on the `exit` stream's line — which of the four summary
     *  forms it is, so the caller (RunView) doesn't have to re-derive it
     *  from text. Colors the line; see the `.exit` rules below. `"crashed"`
     *  gets the same red as `"error"` (see `.outcome-crashed` below) — the
     *  distinction is in the wording, not the color. */
    outcome?: "success" | "error" | "stopped" | "crashed";
  }

  interface Props {
    lines: OutputLine[];
    /** Shows the blinking accent cursor after the last line. */
    running?: boolean;
    /** Idle, showing a previous run's output rather than a live one — dims
     *  stdout/stderr text (the exit summary keeps its outcome color; it's
     *  still the meaningful "how did it end" answer even in replay). */
    dim?: boolean;
    /** Mirrors `RunRecord.interactive` (docs/run-journal.md, "Interactive
     *  commands"): the terminal owns the tty, so pult journals meta + exit
     *  only — `lines` never gets a stdout/stderr entry for one of these,
     *  live or replayed. Renders an explanatory line in place of what would
     *  otherwise be a blank-looking pane; see `showInteractiveNote` below. */
    interactive?: boolean;
  }

  let { lines, running = false, dim = false, interactive = false }: Props = $props();
  let containerEl: HTMLDivElement | undefined = $state();

  // True whenever this is an interactive run and no actual output has (or
  // ever will have) arrived — which for an interactive run is always, since
  // the spec guarantees no stdout/stderr lines are ever journaled for one.
  // The `some` check is defensive rather than load-bearing. Deliberately NOT
  // folded into a single "lines.length === 0" replacement — a finished
  // interactive run's `lines` still carries the real journaled exit summary
  // (see +page.svelte's `finish`), and that line renders below this note,
  // not instead of it, so the two compose rather than one clobbering the
  // other.
  const showInteractiveNote = $derived(
    interactive && !lines.some((l) => l.stream === "stdout" || l.stream === "stderr"),
  );

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

  // ANSI segment rendering (stdout/stderr text only — see ansi.ts's module
  // comment for the parser itself). SECURITY: this only ever produces a
  // `style` attribute string built from values this module computed itself
  // (a fixed "ansi-N" token or a hex string assembled byte-by-byte in
  // ansi.ts's `toHex`) — segment *text* always goes through plain Svelte
  // text interpolation below, never `{@html}`.
  function segmentStyle(seg: Segment): string {
    const parts: string[] = [];
    if (seg.fg) parts.push(`color: ${colorValue(seg.fg)}`);
    if (seg.bg) parts.push(`background-color: ${colorValue(seg.bg)}`);
    if (seg.bold) parts.push("font-weight: 700");
    if (seg.dim) parts.push("opacity: 0.7");
    if (seg.italic) parts.push("font-style: italic");
    if (seg.underline) parts.push("text-decoration: underline");
    return parts.join("; ");
  }

  // Palette tokens ("ansi-0".."ansi-15") resolve through this pane's own
  // custom properties (see the .output rules below) so the 16-color
  // palette stays tunable in one place; anything else is already a literal
  // "#rrggbb" from ansi.ts's 256-color/truecolor math — used as-is.
  function colorValue(token: string): string {
    return token.startsWith("ansi-") ? `var(--${token})` : token;
  }
</script>

<div class="output mono pult-screen pult-crt-glow" class:dim bind:this={containerEl} onscroll={onScroll}>
  {#if showInteractiveNote}
    <span class="line note">{running
        ? "running in a terminal — output stays there (interactive command)"
        : "ran in a terminal — output wasn't captured (interactive command)"}</span>
  {/if}
  {#each lines as line, i (i)}
    {#if line.stream === "stdout" || line.stream === "stderr"}
      <span class="line {line.stream}">
        {#each parseAnsiLine(line.text) as seg, si (si)}
          <span style={segmentStyle(seg)}>{seg.text}</span>
        {/each}
      </span>
    {:else}
      <span class="line {line.stream}" class:outcome-success={line.outcome === "success"} class:outcome-error={line.outcome === "error"} class:outcome-stopped={line.outcome === "stopped"} class:outcome-crashed={line.outcome === "crashed"}
        >{line.text}</span
      >
    {/if}
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

    /* ANSI 16-color palette (parsed by ../ansi.ts, consumed via
       segmentStyle/colorValue above as `var(--ansi-N)`) — scoped here
       rather than in crt.css because only this pane's stdout/stderr text
       ever needs it. Deliberately ONE palette, not a light/dark pair: this
       screen's own background (--crt-bg, crt.css) is a fixed phosphor tint
       that doesn't change with the app's light/dark theme (crt.css's file
       comment — "a physical tube doesn't change with room lighting"), so
       there's nothing for a second theme variant to adapt to. Hand-tuned
       against --crt-bg (#10160f) rather than pasted stock VGA values: hues
       stay recognizable as the classic 16 (red/green/yellow/blue/magenta/
       cyan + neutrals, each with a brighter 8-15 counterpart) but pulled
       toward the same warm/cool balance as the rest of the phosphor
       palette below, so colored output reads as part of this screen
       instead of a foreign terminal pasted on top of it. */
    --ansi-0: #3b4a3f;
    --ansi-1: #e88a7a;
    --ansi-2: #7ed492;
    --ansi-3: #d9c46a;
    --ansi-4: #6fa8d8;
    --ansi-5: #c98fd9;
    --ansi-6: #6fd6d0;
    --ansi-7: #a9c9ab;
    --ansi-8: #7c8f80;
    --ansi-9: #ff8a75;
    --ansi-10: #9de8ac;
    --ansi-11: #f0d98a;
    --ansi-12: #93c3ec;
    --ansi-13: #e0aeef;
    --ansi-14: #8ceae4;
    --ansi-15: #e9f5ea;
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

  /* The interactive-run placeholder (see `showInteractiveNote` above) —
     dimmed like replayed stdout/stderr rather than the ink color, since it's
     explanatory chrome, not actual command output. */
  .line.note {
    color: var(--crt-dim, #5f7563);
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

  /* Same red as a plain nonzero-exit error — a crash's honest copy is what
     sets it apart (see +page.svelte's `finish`), not a separate color; both
     are the same "this run did not end well" red-family verdict. */
  .line.exit.outcome-crashed {
    color: var(--crt-red, #e88a7a);
    border-top-color: transparent;
  }

  /* Replayed (idle, prior-run) output — stdout/stderr text dims; the exit
     summary keeps its outcome color, see the Props comment above. Dim via
     `opacity` alone rather than also swapping `color`: a line's ANSI-colored
     segments carry their own inline `color`/`background-color` (see
     segmentStyle above) that overrides any `color` set here, so a combined
     color+opacity rule only ever muted the plain-text default while ANSI
     spans stayed bright — inconsistent and, for plain text, low-contrast
     against the CRT glass once the ambient glow/shine layers wash extra
     light over it. Opacity alone dims every segment uniformly and preserves
     each segment's hue relationships. */
  .output.dim .line.stdout,
  .output.dim .line.stderr {
    opacity: 0.65;
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
