// The grouping rule for the board, implemented once here and shared by
// every view that needs it — mirrors docs/reference.md in the pult repo
// verbatim for a single level of grouping:
//
//   a command's group is its `category` if set, else the module's declared
//   `name:` for commands that came from an include, else the include it
//   came from (`origin`, the raw source string), else the implicit `local`
//   group; groups containing at least one locally-declared command come
//   first (in order of first appearance), then the remaining groups in
//   include order. Categories merge across sources — a module tagging its
//   exports `Deploy` joins the local `Deploy` group, not a separate one.
//
// On top of that, the board applies a *least-nesting* rule to decide
// whether to show that single flat level, or a second level nested inside
// it (one outer panel per source, category sub-groups within each). Let
// S = the number of distinct sources in the listing (local commands are one
// source; each include is its own source) and C = whether any command in
// the listing carries a category:
//
//   - S <= 1 (only local, or only one source at all): flat, grouped by
//     category exactly as documented above — nesting a single source under
//     itself would add a level with nothing to distinguish.
//   - S > 1 and C is false: flat, grouped by source (no command has a
//     category to group by, so the single-level rule above already reduces
//     to "group by origin").
//   - S > 1 and C is true: nested — outer groups by source, inner
//     sub-groups by category within that source. A command with no category
//     falls into a "General" sub-group (reusing the board's existing
//     fallback-label instinct — `LOCAL_GROUP_LABEL` for "no grouping key at
//     all" at the top level, `GENERAL_SUBGROUP_LABEL` for "no category
//     within a known source" one level down). Sub-groups are ordered by
//     first appearance within their source, same convention as the flat
//     rule — *not* forced to put "General" first.
//
//     Categories do NOT merge across sources in nested mode: a local
//     `Deploy` sub-group and an included module's `Deploy` sub-group are
//     two separate sub-groups (one under each source's panel), even though
//     the flat rule above would merge them into one `Deploy` group. Nesting
//     exists precisely to keep same-named categories from different sources
//     visually distinct once there's more than one source to distinguish.
//
// This keeps the flat (single-level) board — still the common case for a
// small manifest — pixel-identical to before nesting existed; nesting only
// engages once there's something to nest.

import type { CommandInfo, Listing } from "./types";

export const LOCAL_GROUP_KEY = "\0local";
export const LOCAL_GROUP_LABEL = "Local";
export const GENERAL_SUBGROUP_LABEL = "General";

/** A category sub-group inside one source's outer group — only present when
 *  `GroupedListing.nested` is true. */
export interface CommandSubgroup {
  key: string;
  label: string;
  commands: CommandInfo[];
}

export interface CommandGroup {
  key: string;
  label: string;
  /** Flat command list for this group. In nested mode this is the
   *  concatenation of `subgroups`' commands in sub-group order (kept in
   *  sync so board-wide flat indexing — e.g. the power-on stagger — doesn't
   *  need to know whether nesting is active). */
  commands: CommandInfo[];
  /** Category sub-groups within this source, in first-appearance order.
   *  Present only when the listing as a whole was nested (see
   *  `GroupedListing.nested`) — never a mix within one result. */
  subgroups?: CommandSubgroup[];
}

export interface GroupedListing {
  /** Whether the least-nesting rule above engaged for this listing. Decided
   *  once for the whole listing (S/C are properties of the listing, not of
   *  any one group), so callers can branch render logic once instead of
   *  per group, and so re-deciding never happens mid-search (see
   *  `groupCommands`'s doc comment on why callers should call this on the
   *  unfiltered listing). */
  nested: boolean;
  groups: CommandGroup[];
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

function sourceKeyAndLabel(cmd: CommandInfo, listing: Listing): { key: string; label: string } {
  if (cmd.origin) {
    const include = listing.includes.find((i) => i.source === cmd.origin);
    const label = include?.name ?? cmd.origin;
    return { key: `origin:${cmd.origin}:${label}`, label };
  }
  return { key: LOCAL_GROUP_KEY, label: LOCAL_GROUP_LABEL };
}

/** Flat single-level grouping — pult's documented rule verbatim (category,
 *  else module name, else origin, else local; local-containing groups
 *  first, then include order). Used both when the least-nesting rule keeps
 *  the board flat, and internally as "group by key" for the nested case's
 *  outer (by source) and inner (by category) levels. */
function buildGroups(
  listing: Listing,
  keyAndLabel: (cmd: CommandInfo, listing: Listing) => { key: string; label: string },
): CommandGroup[] {
  const groups = new Map<
    string,
    CommandGroup & { firstLocalIndex: number | null; includeOrderIndex: number }
  >();

  listing.commands.forEach((cmd, index) => {
    const { key, label } = keyAndLabel(cmd, listing);
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

/** Category sub-groups within a single source's commands, in first-appearance
 *  order (a `Map`'s iteration order is insertion order, so no extra sort is
 *  needed the way the source/local split above needs one). Uncategorized
 *  commands fall into "General". */
function buildSubgroups(commands: CommandInfo[]): CommandSubgroup[] {
  const subgroups = new Map<string, CommandSubgroup>();
  for (const cmd of commands) {
    const key = cmd.category ? `category:${cmd.category}` : "\0general";
    const label = cmd.category ?? GENERAL_SUBGROUP_LABEL;
    let subgroup = subgroups.get(key);
    if (!subgroup) {
      subgroup = { key, label, commands: [] };
      subgroups.set(key, subgroup);
    }
    subgroup.commands.push(cmd);
  }
  return Array.from(subgroups.values());
}

export function groupCommands(listing: Listing): GroupedListing {
  const sourceCount = new Set(
    listing.commands.map((cmd) => (cmd.origin === null ? LOCAL_GROUP_KEY : cmd.origin)),
  ).size;
  const hasCategory = listing.commands.some((cmd) => cmd.category !== null);
  const nested = sourceCount > 1 && hasCategory;

  if (!nested) {
    return { nested: false, groups: buildGroups(listing, groupKeyAndLabel) };
  }

  const outer = buildGroups(listing, sourceKeyAndLabel);
  const groups = outer.map((group) => {
    const subgroups = buildSubgroups(group.commands);
    // Re-flatten from the sub-groups (rather than keeping `group.commands`'
    // original interleaved order) so a flat index built from `commands` —
    // e.g. Board.svelte's power-on stagger — walks commands in the same
    // order they're actually rendered in (sub-group by sub-group).
    const commands = subgroups.flatMap((sg) => sg.commands);
    return { ...group, commands, subgroups };
  });
  return { nested: true, groups };
}
