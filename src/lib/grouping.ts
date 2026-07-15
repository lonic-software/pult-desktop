// The grouping rule for the sidebar, implemented once here and shared by
// every view that needs it — mirrors docs/reference.md in the pult repo
// verbatim:
//
//   a command's group is its `category` if set, else the module's declared
//   `name:` for commands that came from an include, else the include it
//   came from (`origin`, the raw source string), else the implicit `local`
//   group; groups containing at least one locally-declared command come
//   first (in order of first appearance), then the remaining groups in
//   include order. Categories merge across sources — a module tagging its
//   exports `Deploy` joins the local `Deploy` group, not a separate one.

import type { CommandInfo, Listing } from "./types";

export const LOCAL_GROUP_KEY = "\0local";
export const LOCAL_GROUP_LABEL = "Local";

export interface CommandGroup {
  key: string;
  label: string;
  commands: CommandInfo[];
}

function groupKeyAndLabel(cmd: CommandInfo, listing: Listing): { key: string; label: string } {
  if (cmd.category) {
    return { key: `category:${cmd.category}`, label: cmd.category };
  }
  if (cmd.origin) {
    const include = listing.includes.find((i) => i.source === cmd.origin);
    const label = include?.name ?? cmd.origin;
    return { key: `origin:${cmd.origin}:${label}`, label };
  }
  return { key: LOCAL_GROUP_KEY, label: LOCAL_GROUP_LABEL };
}

export function groupCommands(listing: Listing): CommandGroup[] {
  const groups = new Map<
    string,
    CommandGroup & { firstLocalIndex: number | null; includeOrderIndex: number }
  >();

  listing.commands.forEach((cmd, index) => {
    const { key, label } = groupKeyAndLabel(cmd, listing);
    let group = groups.get(key);
    if (!group) {
      const includeOrderIndex = cmd.origin
        ? listing.includes.findIndex((i) => i.source === cmd.origin)
        : Number.POSITIVE_INFINITY;
      group = { key, label, commands: [], firstLocalIndex: null, includeOrderIndex };
      groups.set(key, group);
    }
    group.commands.push(cmd);
    if (cmd.origin === null && group.firstLocalIndex === null) {
      group.firstLocalIndex = index;
    }
  });

  const all = Array.from(groups.values());
  const withLocal = all
    .filter((g) => g.firstLocalIndex !== null)
    .sort((a, b) => (a.firstLocalIndex as number) - (b.firstLocalIndex as number));
  const includeOnly = all
    .filter((g) => g.firstLocalIndex === null)
    .sort((a, b) => a.includeOrderIndex - b.includeOrderIndex);

  return [...withLocal, ...includeOnly].map(({ key, label, commands }) => ({ key, label, commands }));
}
