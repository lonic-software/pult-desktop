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
    /** Phosphor persistence bloom (design backlog) — set by +page.svelte's
     *  `applyEvent` only on a genuinely live-arriving `stdout`/`stderr` line
     *  (see `OutputLine.freshAt`'s doc comment in `$lib/types.ts` for the
     *  live-vs-replay determination). `isBloomFresh` below is what actually
     *  decides whether to play the animation right now — this field alone
     *  never does. */
    freshAt?: number;
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

  // Phosphor persistence bloom (design backlog): how long after `freshAt`
  // a line still gets the `.bloom` class — comfortably longer than the CSS
  // animation itself (~350ms, see `.line.bloom` below) so the class is still
  // present when the animation naturally finishes playing (an element
  // doesn't need a class held past its own animation's end, but there's no
  // reason to race removing it either), yet short enough that a line is
  // reliably judged "not fresh" again well before anyone could plausibly
  // switch views away and back to see it.
  const BLOOM_RECENT_MS = 600;

  // Deliberately reads `Date.now()` directly rather than through `$state`/
  // `$derived` — Svelte only re-runs a template expression when one of ITS
  // OWN reactive reads changes, and `line.freshAt` (once set) never changes
  // again, so this expression naturally evaluates exactly once: whenever
  // this specific line's DOM node is first created. For a newly-appended
  // live line that's the moment it's born — `now` is fresh, `.bloom` applies,
  // and the browser plays the animation on that brand-new node exactly like
  // any element rendered with an animating class already on it. For an
  // OLDER line rendered again because the whole pane remounted (switching
  // the view away and back — RunView/OutputPane are destroyed and recreated,
  // not merely re-rendered), that's the moment its DOM node is (re)created
  // in the new mount, but `now - line.freshAt` is by then long past
  // `BLOOM_RECENT_MS`, so it correctly comes out false — no re-bloom of
  // history. A bare boolean stamped once at append time couldn't make this
  // distinction: it wouldn't know how long ago "fresh" happened by the time
  // a remount asks again.
  function isBloomFresh(line: OutputLine): boolean {
    return line.freshAt !== undefined && Date.now() - line.freshAt < BLOOM_RECENT_MS;
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
      <span class="line {line.stream}" class:bloom={isBloomFresh(line)}>
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

  /* Phosphor persistence bloom (design backlog, `isBloomFresh` above): a
     freshly-drawn live line flashes brighter than the standing phosphor
     level, then settles — the optical twin of the cooling fade below, at
     the opposite end of a line's life (drawn vs. going stale). `filter:
     brightness` rather than `color`/`opacity` so it composes for free with
     everything already layered on a line: it's a property nothing else
     here touches, it visually brightens the ANSI segments nested inside
     (`filter` affects an element's whole rendered subtree, not just its own
     box, so the colored spans from `segmentStyle` above brighten right
     along with the line's own ink), and it doesn't fight the `.dim`/cooling
     opacity transition below if a run happens to finish moments after its
     last line bloomed (independent properties, same element). `text-shadow`
     uses `currentColor` rather than a literal — it's an inherited property,
     so an ANSI-colored child span (which sets its own `color` but no
     `text-shadow`) still glows in ITS own color, not the line's default ink,
     while a plain stdout/stderr span glows in the line's own phosphor/red.
     No font-weight/size change — only glow, so nothing reflows.
     Short (~350ms) and one-shot per element: applying `.bloom` to a brand
     new DOM node plays the animation from its start the moment the node is
     inserted, exactly like a node created with any other animating class
     already on it — no JS-driven restart needed. Reduced motion is handled
     globally (global.css's `prefers-reduced-motion` block collapses every
     animation-duration to near zero), so no local override is needed here. */
  .line.bloom {
    animation: line-bloom 350ms ease-out;
  }

  @keyframes line-bloom {
    from {
      filter: brightness(1.6);
      text-shadow: 0 0 6px currentColor;
    }
    to {
      filter: brightness(1);
      text-shadow: 0 0 0 transparent;
    }
  }

  /* Phosphor palette (crt.css's `--crt-*` custom properties, inherited from
     the ancestor `.pult-crt` regardless of Svelte's per-component scoping —
     see crt.css's file comment) with a literal fallback in case this pane
     is ever rendered outside that ancestor. */
  /* Phosphor-cooling decay: when a run finishes and the pane dims (see the
     `.output.dim` rule below), stdout/stderr opacity should fade rather than
     snap. A real CRT tube's afterglow actually front-loads its decay (fast
     dim, then a long tail), but that's the opposite of what this feature
     needs: a transition that dims most of the way in the first few hundred
     ms reads as an instant cut, not a fade — visibility beats physical
     accuracy here. So the curve is a slow-in/slow-out sigmoid (roughly
     symmetric, not eased-out) over several seconds, long enough that the eye
     can actually track the light draining rather than notice it's already
     gone. This rule (leaving the dim state, i.e. un-dimming when a new run
     starts) has no delay: `dim` flips false the moment a run starts, and
     RunView's history clears/replaces lines for a fresh run anyway, so any
     visible un-dim here is on lines that are about to be replaced regardless
     — no need to hold it.
     The `transition` has to live here, on the *base* rule that applies in
     both the bright and dim states, not only inside `.dim` — a transition
     only animates a property change on the element it's already declared
     on, so a rule scoped to `.dim` alone would apply the property at the
     same instant the opacity value changes and never get a chance to run.
     Declaring it here costs nothing on newly mounted lines: Svelte's DOM
     insertion doesn't count as a transitionable property change (there's no
     prior computed value to interpolate from), so appended lines still
     render at full brightness immediately with no fade-in.
     Reduced motion is handled globally (global.css collapses all
     transition/animation durations under prefers-reduced-motion), so no
     local override is needed here. */
  .line.stdout {
    color: var(--crt-ink, #a9c9ab);
    transition: opacity 4s cubic-bezier(0.45, 0.05, 0.55, 0.95);
  }

  .line.stderr {
    color: var(--crt-red, #e88a7a);
    transition: opacity 4s cubic-bezier(0.45, 0.05, 0.55, 0.95);
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
    /* Entering dim gets a short delay the base rule above doesn't: since
       transitions are read from the destination state's own rule, this
       (more specific) `transition` wins over the base one only while `.dim`
       applies, letting the exit summary land and get read for a beat before
       the glow visibly starts cooling — makes the fade read as a
       consequence of the run ending rather than a random animation kicking
       off mid-scroll. Un-dimming (this rule ceasing to apply) falls back to
       the base rule's undelayed transition, see its comment above. */
    transition: opacity 4s cubic-bezier(0.45, 0.05, 0.55, 0.95) 250ms;
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
