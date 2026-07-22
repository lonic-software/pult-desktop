<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import {
    doctorCheck,
    getPultPath,
    isMock,
    listRuns,
    loadRack,
    openRepo,
    pickFolder,
    pultVersion,
    runCommand,
    saveRack,
    setPultPath,
    stopRun,
    subscribeRunOutput,
    tailRun,
    trustRepo,
  } from "$lib/api";
  import type {
    CommandInfo,
    DoctorReport,
    Listing,
    OutputLine,
    RackDevice,
    RunEvent,
    RunRecord,
    RunSummary,
  } from "$lib/types";
  import { breadcrumbFor, groupCommands, type CommandGroup, type GroupedListing } from "$lib/grouping";
  import { formatDuration } from "$lib/time";
  import type { BoardMeterOverride } from "$lib/readiness";
  import { SUCCESS_BLINK_COUNT, STOPPED_BLINK_COUNT, BLINK_PERIOD_MS } from "$lib/meterLiveness";
  import Toolbar from "$lib/components/Toolbar.svelte";
  import Rack from "$lib/components/Rack.svelte";
  import Board from "$lib/components/Board.svelte";
  import RunView from "$lib/components/RunView.svelte";
  import TrustModal from "$lib/components/TrustModal.svelte";
  import SettingsModal from "$lib/components/SettingsModal.svelte";
  import EmptyState from "$lib/components/EmptyState.svelte";

  let repoPath: string | null = $state(null);
  let listing: Listing | null = $state.raw<Listing | null>(null);
  let listingError: string | null = $state(null);
  let doctorReport: DoctorReport | null = $state(null);
  let selectedId: string | null = $state(null);
  let view: "board" | "run" = $state("board");
  let showTrustModal = $state(false);
  let trustBusy = $state(false);
  let showSettings = $state(false);
  let pultPathSetting: string | null = $state(null);
  let versionInfo: string | null = $state(null);
  let search = $state("");
  let theme: "system" | "light" | "dark" = $state("system");

  // The rack (design 4a): the persisted list of mounted devices (repos) and
  // which one is active. `devices` mirrors the rack.json store — every
  // mutation goes through `persistRack` so the list survives restarts and
  // the last-active device re-opens on launch (see onMount).
  let devices: RackDevice[] = $state([]);
  let rackCollapsed = $state(false);

  // Paths where the user clicked "Not now" on the trust modal this session —
  // suppresses re-prompting on every switch back to that device. Read only
  // inside `loadRepo`, so a plain Set (no reactivity needed).
  const readOnlyPaths = new Set<string>();

  // Journal-reader hydration (docs/run-journal.md "Desktop app changes when
  // this lands", item 2): every run this app currently has a live tail on —
  // whether started by `handleRun` itself, hydrated on device open, lazily
  // tailed when the user opens a finished command's details, or picked up by
  // the CLI-visibility poll below — routed to its record by run_id. One
  // `pult://run-output` subscription for the whole app (see the `onMount`
  // below) rather than one `listen()` per tailed run: a repo can have
  // several runs in flight (hydration can eagerly tail more than one running
  // command) and background devices keep their own tails alive while
  // inactive, so "one listener per tail" would mean stacking up an unbounded
  // number of them over a session. `runCommand`'s own dedicated per-call
  // listener (see real/backend.ts) still exists separately for the run it
  // starts — this map is never populated for those run ids, so the two
  // never double-handle the same event.
  const activeTails = new Map<string, (event: RunEvent) => void>();

  // Per-device "is a run of mine now visible in the journal that I've never
  // seen before" polling — see `pollRuns` below. Only the currently active
  // device is ever polled (matches the spec's "while a device is active");
  // switching/ejecting/unmounting always tears the previous one down first.
  const CLI_POLL_MS = 3000;
  let activePollTimer: ReturnType<typeof setInterval> | undefined;
  let activePollPath: string | null = null;

  // Per-device session state, keyed by repo path — NOT reset on device
  // switch, so a run kicked off in one device keeps streaming while another
  // is active, and switching back finds its output (plus the rack's amber
  // "running…" lamp meanwhile). Everything below the maps derives the
  // active device's slice, which is what the board/run view render.
  let runsByRepo: Record<string, Record<string, RunRecord>> = $state({});
  let overridesByRepo: Record<string, Record<string, BoardMeterOverride | null>> = $state({});
  let paramValuesByRepo: Record<string, Record<string, Record<string, string>>> = $state({});

  const runs: Record<string, RunRecord> = $derived(
    repoPath ? (runsByRepo[repoPath] ?? {}) : {},
  );
  const boardOverrides: Record<string, BoardMeterOverride | null> = $derived(
    repoPath ? (overridesByRepo[repoPath] ?? {}) : {},
  );
  // In-session param form values only — RunView hydrates them from per-repo
  // disk persistence (loadParamValues) on open and writes non-secret edits
  // back (saveParamValue), which is what the parameters module's
  // "remembered per repo" footer note promises — see RunView.
  const paramValues: Record<string, Record<string, string>> = $derived(
    repoPath ? (paramValuesByRepo[repoPath] ?? {}) : {},
  );

  // Devices with at least one in-flight run — drives the rack's amber lamp.
  const runningPaths: ReadonlySet<string> = $derived(
    new Set(
      Object.entries(runsByRepo)
        .filter(([, byCommand]) => Object.values(byCommand).some((r) => r.running))
        .map(([path]) => path),
    ),
  );
  // The board's post-run transient/latch overlay per command id (inside the
  // per-device `overridesByRepo` map above) — see readiness.ts's
  // `BoardMeterOverride` doc comment for the pure state math this drives;
  // this is the timer/acknowledgment bookkeeping around it, kept alongside
  // `runsByRepo` (same session-scoped, per-device lifetime — kept across
  // device switches, dropped on eject, never persisted). `success`/`stopped`
  // self-clear via `boardOverrideTimers` below (keyed `<path>\0<commandId>`,
  // since a blink can finish while its device sits inactive in the rack),
  // once each has finished its own blink sequence (docs/design-language.md's
  // "Blink is a mode" — see Meter.svelte's `isBlinking`); `run-failed` only
  // clears on acknowledgment (see the `$effect` near the bottom of this
  // block). The two durations below are each an iteration-count ×
  // meterLiveness.ts's BLINK_PERIOD_MS, so they exactly cover Meter.svelte's
  // `.well.glow-success`/`.well.glow-stopped` animation-iteration-count — a
  // mismatch here would either clip the last blink or leave a stretch of
  // steady color sitting after the CSS animation already finished.
  const boardOverrideTimers = new Map<string, ReturnType<typeof setTimeout>>();
  const SUCCESS_PULSE_MS = SUCCESS_BLINK_COUNT * BLINK_PERIOD_MS;
  const STOP_FLASH_MS = STOPPED_BLINK_COUNT * BLINK_PERIOD_MS;

  function timerKey(path: string, commandId: string): string {
    return `${path}\0${commandId}`;
  }

  function setBoardOverride(path: string, commandId: string, override: BoardMeterOverride | null) {
    const forRepo = overridesByRepo[path] ?? {};
    overridesByRepo = { ...overridesByRepo, [path]: { ...forRepo, [commandId]: override } };
  }

  function scheduleOverrideClear(
    path: string,
    commandId: string,
    kind: BoardMeterOverride["kind"],
    ms: number,
  ) {
    const key = timerKey(path, commandId);
    const existing = boardOverrideTimers.get(key);
    if (existing) clearTimeout(existing);
    const handle = setTimeout(() => {
      boardOverrideTimers.delete(key);
      // Only clear if it's still the same transient overlay — a newer run
      // (or a newer overlay from this same run finishing again, though that
      // can't actually happen) must never be clobbered by a stale timer.
      if (overridesByRepo[path]?.[commandId]?.kind === kind) {
        setBoardOverride(path, commandId, null);
      }
    }, ms);
    boardOverrideTimers.set(key, handle);
  }

  // Applies docs/design-language.md's "Only failures latch" rule once a run
  // ends: success blinks green a few times then self-clears, a user stop
  // blinks amber briefly then self-clears, anything else (a real failure, or
  // an invoke-level error before any exit event) latches red — blinking
  // until acknowledged (see the `$effect` below for the "being on/opening
  // the details page" trigger; "starting a new run" is handled directly in
  // `handleRun`).
  function applyBoardOutcome(
    path: string,
    commandId: string,
    outcome: {
      stopped: boolean;
      exitCode: number | null;
      errorText: string | null;
      /** pult's writer died without ever journaling an exit (see
       *  `RunEvent`'s `exit.crashed` doc comment) — its own red-family
       *  latch, same as any other failure (crashed and stopped are mutually
       *  exclusive by construction: a crash is only ever derived from a run
       *  the journal still says is "running"). */
      crashed?: boolean;
    },
  ) {
    if (outcome.stopped) {
      setBoardOverride(path, commandId, { kind: "stopped" });
      scheduleOverrideClear(path, commandId, "stopped", STOP_FLASH_MS);
    } else if (outcome.crashed) {
      setBoardOverride(path, commandId, { kind: "run-failed" });
    } else if (outcome.errorText === null && outcome.exitCode === 0) {
      setBoardOverride(path, commandId, { kind: "success" });
      scheduleOverrideClear(path, commandId, "success", SUCCESS_PULSE_MS);
    } else {
      setBoardOverride(path, commandId, { kind: "run-failed" });
    }
  }

  // Acknowledgment trigger 1 and 2 of 3 (see docs/design-language.md):
  // opening this command's details page, or already being on it the moment
  // it fails. Both are exactly "the selected command's override is
  // run-failed while the run view is showing it" — re-running this effect
  // on either `selectedId`/`view` changing (navigation) or `boardOverrides`
  // changing (a live failure while already on the page) covers both without
  // separate code paths. Trigger 3 (starting a new run) is handled directly
  // in `handleRun`, since that's a state change this effect doesn't
  // otherwise observe as an "acknowledgment". Only ever acts on the active
  // device — an inactive device's run view isn't showing, so its failures
  // stay latched until visited, exactly as designed.
  $effect(() => {
    if (
      repoPath &&
      view === "run" &&
      selectedId &&
      boardOverrides[selectedId]?.kind === "run-failed"
    ) {
      setBoardOverride(repoPath, selectedId, null);
    }
  });

  // `?tooltip=<command-id>` mock-screenshot hook — see Board.svelte's
  // forceTooltipId and CommandCard.svelte's forceTooltip.
  let forceTooltipId: string | null = $state(null);

  // groupCommands runs on the raw (unfiltered) listing so the least-nesting
  // decision (flat vs. nested — see grouping.ts) is made once per listing,
  // not recomputed per keystroke; a search that happens to leave matches in
  // only one source must not flip the board out of nested mode mid-typing.
  // Filtering below only ever removes commands/sub-groups/groups from that
  // fixed shape.
  const grouped: GroupedListing = $derived.by(() => {
    if (!listing) return { nested: false, groups: [] };
    const all = groupCommands(listing);
    const q = search.trim().toLowerCase();
    if (!q) return all;
    const matches = (c: CommandInfo) =>
      c.title.toLowerCase().includes(q) || c.id.toLowerCase().includes(q);
    const groups = all.groups
      .map((g: CommandGroup) => {
        if (!g.subgroups) {
          return { ...g, commands: g.commands.filter(matches) };
        }
        const subgroups = g.subgroups
          .map((sg) => ({ ...sg, commands: sg.commands.filter(matches) }))
          .filter((sg) => sg.commands.length > 0);
        return { ...g, commands: subgroups.flatMap((sg) => sg.commands), subgroups };
      })
      .filter((g: CommandGroup) => g.commands.length > 0);
    return { nested: all.nested, groups };
  });

  const selectedCommand = $derived(
    listing?.commands.find((c) => c.id === selectedId) ?? null,
  );

  const selectedRun = $derived(selectedId ? (runs[selectedId] ?? null) : null);

  // "repo / Source / Category" breadcrumb for the toolbar (design 3a) — only
  // meaningful once a command is selected; the board itself just shows the
  // repo name.
  const selectedBreadcrumb = $derived(
    selectedCommand && listing ? breadcrumbFor(selectedCommand, listing) : null,
  );

  $effect(() => {
    applyTheme(theme);
  });

  function applyTheme(t: "system" | "light" | "dark") {
    const root = document.documentElement;
    if (t === "system") {
      root.removeAttribute("data-theme");
    } else {
      root.setAttribute("data-theme", t);
    }
  }

  function cycleTheme() {
    theme = theme === "system" ? "light" : theme === "light" ? "dark" : "system";
    localStorage.setItem("pult-desktop:theme", theme);
  }

  async function refreshSettingsInfo() {
    pultPathSetting = await getPultPath();
    try {
      versionInfo = await pultVersion();
    } catch (e) {
      versionInfo = String(e);
    }
  }

  function basename(path: string): string {
    const parts = path.split(/[\\/]/).filter(Boolean);
    return parts[parts.length - 1] ?? path;
  }

  function persistRack() {
    // snapshot() strips the $state proxy before the object crosses into the
    // store plugin's serializer.
    void saveRack({ devices: $state.snapshot(devices), activePath: repoPath });
  }

  function upsertDevice(path: string, patch: Partial<RackDevice>) {
    if (devices.some((d) => d.path === path)) {
      devices = devices.map((d) => (d.path === path ? { ...d, ...patch } : d));
    } else {
      devices = [
        ...devices,
        { path, name: basename(path), instruments: null, status: null, ...patch },
      ];
    }
  }

  // Builds a fresh `RunRecord` straight from the journal's own summary of a
  // run this session has never seen live — hydration on device open, and the
  // CLI-visibility poll's "never seen this run_id before" branch, both go
  // through this. `lines`/`step`/`stepHistory`/`progress`/`status` all start
  // empty: none of that is in a `RunSummary`, only ever arrives by actually
  // tailing (`startTail` below) — running runs are tailed immediately by
  // both callers; a finished run's own output is filled in lazily, the
  // first time the user opens its details page (see `maybeLazyTail`).
  function recordFromSummary(summary: RunSummary): RunRecord {
    const startedAt = Date.parse(summary.started_at);
    const endedAt = summary.ended_at ? Date.parse(summary.ended_at) : null;
    return {
      runId: summary.run_id,
      running: summary.status === "running",
      lines: [],
      step: null,
      stepHistory: [],
      progress: null,
      status: null,
      stopped: summary.status === "stopped",
      crashed: summary.status === "crashed",
      exitCode: summary.exit_code,
      startedAt: Number.isFinite(startedAt) ? startedAt : Date.now(),
      endedAt: endedAt !== null && Number.isFinite(endedAt) ? endedAt : null,
    };
  }

  // A finished run whose journal summary already says it failed (crashed, or
  // a nonzero exit) latches the board's red overlay immediately on sight —
  // same "only failures latch" rule `applyBoardOutcome` enforces for a run
  // that just finished live (see its doc comment), just entered from disk
  // instead of from a live `finish()`. Success/stopped are deliberately NOT
  // resurrected here: those are transient "this just happened" blinks (see
  // `scheduleOverrideClear`), and a run from history didn't just happen —
  // showing one anyway would misread as brand new.
  function latchFailureFromSummary(path: string, commandId: string, summary: RunSummary) {
    if (summary.status === "crashed" || (summary.status === "exited" && summary.exit_code !== 0)) {
      setBoardOverride(path, commandId, { kind: "run-failed" });
    }
  }

  // Builds the same patch-run/finish/apply-event closures `handleRun` uses
  // for a run it starts itself, parameterized so hydration/the poll/a lazy
  // tail can drive an *existing* run_id (not one this call minted) through
  // the exact same event-to-record mapping — see `applyEvent`'s switch,
  // which is the one and only place a `RunEvent` becomes a `RunRecord`
  // patch, whichever of the app's several run-watching paths produced it.
  function makeRunHandlers(path: string, commandId: string, runId: string) {
    // Applies `fn` to this run's record iff it's still the record this
    // invocation is watching (same runId) — events for a run that's been
    // superseded (a newer run of the same command started) or whose device
    // was ejected land nowhere.
    function patchRun(fn: (current: RunRecord) => RunRecord): boolean {
      const forRepo = runsByRepo[path] ?? {};
      const current = forRepo[commandId];
      if (!current || current.runId !== runId) return false;
      runsByRepo = { ...runsByRepo, [path]: { ...forRepo, [commandId]: fn(current) } };
      return true;
    }

    function appendLine(line: OutputLine) {
      patchRun((current) => ({ ...current, lines: [...current.lines, line] }));
    }

    function updateRecord(
      patch: Partial<Pick<RunRecord, "step" | "stepHistory" | "progress" | "status">>,
    ) {
      patchRun((current) => ({ ...current, ...patch }));
    }

    // Terminal states share one summary-line shape (the output module's
    // final "✓ done in M:SS" / "✗ exited with code N after M:SS" / "■
    // stopped after M:SS" / "⚠ crashed …" line — see RunView/OutputPane) so
    // every way a run can end goes through the same bookkeeping. This also
    // has to handle a REPLAY correctly, not just a run that just finished
    // live: a lazily-tailed already-terminal run's journaled exit arrives
    // here minutes or days after the fact (see `maybeLazyTail`), so —
    // `endedAt` only ever gets set once, from whichever `finish` call
    // reaches it first (a live run has none yet; a replayed one was already
    // seeded with its real one by `recordFromSummary` and must keep it —
    // Date.now() here would silently overwrite history with "just now").
    // `dur` is derived from that same real `endedAt`, never from "how long
    // ago did this call happen to run", which for a replay is the run's AGE,
    // not its duration. And `applyBoardOutcome`'s transient/latching
    // overlays are themselves an "this just happened" signal — replaying an
    // old run must not blink the board as if it just finished (a replayed
    // failure is already latched, seeded up front by `latchFailureFromSummary`;
    // nothing is lost by skipping it here).
    function finish(
      exitCode: number | null,
      stopped: boolean,
      errorText: string | null,
      crashed: boolean = false,
    ) {
      let wasLive = false;
      const applied = patchRun((current) => {
        wasLive = current.running;
        const endedAt = current.endedAt ?? Date.now();
        const dur = formatDuration(endedAt - current.startedAt);
        let text: string;
        let outcome: NonNullable<OutputLine["outcome"]>;
        if (errorText !== null) {
          text = errorText;
          outcome = "error";
        } else if (crashed) {
          // No "(after X)" here — a crash never journals an end time, so
          // any duration is an invented one, not a real answer.
          text = `⚠ crashed — pult stopped without recording an exit`;
          outcome = "crashed";
        } else if (stopped) {
          text = `■ stopped after ${dur}`;
          outcome = "stopped";
        } else if (exitCode === 0) {
          text = `✓ done in ${dur}`;
          outcome = "success";
        } else {
          text = `✗ exited with code ${exitCode ?? "unknown"} after ${dur}`;
          outcome = "error";
        }
        return {
          ...current,
          running: false,
          stopped,
          crashed,
          exitCode,
          endedAt,
          lines: [...current.lines, { stream: "exit", text, outcome }],
        };
      });
      if (applied && wasLive) applyBoardOutcome(path, commandId, { stopped, exitCode, errorText, crashed });
    }

    function applyEvent(event: RunEvent) {
      switch (event.kind) {
        case "line":
          appendLine({ stream: event.stream, text: event.text });
          break;
        case "step":
          patchRun((current) => {
            const step = { k: event.k, n: event.n, name: event.name, at: Date.now() };
            return { ...current, step, stepHistory: [...current.stepHistory, step] };
          });
          break;
        case "progress":
          updateRecord({ progress: { pct: event.pct, text: event.text } });
          break;
        case "status":
          updateRecord({ status: event.text });
          break;
        case "exit":
          finish(event.code, event.stopped, null, event.crashed ?? false);
          break;
      }
    }

    return { patchRun, finish, applyEvent };
  }

  // Starts (or, if the backend's already tailing this run_id, no-ops into)
  // watching `runId`'s full backlog-then-live journal tail, registering its
  // handler in the shared `activeTails` map so the one app-wide subscription
  // (see `onMount`) routes its events here by run_id.
  function startTail(path: string, commandId: string, runId: string) {
    const { applyEvent } = makeRunHandlers(path, commandId, runId);
    activeTails.set(runId, applyEvent);
    void tailRun(path, runId);
  }

  // Lazy-tail hook (docs/run-journal.md's hydration leg): a hydrated/polled
  // terminal run is seeded from its `RunSummary` alone — no output, since a
  // summary carries none. The board never renders a finished run's lines
  // (only its running/lamp state), so the first (and, once tailed, only —
  // `record.lines.length` guards a repeat visit) moment output is actually
  // needed is opening that command's details page, which is exactly where
  // this is called from (`selectCommand`) — eagerly tailing a device's whole
  // run history on open instead would mean fetching output for commands the
  // user may never look at again.
  function maybeLazyTail(commandId: string) {
    if (!repoPath) return;
    const path = repoPath;
    const record = runsByRepo[path]?.[commandId];
    if (!record || record.running || record.lines.length > 0) return;
    startTail(path, commandId, record.runId);
  }

  // Groups `listRuns`'s newest-first result down to each command's single
  // newest run — the shape both hydration and the poll seed records from.
  function newestRunPerCommand(summaries: RunSummary[]): Map<string, RunSummary> {
    const newest = new Map<string, RunSummary>();
    for (const s of summaries) {
      if (!newest.has(s.command_id)) newest.set(s.command_id, s);
    }
    return newest;
  }

  // Hydration (docs/run-journal.md's "Desktop app changes when this lands",
  // item 2): seeds every command's newest journaled run into its record so
  // runs survive an app restart, then eagerly tails only the ones still
  // running (a finished run's output is filled in lazily — see
  // `maybeLazyTail`). Never overwrites a record already present for a
  // command — `loadRepo` calls this on every switch back to an
  // already-visited device this session, and the live in-memory record
  // (whether from a run started this session or an earlier hydration/poll)
  // is always more authoritative than a fresh disk read.
  async function hydrateRuns(path: string) {
    let summaries: RunSummary[];
    try {
      summaries = await listRuns(path);
    } catch (e) {
      // Run history is a nice-to-have, same posture as `loadDoctor` below —
      // its absence shouldn't block browsing or starting new runs.
      console.error(e);
      return;
    }
    if (repoPath !== path) return; // stale-switch guard, same pattern as loadDoctor
    const newest = newestRunPerCommand(summaries);
    const forRepo = { ...(runsByRepo[path] ?? {}) };
    let changed = false;
    for (const [commandId, summary] of newest) {
      if (forRepo[commandId]) continue;
      forRepo[commandId] = recordFromSummary(summary);
      latchFailureFromSummary(path, commandId, summary);
      changed = true;
    }
    if (changed) runsByRepo = { ...runsByRepo, [path]: forRepo };
    for (const [commandId, summary] of newest) {
      if (summary.status === "running") startTail(path, commandId, summary.run_id);
    }
  }

  // The CLI-run-visibility poll (docs/run-journal.md's demo consequence of
  // hydration): while a device is active, keep re-reading its journal so a
  // run started outside this app window (a real `pult deploy` typed in a
  // terminal, or another window of this same app) shows up without the user
  // doing anything. A command's newest run_id differing from what this
  // session already knows — whether because the command had no record at
  // all yet or because a fresh run superseded the one we had — means "never
  // seen before"; a same-run_id record we still believe is running but the
  // journal no longer does is the crash/missed-exit catch-up path (normally
  // redundant with the tail's own synthesized exit, but cheap insurance
  // against ever missing it — `startTail` on an id already being tailed is a
  // no-op both here and at the backend, so this never double-handles).
  async function pollRuns(path: string) {
    let summaries: RunSummary[];
    try {
      summaries = await listRuns(path);
    } catch (e) {
      console.error(e);
      return;
    }
    if (repoPath !== path || activePollPath !== path) return; // switched/stopped mid-request
    const newest = newestRunPerCommand(summaries);
    const forRepo = { ...(runsByRepo[path] ?? {}) };
    let changed = false;
    for (const [commandId, summary] of newest) {
      const current = forRepo[commandId];
      if (!current || current.runId !== summary.run_id) {
        forRepo[commandId] = recordFromSummary(summary);
        latchFailureFromSummary(path, commandId, summary);
        changed = true;
        if (summary.status === "running") startTail(path, commandId, summary.run_id);
      } else if (current.running && summary.status !== "running" && !activeTails.has(current.runId)) {
        // Catch-up: this record still says running but the journal no
        // longer does, and nothing is actively tailing it (the tail's own
        // synthesized exit should normally beat us here). A fresh tail
        // replays the backlog from offset 0, so reset the replayable
        // fields first — otherwise every line this record already
        // streamed live would be appended a second time on top of itself.
        forRepo[commandId] = {
          ...current,
          lines: [],
          step: null,
          stepHistory: [],
          progress: null,
          status: null,
        };
        changed = true;
        startTail(path, commandId, current.runId);
      }
    }
    if (changed) runsByRepo = { ...runsByRepo, [path]: forRepo };
  }

  function stopActivePoll() {
    if (activePollTimer) clearInterval(activePollTimer);
    activePollTimer = undefined;
    activePollPath = null;
  }

  function startPollFor(path: string) {
    stopActivePoll();
    activePollPath = path;
    activePollTimer = setInterval(() => void pollRuns(path), CLI_POLL_MS);
  }

  // Opens (or switches to) the device at `path`. Per-device session state
  // (runsByRepo & co.) deliberately survives this — only the active-listing
  // state resets. The post-await guards handle a device switch racing a slow
  // open: the stale open's listing must never clobber the newer device's.
  async function loadRepo(path: string) {
    stopActivePoll();
    repoPath = path;
    listingError = null;
    listing = null;
    doctorReport = null;
    selectedId = null;
    view = "board";
    showTrustModal = false;

    try {
      const result = await openRepo(path);
      upsertDevice(path, {
        name: result.name || basename(path),
        instruments: result.commands.length,
        status: result.trusted ? "ok" : "untrusted",
      });
      persistRack();
      if (repoPath !== path) return;
      listing = result;
      void hydrateRuns(path);
      startPollFor(path);
      if (result.trusted) {
        await loadDoctor(path);
      } else if (!readOnlyPaths.has(path)) {
        showTrustModal = true;
      }
    } catch (e) {
      upsertDevice(path, { status: "error" });
      persistRack();
      if (repoPath === path) listingError = String(e);
    }
  }

  async function loadDoctor(path: string) {
    try {
      const report = await doctorCheck(path);
      // Same stale-switch guard as loadRepo — a slow doctor for a device
      // the user already switched away from must not paint the new board.
      if (repoPath === path) doctorReport = report;
    } catch (e) {
      // Readiness is a nice-to-have overlay; a failure here shouldn't block
      // browsing or running commands. Lamps just stay unlit ("No check").
      console.error(e);
    }
  }

  async function handleMountDevice() {
    const path = await pickFolder();
    if (!path) return;
    // Re-picking an already-mounted folder just switches to it — and gives
    // its trust prompt another chance if "Not now" was clicked earlier.
    readOnlyPaths.delete(path);
    await loadRepo(path);
  }

  async function handleSelectDevice(path: string) {
    if (path === repoPath) return;
    await loadRepo(path);
  }

  function handleEjectDevice(path: string) {
    // Eject = unmount, nothing more: the journal leg means a run left going
    // is no longer an orphan nothing will ever look at again — it's visible
    // and stoppable from the CLI (or another window), and reappears here via
    // hydration if this device gets remounted (see `hydrateRuns`). Stopping
    // it here would be actively wrong now, not just unnecessary.
    if (activePollPath === path) stopActivePoll();
    for (const run of Object.values(runsByRepo[path] ?? {})) {
      activeTails.delete(run.runId);
    }
    for (const [key, t] of [...boardOverrideTimers]) {
      if (key.startsWith(`${path}\0`)) {
        clearTimeout(t);
        boardOverrideTimers.delete(key);
      }
    }
    runsByRepo = omitKey(runsByRepo, path);
    overridesByRepo = omitKey(overridesByRepo, path);
    paramValuesByRepo = omitKey(paramValuesByRepo, path);
    devices = devices.filter((d) => d.path !== path);
    readOnlyPaths.delete(path);
    if (repoPath === path) {
      repoPath = null;
      listing = null;
      listingError = null;
      doctorReport = null;
      selectedId = null;
      view = "board";
      showTrustModal = false;
    }
    persistRack();
  }

  function omitKey<T>(map: Record<string, T>, key: string): Record<string, T> {
    const { [key]: _omitted, ...rest } = map;
    return rest;
  }

  function toggleRackCollapsed() {
    rackCollapsed = !rackCollapsed;
    localStorage.setItem("pult-desktop:rack-collapsed", rackCollapsed ? "1" : "0");
  }

  async function handleTrust() {
    if (!repoPath) return;
    trustBusy = true;
    try {
      await trustRepo(repoPath);
      showTrustModal = false;
      await loadRepo(repoPath);
    } catch (e) {
      listingError = String(e);
      showTrustModal = false;
    }
    trustBusy = false;
  }

  function handleNotNow() {
    showTrustModal = false;
    if (repoPath) readOnlyPaths.add(repoPath);
  }

  function selectCommand(id: string) {
    selectedId = id;
    view = "run";
    maybeLazyTail(id);
  }

  function backToBoard() {
    view = "board";
  }

  async function handleRun(commandId: string, values: Record<string, string>) {
    if (!repoPath) return;
    // Captured once: every closure below writes through `runsByRepo[path]`
    // rather than the active-device `runs` slice, so this run keeps
    // streaming (and finishes, and sets its board overlay) even while its
    // device sits inactive in the rack.
    const path = repoPath;
    const runId = crypto.randomUUID();
    // Acknowledgment trigger 3 of 3 (see docs/design-language.md): starting
    // a new run clears any latched/transient overlay left over from the
    // previous one outright, rather than leaving it to `boardMeterFor`
    // (readiness.ts) to merely out-prioritize while running — that function
    // treats `running` as always winning, but the overlay would still be
    // sitting there ready to reappear if this run somehow ended without
    // setting a new one (it can't, `finish` below always does, but clearing
    // here is the actual acknowledgment moment, not a rendering detail).
    const key = timerKey(path, commandId);
    const existingTimer = boardOverrideTimers.get(key);
    if (existingTimer) clearTimeout(existingTimer);
    boardOverrideTimers.delete(key);
    setBoardOverride(path, commandId, null);
    runsByRepo = {
      ...runsByRepo,
      [path]: {
        ...(runsByRepo[path] ?? {}),
        [commandId]: {
          runId,
          running: true,
          lines: [],
          step: null,
          stepHistory: [],
          progress: null,
          status: null,
          stopped: false,
          crashed: false,
          exitCode: null,
          startedAt: Date.now(),
          endedAt: null,
        },
      },
    };

    // Same patch-run/finish/apply-event closures hydration/the poll/a lazy
    // tail use for a run they didn't start themselves (see `makeRunHandlers`)
    // — this is the one place a run_id is actually minted, everything after
    // that is shared.
    const { finish, applyEvent } = makeRunHandlers(path, commandId, runId);

    try {
      await runCommand(path, commandId, values, runId, applyEvent);
    } catch (e) {
      finish(null, false, String(e));
    }
  }

  function handleStop(runId: string) {
    if (repoPath) void stopRun(repoPath, runId);
  }

  function handleValuesChange(commandId: string, values: Record<string, string>) {
    if (!repoPath) return;
    const forRepo = paramValuesByRepo[repoPath] ?? {};
    paramValuesByRepo = { ...paramValuesByRepo, [repoPath]: { ...forRepo, [commandId]: values } };
  }

  function openSettings() {
    refreshSettingsInfo();
    showSettings = true;
  }

  async function saveSettings(path: string) {
    await setPultPath(path);
    showSettings = false;
    await refreshSettingsInfo();
  }

  // The one app-wide `pult://run-output` subscription every tailed run's
  // events route through by run_id (see `activeTails`'s doc comment) — set
  // up once, for the app's whole lifetime, independent of which device is
  // active or mounted at all (a background device's tail keeps delivering
  // here even while its device sits inactive in the rack).
  const unsubscribeRunOutput = subscribeRunOutput((event) => {
    const handler = activeTails.get(event.run_id);
    if (!handler) return;
    handler(event);
    if (event.kind === "exit") activeTails.delete(event.run_id);
  });

  onDestroy(() => {
    unsubscribeRunOutput();
    stopActivePoll();
  });

  onMount(() => {
    const saved = localStorage.getItem("pult-desktop:theme");
    if (saved === "light" || saved === "dark" || saved === "system") {
      theme = saved;
    }
    rackCollapsed = localStorage.getItem("pult-desktop:rack-collapsed") === "1";

    // Restore the rack (design 4a): the mounted-device list, then the
    // last-active device straight onto the board. In mock mode the store
    // starts empty and the `?mockstate` flows below drive mounting
    // themselves, so this resolves to a no-op there — the guards also keep
    // a slow store read from clobbering anything such a flow already did.
    void (async () => {
      const rack = await loadRack();
      if (devices.length === 0) devices = rack.devices;
      if (rack.activePath && !repoPath) await loadRepo(rack.activePath);
    })();

    // Mock-only screenshot helpers: `?theme=dark` forces a theme, and
    // `?mockstate=modal|trusted` drives the app straight to a given state
    // without manual clicking — used to script the light/dark/trust-modal
    // screenshots in a plain headless-Chrome pass (see README's mock mode
    // section). Never active outside VITE_MOCK=1.
    if (isMock) {
      const params = new URLSearchParams(window.location.search);
      const forcedTheme = params.get("theme");
      if (forcedTheme === "light" || forcedTheme === "dark") {
        theme = forcedTheme;
      }
      const mockState = params.get("mockstate");
      const forcedSelect = params.get("select");
      const forcedSearch = params.get("search");
      const forcedRun = params.get("run");
      const forcedTooltip = params.get("tooltip");
      // `?stopAfter=<ms>` — only meaningful alongside `?run=`: schedules an
      // automatic Stop click `ms` after the run starts, purely so the stop
      // flow (brief amber, no latch — see docs/design-language.md) can be
      // screenshotted from a one-shot headless render instead of needing
      // real interactive clicking.
      const stopAfter = params.get("stopAfter");
      if (forcedTooltip) forceTooltipId = forcedTooltip;
      if (mockState === "modal" || mockState === "trusted" || mockState === "untrusted") {
        void (async () => {
          await handleMountDevice();
          if (mockState === "trusted") {
            await handleTrust();
            if (forcedSelect) selectCommand(forcedSelect);
            if (forcedSearch) search = forcedSearch;
            // `?run=<command-id>` kicks off a mock run without navigating
            // into its run view, so a board screenshot can show a card
            // mid-run (running strip + amber meter) — see "Mock mode" in
            // the README.
            if (forcedRun) void handleRun(forcedRun, {});
            if (forcedRun && stopAfter) {
              const ms = Number(stopAfter);
              if (Number.isFinite(ms)) {
                setTimeout(() => {
                  const active = runs[forcedRun];
                  if (active?.running) handleStop(active.runId);
                }, ms);
              }
            }
          } else if (mockState === "untrusted") {
            // Dismiss the trust modal (as if the user clicked "Not now")
            // to land on the read-only board itself, all dark — the modal
            // would otherwise sit on top of it for every screenshot.
            handleNotNow();
          }
        })();
      }
    }
  });
</script>

<svelte:head>
  <title>{listing?.name ?? "pult-desktop"}</title>
</svelte:head>

<div class="app">
  <Toolbar
    repoName={listing?.name ?? null}
    {search}
    {theme}
    breadcrumb={view === "run" ? selectedBreadcrumb : null}
    onBack={view === "run" ? backToBoard : null}
    onSearch={(v) => (search = v)}
    onToggleTheme={cycleTheme}
    onOpenSettings={openSettings}
  />

  <div class="body">
    <Rack
      {devices}
      activePath={repoPath}
      {runningPaths}
      collapsed={rackCollapsed}
      onSelect={handleSelectDevice}
      onMountDevice={handleMountDevice}
      onEject={handleEjectDevice}
      onToggleCollapsed={toggleRackCollapsed}
    />
    {#if !listing}
      <div class="fill">
        <EmptyState
          message={listingError ??
            (repoPath
              ? "Powering up…"
              : devices.length
                ? "Select a device from the rack."
                : "Mount a repository to see its instruments.")}
          onMountDevice={handleMountDevice}
        />
      </div>
    {:else if view === "run" && selectedCommand}
      <main class="content-pane">
        <RunView
          command={selectedCommand}
          path={repoPath ?? ""}
          trusted={listing.trusted}
          {doctorReport}
          run={selectedRun}
          initialValues={paramValues[selectedCommand.id] ?? null}
          onRun={(values) => handleRun(selectedCommand.id, values)}
          onStop={() => selectedRun && handleStop(selectedRun.runId)}
          onValuesChange={(values) => handleValuesChange(selectedCommand.id, values)}
          onBack={backToBoard}
        />
      </main>
    {:else}
      <main class="content-pane">
        <Board
          groups={grouped.groups}
          trusted={listing.trusted}
          {doctorReport}
          {runs}
          overrides={boardOverrides}
          {search}
          {forceTooltipId}
          onSelect={selectCommand}
        />
      </main>
    {/if}
  </div>

  {#if showTrustModal && listing}
    <TrustModal {listing} busy={trustBusy} onTrust={handleTrust} onNotNow={handleNotNow} />
  {/if}

  {#if showSettings}
    <SettingsModal
      currentPath={pultPathSetting}
      {versionInfo}
      onSave={saveSettings}
      onClose={() => (showSettings = false)}
    />
  {/if}

  {#if isMock}
    <div class="mock-badge mono">MOCK</div>
  {/if}
</div>

<style>
  .app {
    height: 100vh;
    display: flex;
    flex-direction: column;
  }

  .body {
    flex: 1;
    min-height: 0;
    display: flex;
  }

  .fill {
    flex: 1;
    min-width: 0;
    height: 100%;
  }

  .content-pane {
    flex: 1;
    min-width: 0;
    min-height: 0;
    background: var(--bg);
  }

  .mock-badge {
    position: fixed;
    bottom: var(--space-2);
    right: var(--space-2);
    font-size: 10px;
    padding: 2px 6px;
    border-radius: 4px;
    background: var(--accent);
    color: var(--accent-ink);
    opacity: 0.85;
    pointer-events: none;
  }
</style>
