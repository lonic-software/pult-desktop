import { describe, expect, it } from "vitest";
import { initialValue, isOptional, restoreValue, runBlocker, runValues } from "./paramValues";
import type { Param } from "./types";

const flag: Param = { name: "force", kind: "flag", arg: "--force" };
const only: Param = { name: "only", kind: "repeat", arg: "--only", min: 0, max: 2, secret: false };
const files: Param = { name: "files", kind: "repeat", arg: null, min: 1, max: null, secret: false };
const env: Param = { name: "env", kind: "pick", options: ["dev", "prod"] };
const note: Param = { name: "note", kind: "input", default: "hi" };

describe("initialValue", () => {
  it("starts a flag off, a repeat empty, and a pick or input at its default", () => {
    expect(initialValue(flag)).toBe(false);
    expect(initialValue(only)).toEqual([]);
    expect(initialValue(env)).toBe("");
    expect(initialValue(note)).toBe("hi");
  });
});

describe("restoreValue", () => {
  it("restores a remembered value of the kind's type", () => {
    expect(restoreValue(flag, true)).toBe(true);
    expect(restoreValue(only, ["a"])).toEqual(["a"]);
    expect(restoreValue(note, "x")).toBe("x");
  });

  it("never restores a secret, even one remembered before it became secret", () => {
    expect(restoreValue({ ...only, secret: true }, ["tok"])).toEqual([]);
    expect(restoreValue({ name: "t", kind: "input", secret: true }, "tok")).toBe("");
  });

  it("drops a remembered value whose type no longer fits the kind", () => {
    expect(restoreValue(flag, "true")).toBe(false);
    expect(restoreValue(only, "a")).toEqual([]);
    expect(restoreValue(note, ["x"])).toBe("hi");
  });
});

describe("isOptional", () => {
  it("is optional for a flag, a min-0 repeat and a defaulted input only", () => {
    expect(isOptional(flag)).toBe(true);
    expect(isOptional(only)).toBe(true);
    expect(isOptional(files)).toBe(false);
    expect(isOptional(env)).toBe(false);
    expect(isOptional(note)).toBe(true);
  });
});

describe("runValues", () => {
  it("sends typed values, without a repeat's blank rows", () => {
    expect(runValues([flag, only, env], { force: true, only: ["a", "", "b c"], env: "dev" })).toEqual({
      force: true,
      only: ["a", "b c"],
      env: "dev",
    });
  });

  it("fills a param missing from the form with its initial value", () => {
    expect(runValues([flag, files], {})).toEqual({ force: false, files: [] });
  });
});

describe("runBlocker", () => {
  it("refuses a command with a kind this app doesn't know", () => {
    const later: Param = { name: "when", kind: "schedule" };
    expect(runBlocker([env, later], {})).toContain("when (schedule)");
  });

  it("checks a repeat's count against its bounds, blank rows not counted", () => {
    expect(runBlocker([files], { files: ["", " "] })).toBe("files needs at least 1 value.");
    expect(runBlocker([only, files], { only: ["a", "b", "c"], files: ["x"] })).toBe(
      "only takes at most 2 values.",
    );
    expect(runBlocker([only, files], { only: ["a"], files: ["x"] })).toBeNull();
  });

  it("needs a value for a required pick or input, not for a defaulted one", () => {
    expect(runBlocker([env, note], { env: " ", note: "" })).toBe("env needs a value.");
    expect(runBlocker([env, note], { env: "dev", note: "" })).toBeNull();
  });
});
