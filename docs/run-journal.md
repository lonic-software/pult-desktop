# Run journal — protocol spec (draft 1)

_Horizon 0 of [the roadmap](./roadmap.md). This draft lives in pult-desktop
while the protocol is being designed; once implemented it belongs in the
pult repo's `docs/reference.md` alongside the events protocol, since pult
is the writer and this document is a contract about what pult writes._

## Invariant

**Every `pult` run is written to disk by the pult process itself,
regardless of who launched it or who is watching.** The desktop app, the
CLI's own `--follow`-style features, a future companion app, and a future
team control plane are all *readers* of this journal. Nothing else is ever
a source of truth about a run. A design choice that requires a second
source of truth is wrong (roadmap, closing rule).

The invariant exempts **ephemeral runs**, which have no journal-worthy
identity to begin with: `pult x` (a module source run with no manifest
behind it) and `--print` (a dry-run preview — nothing executes, so there is
no run). Everything else — every manifest command, CLI- or app-spawned,
interactive or not — is journaled.

Two consequences worth stating up front:

- The desktop app stops claiming `PULT_EVENTS` (see "Interaction with
  PULT_EVENTS" — claiming it would actually *starve* the journal) and stops
  owning pipes; it spawns detached and tails files.
- A run's lifetime is decoupled from any viewer's lifetime. Quit the app
  mid-deploy: the deploy continues, journaled; reopen and re-tail.

## Location

Journals live in a global per-user state directory, **not** inside the
repo — repos aren't always writable, and history in a worktree would die
with the worktree. _(Decided 2026-07-17: user home confirmed as the
default. An **opt-in in-repo journal** — so executions can become part of
repo history — is a future extension: an additive `journal:` manifest
setting redirecting the writer, with the reader contract unchanged. Not in
schema 1.)_

| Platform | `<state>` |
| --- | --- |
| Linux | `$XDG_STATE_HOME/pult` (default `~/.local/state/pult`) |
| macOS | `~/Library/Application Support/pult/state` |
| Windows | `%LOCALAPPDATA%\pult\state` |

`$PULT_STATE_DIR`, when set, overrides all three (tests, sandboxes).

Layout:

```
<state>/repos/<repo-key>/
  repo.json                    # { "schema": 1, "dir": "<canonical dir>" }
  runs/<run-id>/
    meta.json
    events.jsonl
```

- `<repo-key>` = first 16 hex chars of SHA-256 of the canonical
  (symlink-resolved, absolute) manifest directory — the listing's `dir`.
  `repo.json` is the human-readable reverse mapping, written on first run,
  so readers can map keys back to paths without hashing every candidate.
- Per-repo grouping keeps discovery cheap (the desktop watches one `runs/`
  directory per mounted device) and makes retention naturally per-repo.

## Run identity

- `run-id` is a UUID string by default. The invoker may instead supply one
  explicitly (`pult <cmd> --run-id <id>` — the desktop app keeps generating
  its own, as today); when absent, pult generates a UUIDv7 so directory
  listings sort chronologically by name.
- `run-id` is also a directory name, so it's validated: 1–64 ASCII
  alphanumeric/`-` characters, nothing else. An invalid explicit
  `--run-id` (a path with `/`, `..`, or an absolute prefix, say) is a
  **usage error** — pult refuses the invocation up front, before anything
  runs, rather than accepting a string that could otherwise escape the
  journal's own directory tree on either the writer or a reader side.
- Run ids are globally unique and permanent: they become subscription keys
  and URL path segments in roadmap Horizons 2–4. Readers must treat them as
  opaque.

## `meta.json`

Written once at spawn, rewritten (atomically: write `meta.json.tmp`, then
rename) on every status transition. Schema is additive-only, same
discipline as the listing schema.

```jsonc
{
  "schema": 1,
  "run_id": "0198c0de-…",
  "repo_dir": "/Users/mate/work/acme-ops",     // canonical listing `dir`
  "manifest": "/Users/mate/work/acme-ops/pult.yaml",
  "command_id": "aws:deploy",
  "command_title": "Deploy stack",             // display without re-listing
  "params": { "region": "eu-west-1", "token": "<redacted>" },
  "origin": "cli",                             // "cli" | "desktop" | future: "remote:<device-id>"
  "interactive": false,
  "pult_version": "0.4.0",
  "pid": 41230,
  "pgid": 41230,                               // absent on Windows
  "started_at": "2026-07-17T13:05:12.412Z",
  "status": "running",                         // "running" | "exited" | "stopped"
  "exit_code": null,                           // set when status = "exited"
  "ended_at": null                             // set on any terminal status
}
```

Field notes:

- **`params`** is the audit record. Any param with `secret: true` in the
  manifest is journaled as the literal string `"<redacted>"` — the value
  never touches disk. Output lines are *not* scrubbed (it can't be done
  reliably); a script that echoes a secret has the same exposure it has in
  a terminal. This asymmetry is deliberate and documented rather than
  pretended away.
- **`status`** has exactly three writer-written values. "Crashed" is not
  among them — it is a *reader-derived* state (below), because a crashed
  writer by definition can't record its own crash.
- **`origin`** is informational in schema 1; Horizon 3 makes it
  load-bearing (audit trail of remote triggers).

## `events.jsonl`

Append-only, one JSON object per line, written by the pult process as
events happen. The kinds mirror the desktop's existing `RunEvent` union —
this is "PULT_EVENTS plus stdout/stderr plus exit, as JSON":

```jsonc
{ "ts": 1784293512412, "kind": "line",     "stream": "stdout", "text": "building image…" }
{ "ts": 1784293512890, "kind": "step",     "k": 1, "n": 3, "name": "build" }
{ "ts": 1784293513020, "kind": "progress", "pct": 33, "text": null }        // pct null = indeterminate "?"
{ "ts": 1784293513400, "kind": "status",   "text": "verifying environment" }
{ "ts": 1784293519001, "kind": "exit",     "code": 0, "stopped": false }
```

- **`ts`** (unix ms, writer's clock) on every record — today the desktop
  stamps step arrival times locally (`StepEvent.at`); the journal carries
  time so history renders stage durations after the fact.
- No `run_id` per line — it's implied by the directory. No schema per line
  — `meta.json`'s `schema` covers the pair.
- `step`/`progress`/`status` payloads follow the PULT_EVENTS v1 grammar
  exactly (same clamping, same 1-based `k <= n`). Malformed event lines
  from the child are dropped by pult before journaling, per the protocol's
  existing leniency — readers never see them.
- The final record of a completed run is always exactly one `exit`.
- **Unknown `kind`s must be skipped by readers, not errored** — the same
  forward-compatibility rule as the wire protocol.

### Writer rules

- Single writer per run: the pult process. Line-buffered appends, flushed
  per line (output is human-paced; per-line `write(2)` is nothing).
  `fsync` is required only on `meta.json` status transitions, not per
  event line — after a power loss a run may lose trailing output but never
  its outcome-vs-crashed distinction. A status transition writes
  `meta.json.tmp`, fsyncs its contents, renames it into place, then
  best-effort fsyncs the containing run directory too — the rename itself
  is a directory-entry update, which needs its own fsync on most
  filesystems to survive a crash right after.
- A reader may observe a torn final line (crash mid-append). Readers must
  treat an unparseable *last* line of the file as "not yet written".
- **Interactive commands** (`interactive: true`): the terminal owns the
  tty, so pult journals `meta.json` and the `exit` event only —
  `events.jsonl` legitimately contains a single line. Readers render these
  as "ran in terminal".
- **Non-unix (Windows) targets: journaling is disabled entirely, for now.**
  Crash detection depends on a liveness probe for the writer process, and
  no cheap equivalent to unix's signal-0 exists there yet — journaling
  into a `running` state a reader could never resolve (no way to ever
  detect the writer died) would be worse than not journaling at all, so
  pult on Windows simply doesn't journal a run rather than doing that.
  Revisit once an `OpenProcess`-based probe lands. Readers on any platform
  may still encounter journals a unix pult wrote (a shared drive, a synced
  state dir) and read them normally — this only affects the writer.

### Reader rules

- Discovery: enumerate `runs/`, read `meta.json`. Live-follow: tail
  `events.jsonl` via fs notification with a polling fallback (≤250 ms).
  Readers never write inside a run directory, and never delete one that is
  `status: "running"` with a live process.
- **Crash detection:** `status: "running"` + dead process = render as
  *crashed*. The liveness probe is the writer **pid** (`meta.json`'s
  `pid`, the journaling pult process — not its child, and not the
  `pgid`, which is a separate capability used only to *stop* a run, never
  to probe it). Pid reuse across reboots is real; a reader that knows the
  boot time may treat any `running` run whose `started_at` predates boot
  as crashed without probing. Schema 1 accepts the residual within-boot
  reuse risk; if it ever bites, an additive `proc_started_at` field fixes
  it without a major version.

## Stopping a run — including one you didn't spawn

`meta.json`'s `pgid` is the stop capability. Any reader may stop a run
using the same ladder the desktop uses today: `SIGTERM` to the group, a
grace period (desktop's `STOP_GRACE`), then `SIGKILL` to the group.

For that to journal cleanly, **pult must handle `SIGTERM` gracefully**:
forward/await its child tree, append the `exit` event with
`"stopped": true`, rewrite meta to `status: "stopped"`, then exit. A
`SIGKILL`ed pult can't do any of that — the run is then simply found later
by crash detection, which is the honest description of what happened.

Windows: moot for now, since journaling itself is off there (see Writer
rules) — there is no journaled `pgid` to signal in the first place.
Stopping an app-spawned run on Windows falls back to whatever
non-journal-based mechanism the desktop's stop feature already uses there
today (`TerminateProcess` on the pid).

## Interaction with `PULT_EVENTS` (breaking-ish change, by design)

pult's documented passthrough rule says: if `PULT_EVENTS` is already set
when pult runs a command, pult creates no pipe and does no translation —
the channel passes through to whoever claimed it. **The desktop app claims
it today** (see `pult_bin.rs::run_streaming`), which means under the
current arrangement *pult never sees step/progress/status events for
app-spawned runs* — and therefore could not journal them.

Resolution: once the journal ships, the desktop stops claiming
`PULT_EVENTS` and stops reading child pipes entirely. pult owns its
channel again (as it does for CLI runs), journals everything, and the app
tails the journal — one code path for app-spawned and CLI-spawned runs
alike. The passthrough rule itself stays documented and unchanged for
other wrappers; the spec's requirement is only that *journaling happens at
whatever point pult still sees the events*, and a wrapper that claims the
channel accepts that step/progress/status won't be journaled for its runs
(lines and exit still are, since those flow through pult regardless).

**Implementation detail, not a protocol change:** when pult owns the
channel for a journaled run, it no longer assumes fd 3 is free and
`dup2`s the pipe's write end onto it — the journal's own `events.jsonl`
handle routinely occupies low fds by the time the channel is wired up, and
an invoker-passed descriptor (`pult import 3<seed.txt`) deserves to
survive untouched rather than win a coin toss against the channel. Instead
pult passes the pipe's write end through on **its own fd number** —
`PULT_EVENTS=<n>` for whatever `n` the pipe actually got, CLOEXEC cleared,
no `dup2` at all. This is legal only because the wire protocol already
requires every child to read the fd number from `$PULT_EVENTS` rather than
assuming `3`; that existing rule is what makes fd number irrelevant to a
well-behaved child. The passthrough rule is unaffected: an invoker-passed
`PULT_EVENTS` still wins outright (pult creates no pipe, does no
translation) exactly as before.

## Retention

- Writer-side pruning at run start: keep the most recent `N` runs per
  `(repo, command_id)`, default `N = 20`, overridable via
  `PULT_RUNS_KEEP`. Never prune a `running` entry whose process is alive.
- No daemon, no background sweeper — the next run cleans up after the
  previous ones, and `pult runs prune` (future CLI nicety) can do it on
  demand.

## Desktop app changes when this lands

1. Spawn runs detached (own process group already; drop the pipes and the
   `PULT_EVENTS` claim), passing `--run-id`.
2. Replace the in-memory-only `runsByRepo` hydration: on device open,
   discover that repo's journal, render history, tail live runs. The
   in-memory shape (`RunRecord`) stays; it just gains a loader.
3. Stop button signals the journaled `pgid` — works identically for runs
   the app never spawned.
4. Quit-time behavior becomes "do nothing": runs continue, journal
   persists.

## Open questions (decide before implementing)

1. ~~**Does `pult --list` grow a `runs` surface?**~~ _Decided 2026-07-17:
   yes — `pult runs list --json`, `pult runs tail <run-id> --json
   [--follow]`, `pult runs prune`. Sugar over the same files; the file
   layout remains contract too, but thin clients should prefer the CLI._
2. **Journal `line` granularity vs. raw byte stream** — schema 1 journals
   line-split output (matching today's UI). Full-fidelity terminal capture
   (colors, partial lines, `\r` progress bars) would need a raw log
   alongside; deferred until something needs it.
3. **Env fingerprint in meta** (git sha, dirty flag, hostname) — cheap now,
   valuable for Horizon 4 audit; needs a privacy pass before adding.
4. **Retention by size** as well as count (a single verbose run can be
   huge). Probably `PULT_RUNS_MAX_MB` per repo, prune oldest-first.
