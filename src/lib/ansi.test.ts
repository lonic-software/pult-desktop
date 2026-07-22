import { describe, expect, it } from "vitest";
import { parseAnsiLine } from "./ansi";

// Every assertion here also stands in for the module's core safety
// invariant: no segment's `text` ever contains an escape byte (ESC or BEL).
// `assertClean` below enforces that on every parsed line so a regression in
// any one test case would fail loudly rather than just losing color.
function assertClean(text: string) {
  expect(text).not.toMatch(/[\x1b\x07]/);
}

describe("parseAnsiLine", () => {
  it("returns plain text untouched when there are no escapes", () => {
    const segs = parseAnsiLine("all suites passed");
    expect(segs).toEqual([{ text: "all suites passed" }]);
  });

  it("parses a basic fg color (30-37)", () => {
    const segs = parseAnsiLine("\x1b[32mgreen\x1b[0m plain");
    assertClean(segs.map((s) => s.text).join(""));
    expect(segs).toEqual([
      { text: "green", fg: "ansi-2" },
      { text: " plain" },
    ]);
  });

  it("parses a bright fg color (90-97)", () => {
    const segs = parseAnsiLine("\x1b[91mbright red\x1b[0m");
    expect(segs).toEqual([{ text: "bright red", fg: "ansi-9" }]);
  });

  it("parses bg colors (40-47, 100-107)", () => {
    const segs = parseAnsiLine("\x1b[44mblue bg\x1b[0m\x1b[103mbright yellow bg\x1b[0m");
    expect(segs).toEqual([
      { text: "blue bg", bg: "ansi-4" },
      { text: "bright yellow bg", bg: "ansi-11" },
    ]);
  });

  it("parses bold, dim, italic, underline", () => {
    const segs = parseAnsiLine("\x1b[1mbold\x1b[0m\x1b[2mdim\x1b[0m\x1b[3mitalic\x1b[0m\x1b[4munderline\x1b[0m");
    expect(segs).toEqual([
      { text: "bold", bold: true },
      { text: "dim", dim: true },
      { text: "italic", italic: true },
      { text: "underline", underline: true },
    ]);
  });

  it("nests multiple SGR attributes in one code (e.g. bold + fg)", () => {
    const segs = parseAnsiLine("\x1b[1;31mbold red\x1b[0m");
    expect(segs).toEqual([{ text: "bold red", bold: true, fg: "ansi-1" }]);
  });

  it("accumulates SGR codes across separate escapes until reset", () => {
    const segs = parseAnsiLine("\x1b[1m\x1b[32mbold green\x1b[0mplain");
    expect(segs).toEqual([
      { text: "bold green", bold: true, fg: "ansi-2" },
      { text: "plain" },
    ]);
  });

  it("22/23/24 turn off bold+dim / italic / underline individually", () => {
    const segs = parseAnsiLine("\x1b[1;3;4mall\x1b[22mno-bold-no-dim\x1b[23mno-italic\x1b[24mno-underline");
    expect(segs).toEqual([
      { text: "all", bold: true, italic: true, underline: true },
      { text: "no-bold-no-dim", italic: true, underline: true },
      { text: "no-italic", underline: true },
      { text: "no-underline" },
    ]);
  });

  it("39/49 reset fg/bg to default independently of other attributes", () => {
    const segs = parseAnsiLine("\x1b[1;31;44mstyled\x1b[39mno-fg\x1b[49mno-bg");
    expect(segs).toEqual([
      { text: "styled", bold: true, fg: "ansi-1", bg: "ansi-4" },
      { text: "no-fg", bold: true, bg: "ansi-4" },
      { text: "no-bg", bold: true },
    ]);
  });

  it("0 resets everything, including fg/bg/bold", () => {
    const segs = parseAnsiLine("\x1b[1;31;44mstyled\x1b[0mplain");
    expect(segs).toEqual([
      { text: "styled", bold: true, fg: "ansi-1", bg: "ansi-4" },
      { text: "plain" },
    ]);
  });

  it("7 (reverse) swaps fg/bg", () => {
    const segs = parseAnsiLine("\x1b[31;44;7mreversed\x1b[0m");
    expect(segs).toEqual([{ text: "reversed", fg: "ansi-4", bg: "ansi-1" }]);
  });

  it("7 then 27 round-trips back to the original colors", () => {
    const segs = parseAnsiLine("\x1b[31;44;7;27mback to normal\x1b[0m");
    expect(segs).toEqual([{ text: "back to normal", fg: "ansi-1", bg: "ansi-4" }]);
  });

  describe("256-color (38;5;n / 48;5;n)", () => {
    it("maps n < 16 to the same basic palette tokens", () => {
      const segs = parseAnsiLine("\x1b[38;5;9mtext\x1b[0m");
      expect(segs).toEqual([{ text: "text", fg: "ansi-9" }]);
    });

    it("computes the 6x6x6 cube for 16-231", () => {
      // 208 ("orange" in most 256-color charts): i = 208-16 = 192 ->
      // r = step[floor(192/36)%6] = step[5] = 255
      // g = step[floor(192/6)%6]  = step[2] = 135
      // b = step[192%6]           = step[0] = 0
      const segs = parseAnsiLine("\x1b[38;5;208mtext\x1b[0m");
      expect(segs).toEqual([{ text: "text", fg: "#ff8700" }]);
    });

    it("computes the cube corner values exactly (16 = black corner, 231 = white corner)", () => {
      const segs = parseAnsiLine("\x1b[38;5;16mblack\x1b[0m\x1b[38;5;231mwhite\x1b[0m");
      expect(segs).toEqual([
        { text: "black", fg: "#000000" },
        { text: "white", fg: "#ffffff" },
      ]);
    });

    it("computes the grayscale ramp for 232-255", () => {
      // 232 -> level 8, 255 -> level 8 + 23*10 = 238
      const segs = parseAnsiLine("\x1b[48;5;232mdark\x1b[0m\x1b[48;5;255mlight\x1b[0m");
      expect(segs).toEqual([
        { text: "dark", bg: "#080808" },
        { text: "light", bg: "#eeeeee" },
      ]);
    });
  });

  describe("truecolor (38;2;r;g;b / 48;2;r;g;b)", () => {
    it("renders an exact hex from r;g;b", () => {
      const segs = parseAnsiLine("\x1b[38;2;18;52;86mtext\x1b[0m");
      expect(segs).toEqual([{ text: "text", fg: "#123456" }]);
    });

    it("applies to bg independently of fg", () => {
      const segs = parseAnsiLine("\x1b[38;2;255;0;0;48;2;0;255;0mtext\x1b[0m");
      expect(segs).toEqual([{ text: "text", fg: "#ff0000", bg: "#00ff00" }]);
    });
  });

  it("strips other CSI sequences (cursor movement, erase-line) without rendering them", () => {
    const segs = parseAnsiLine("\x1b[2Kprogress: \x1b[1A\x1b[32m50%\x1b[0m\x1b[K");
    for (const s of segs) assertClean(s.text);
    expect(segs).toEqual([{ text: "progress: " }, { text: "50%", fg: "ansi-2" }]);
  });

  it("strips OSC sequences terminated by BEL", () => {
    const segs = parseAnsiLine("\x1b]0;window title\x07visible text");
    expect(segs).toEqual([{ text: "visible text" }]);
  });

  it("strips OSC sequences terminated by ST (ESC \\)", () => {
    const segs = parseAnsiLine("\x1b]8;;https://example.com\x1b\\link text\x1b]8;;\x1b\\");
    expect(segs).toEqual([{ text: "link text" }]);
  });

  it("does not throw and does not leak bytes on a truncated CSI sequence", () => {
    const segs = parseAnsiLine("before\x1b[32");
    for (const s of segs) assertClean(s.text);
    expect(segs).toEqual([{ text: "before" }]);
  });

  it("does not throw and does not leak bytes on a truncated OSC sequence", () => {
    const segs = parseAnsiLine("before\x1b]0;unterminated title");
    for (const s of segs) assertClean(s.text);
    expect(segs).toEqual([{ text: "before" }]);
  });

  it("does not throw and does not leak bytes on a bare trailing escape", () => {
    const segs = parseAnsiLine("text\x1b");
    for (const s of segs) assertClean(s.text);
    expect(segs).toEqual([{ text: "text" }]);
  });

  it("does not throw on malformed 38/48 sequences (missing mode)", () => {
    const segs = parseAnsiLine("\x1b[38mtext\x1b[0m");
    for (const s of segs) assertClean(s.text);
    expect(segs).toEqual([{ text: "text" }]);
  });

  it("does not throw on malformed 38;5 with a missing index", () => {
    const segs = parseAnsiLine("\x1b[38;5mtext\x1b[0m");
    for (const s of segs) assertClean(s.text);
    expect(segs).toEqual([{ text: "text" }]);
  });

  describe("\\r overwrite semantics", () => {
    it("keeps only the content after the last \\r", () => {
      const segs = parseAnsiLine("downloading 10%\rdownloading 55%\rdownloading 100%");
      expect(segs).toEqual([{ text: "downloading 100%" }]);
    });

    it("does not leak styles from an overwritten frame into the final one", () => {
      const segs = parseAnsiLine("\x1b[31merror-ish frame\r\x1b[32mfinal ok frame\x1b[0m");
      expect(segs).toEqual([{ text: "final ok frame", fg: "ansi-2" }]);
    });

    it("treats a line with no \\r as the whole line", () => {
      const segs = parseAnsiLine("no carriage return here");
      expect(segs).toEqual([{ text: "no carriage return here" }]);
    });

    it("handles a trailing \\r with nothing after it as an empty result", () => {
      const segs = parseAnsiLine("some progress\r");
      expect(segs).toEqual([]);
    });
  });

  it("returns an empty array for an empty line", () => {
    expect(parseAnsiLine("")).toEqual([]);
  });
});
