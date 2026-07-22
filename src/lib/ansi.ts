// Minimal ANSI SGR parser for journaled stdout/stderr lines (see
// OutputPane.svelte). Turns a raw line that may contain ANSI escape
// sequences into a flat list of styled text segments the pane renders as
// plain `<span>`s — no dependencies, and never `{@html}` (see the security
// note in OutputPane.svelte's wiring).
//
// LIMITATION (documented, not a bug): SGR state does NOT persist across
// journal lines — every call to `parseAnsiLine` starts from a clean (no
// bold/fg/bg) state, even for consecutive lines from the same run. Most CLI
// tools reset (`ESC[0m`) at the end of every line they emit, so this rarely
// shows in practice, and per-line independence is what lets Svelte render
// each journaled line as an independent, keyed item rather than re-parsing
// a run's whole output as one blob every time a new line arrives.
//
// Carriage-return handling: a raw line containing `\r` (a progress-bar frame
// that overwrote itself in the source terminal, then got journaled as one
// line) is cut down to everything after the LAST `\r` *before* any escape
// parsing happens — so SGR state from an overwritten frame can never leak
// into the frame that actually "won". This is also what a real terminal
// would show if you looked at that line right now.

export interface Segment {
  text: string;
  /** Either a palette token ("ansi-0".."ansi-15" — OutputPane's stylesheet
   *  maps these to CSS custom properties so the 16-color palette can be
   *  tuned in one place) or a literal "#rrggbb" (computed from a 256-color
   *  cube/grayscale index or a truecolor triple). `undefined` = default
   *  foreground/background — the pane's own color, nothing rendered inline. */
  fg?: string;
  bg?: string;
  bold?: boolean;
  dim?: boolean;
  italic?: boolean;
  underline?: boolean;
}

interface SgrState {
  fg?: string;
  bg?: string;
  bold: boolean;
  dim: boolean;
  italic: boolean;
  underline: boolean;
}

function freshState(): SgrState {
  return { bold: false, dim: false, italic: false, underline: false };
}

const ESC = "\x1b";

function paletteToken(index: number): string {
  return `ansi-${index}`;
}

function clampByte(n: number): number {
  return Math.max(0, Math.min(255, Math.trunc(n)));
}

function toHex(r: number, g: number, b: number): string {
  const c = (v: number) => clampByte(v).toString(16).padStart(2, "0");
  return `#${c(r)}${c(g)}${c(b)}`;
}

// xterm 256-color palette: 0-15 are the same 16 basic/bright colors as
// direct SGR 30-37/90-97 (so they resolve to the same palette tokens), 16-231
// is a 6x6x6 color cube, 232-255 is a 24-step grayscale ramp. Standard xterm
// formula for the cube steps and grayscale levels.
const CUBE_STEPS = [0, 95, 135, 175, 215, 255];

function color256ToColor(nRaw: number): string {
  const n = clampByte(nRaw);
  if (n < 16) return paletteToken(n);
  if (n <= 231) {
    const i = n - 16;
    const r = CUBE_STEPS[Math.floor(i / 36) % 6];
    const g = CUBE_STEPS[Math.floor(i / 6) % 6];
    const b = CUBE_STEPS[i % 6];
    return toHex(r, g, b);
  }
  const level = 8 + (n - 232) * 10;
  return toHex(level, level, level);
}

// Applies one SGR (`ESC[...m`) escape's already-split, already-defaulted
// parameter list to `state`, returning the new state. Malformed sequences
// (missing 38/48 sub-params, out-of-range indices/components) are handled
// defensively — skipped or clamped — never thrown.
function applySgr(params: number[], state: SgrState): SgrState {
  const next: SgrState = { ...state };
  for (let i = 0; i < params.length; i++) {
    const p = params[i];
    if (p === 0) {
      next.fg = undefined;
      next.bg = undefined;
      Object.assign(next, freshState());
    } else if (p === 1) {
      next.bold = true;
    } else if (p === 2) {
      next.dim = true;
    } else if (p === 3) {
      next.italic = true;
    } else if (p === 4) {
      next.underline = true;
    } else if (p === 22) {
      next.bold = false;
      next.dim = false;
    } else if (p === 23) {
      next.italic = false;
    } else if (p === 24) {
      next.underline = false;
    } else if (p === 7) {
      // Reverse video: swap fg/bg immediately rather than tracking a
      // "reversed" boolean flag. Given the module's documented per-line
      // (rather than fully stateful, cross-escape) design, an immediate
      // swap is the simplest faithful behavior for the common case (a
      // single 7 in a line); see the 27 branch below for the tradeoff this
      // makes.
      const { fg, bg } = next;
      next.fg = bg;
      next.bg = fg;
    } else if (p === 27) {
      // "Not reversed" — the counterpart to 7. There's no stored reversed
      // flag to clear here (see the 7 branch's comment), so this is a
      // documented no-op: a line that emits 7 then later 27 without any
      // color codes in between round-trips correctly (the second swap
      // undoes the first), but 27 alone (with no preceding 7 in the same
      // line) has nothing to undo, matching real terminals in that one case
      // and diverging only in the rarer case of 27 clearing a reverse that
      // was left over from a previous escape — which per-line parsing
      // already doesn't carry anyway.
      const { fg, bg } = next;
      next.fg = bg;
      next.bg = fg;
    } else if (p >= 30 && p <= 37) {
      next.fg = paletteToken(p - 30);
    } else if (p >= 90 && p <= 97) {
      next.fg = paletteToken(p - 90 + 8);
    } else if (p === 39) {
      next.fg = undefined;
    } else if (p >= 40 && p <= 47) {
      next.bg = paletteToken(p - 40);
    } else if (p >= 100 && p <= 107) {
      next.bg = paletteToken(p - 100 + 8);
    } else if (p === 49) {
      next.bg = undefined;
    } else if (p === 38 || p === 48) {
      const target = p === 38 ? "fg" : "bg";
      const mode = params[i + 1];
      if (mode === 5) {
        const idx = params[i + 2];
        if (idx !== undefined) next[target] = color256ToColor(idx);
        i += 2;
      } else if (mode === 2) {
        const r = params[i + 2];
        const g = params[i + 3];
        const b = params[i + 4];
        if (r !== undefined && g !== undefined && b !== undefined) {
          next[target] = toHex(r, g, b);
        }
        i += 4;
      } else {
        // Malformed 38/48 (missing or unrecognized mode byte) — skip just
        // this code rather than let a bogus trailing param get
        // misinterpreted as an unrelated SGR code on the next loop turn.
        i += 1;
      }
    }
    // Everything else (5 blink, 9 strikethrough, font selection, etc.) is
    // intentionally ignored — no Segment field renders it.
  }
  return next;
}

// Splits `line` into segments, applying SGR escapes as it goes and
// stripping every other escape sequence (CSI cursor/erase codes, OSC,
// truncated/malformed escapes) without ever letting an escape byte end up
// in a segment's `text`.
function tokenize(line: string): Segment[] {
  const segments: Segment[] = [];
  let state = freshState();
  let buf = "";

  function flush() {
    if (buf === "") return;
    segments.push({
      text: buf,
      fg: state.fg,
      bg: state.bg,
      bold: state.bold || undefined,
      dim: state.dim || undefined,
      italic: state.italic || undefined,
      underline: state.underline || undefined,
    });
    buf = "";
  }

  const n = line.length;
  let i = 0;
  while (i < n) {
    const ch = line[i];
    if (ch !== ESC) {
      buf += ch;
      i++;
      continue;
    }

    const kind = line[i + 1];
    if (kind === "[") {
      // CSI: ESC '[' parameter-bytes(0x30-0x3F)* intermediate-bytes(0x20-0x2F)* final-byte(0x40-0x7E)
      let j = i + 2;
      while (j < n && line.charCodeAt(j) >= 0x30 && line.charCodeAt(j) <= 0x3f) j++;
      const paramStr = line.slice(i + 2, j);
      while (j < n && line.charCodeAt(j) >= 0x20 && line.charCodeAt(j) <= 0x2f) j++;
      if (j >= n) {
        // Truncated — no final byte ever arrived. Swallow the rest of the
        // line rather than leak the partial escape into `buf`.
        break;
      }
      const final = line[j];
      if (final === "m") {
        flush();
        const params = paramStr
          .split(";")
          .map((s) => (s === "" ? 0 : parseInt(s, 10)))
          .map((v) => (Number.isNaN(v) ? 0 : v));
        state = applySgr(params, state);
      }
      // Any other final byte (cursor movement, erase-line 'K', etc.) —
      // stripped, no rendering effect, common in spinner/progress output.
      i = j + 1;
      continue;
    }

    if (kind === "]") {
      // OSC: ESC ']' ... terminated by BEL (\x07) or ST (ESC '\\').
      let j = i + 2;
      while (j < n && line[j] !== "\x07" && !(line[j] === ESC && line[j + 1] === "\\")) j++;
      if (j >= n) break; // truncated — swallow the rest.
      i = line[j] === "\x07" ? j + 1 : j + 2;
      continue;
    }

    // A lone/unrecognized escape (a bare trailing ESC, or a two-byte
    // sequence like charset-select `ESC(B`) — drop just the ESC byte so it
    // never leaks into rendered text; whatever follows falls through to the
    // next loop iteration as plain text (harmless for the sequences that
    // show up in real CLI output).
    i++;
  }
  flush();
  return segments;
}

export function parseAnsiLine(text: string): Segment[] {
  const lastCr = text.lastIndexOf("\r");
  const line = lastCr === -1 ? text : text.slice(lastCr + 1);
  return tokenize(line);
}
