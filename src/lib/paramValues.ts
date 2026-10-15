// How each param kind's value lives in the run form: what it starts as,
// what a remembered value may restore, whether it is optional, and what
// reaches `run_command`. pult's schema 2 types a value by its param's kind —
// text for a pick or an input, a boolean for a flag, a list for a repeat —
// and refuses a value of the wrong type, so nothing here coerces one kind's
// value into another's.

import type { Param, ParamValue } from "./types";

/** The kinds this app can render and send. Any other kind makes its
 *  command unrunnable here rather than guessed at. */
const KNOWN_KINDS = new Set(["pick", "input", "flag", "repeat"]);

/** What a field starts as before anything is remembered: an unset flag, an
 *  empty repeat, a pick or input's declared default. */
export function initialValue(param: Param): ParamValue {
  switch (param.kind) {
    case "flag":
      return false;
    case "repeat":
      return [];
    default:
      return param.default ?? "";
  }
}

/** A remembered value, if it still has the type this param's kind takes
 *  (the manifest may have changed the kind since); else the initial value.
 *  A secret is never restored, even one remembered before it became secret. */
export function restoreValue(param: Param, remembered: ParamValue | undefined): ParamValue {
  if (param.secret) return initialValue(param);
  const fits =
    param.kind === "flag"
      ? typeof remembered === "boolean"
      : param.kind === "repeat"
        ? Array.isArray(remembered) && remembered.every((item) => typeof item === "string")
        : typeof remembered === "string";
  return fits ? (remembered as ParamValue) : initialValue(param);
}

/** Whether a run can go without the operator touching this field: a flag
 *  is simply off, a repeat may be empty when its `min` is 0, and a pick or
 *  input is optional when it declares a default. */
export function isOptional(param: Param): boolean {
  switch (param.kind) {
    case "flag":
      return true;
    case "repeat":
      return (param.min ?? 0) === 0;
    default:
      return param.default != null;
  }
}

/** A value as text, for the places only a pick or input can be read
 *  (`depends_on` and `pick.source` reference nothing else). */
export function textOf(value: ParamValue | undefined): string {
  return typeof value === "string" ? value : "";
}

/** A repeat's items as they are sent: blank rows are rows the operator
 *  added and never filled, not items — the same blank a pick or input's
 *  required check uses. */
function items(value: ParamValue | undefined): string[] {
  return Array.isArray(value) ? value.filter((item) => item.trim() !== "") : [];
}

/** The values `run_command` sends, typed by kind. */
export function runValues(params: Param[], values: Record<string, ParamValue>): Record<string, ParamValue> {
  const out: Record<string, ParamValue> = {};
  for (const param of params) {
    const value = values[param.name] ?? initialValue(param);
    out[param.name] = param.kind === "repeat" ? items(value) : value;
  }
  return out;
}

/** Why this command can't run from here as filled in, or null. */
export function runBlocker(params: Param[], values: Record<string, ParamValue>): string | null {
  const unknown = params.filter((p) => !KNOWN_KINDS.has(p.kind));
  if (unknown.length > 0) {
    const names = unknown.map((p) => `${p.name} (${p.kind})`).join(", ");
    return `This pult-desktop can't fill ${names} — update it, or run the command in a terminal.`;
  }
  for (const param of params) {
    if ((param.kind === "pick" || param.kind === "input") && !isOptional(param)) {
      if (!textOf(values[param.name]).trim()) return `${param.name} needs a value.`;
      continue;
    }
    if (param.kind !== "repeat") continue;
    const count = items(values[param.name]).length;
    const min = param.min ?? 0;
    if (count < min) return `${param.name} needs at least ${min} value${min === 1 ? "" : "s"}.`;
    if (param.max != null && count > param.max) {
      return `${param.name} takes at most ${param.max} value${param.max === 1 ? "" : "s"}.`;
    }
  }
  return null;
}
