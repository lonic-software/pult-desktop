# Roadmap: from rack to remote to team

_Status: living document, written 2026-07-17. Owner: Máté. This is direction,
not commitment — each horizon stands on its own and is worth shipping even if
the ones after it never happen._

## Where we are (v0.2.x)

The desktop app owns everything about a run: it spawns `pult`, claims the
`PULT_EVENTS` channel, streams output into in-memory state, and renders it.
With the rack (design 4a), run state became per-device (`runsByRepo` in
`+page.svelte`) so runs survive switching devices — but nothing survives
quitting the app, and a run started from the CLI is invisible to it.

That coupling — *the app is the owner of runs* — is the one thing every item
on this roadmap needs loosened. The through-line of the whole document:

> **pult writes every run down; everything else is a viewer.**

Get that right once (Horizon 0) and CLI visibility, restart persistence,
push notifications, a phone app, and a team service are all "another reader
of the same stream" rather than five separate architectures.

---

## Horizon 0 — the run journal (prerequisite for everything)

**What:** every `pult` invocation — CLI or app-spawned — journals its run to
a well-known location, regardless of who is watching.

Sketch (to be specced properly in the pult repo, alongside
`docs/reference.md`'s events protocol):

- `<state-dir>/runs/<run-id>/meta.json` — command id, repo path, argv,
  param values (**never** secret ones), pid, process group id, start time,
  and a `status` that moves `running → exited(code) | stopped`.
- `<state-dir>/runs/<run-id>/events.jsonl` — append-only, exactly the
  `PULT_EVENTS` stream plus stdout/stderr lines; the same shapes the app's
  `RunEvent` type already mirrors. One writer (the pult process itself),
  N readers tailing.
- `<state-dir>` should default to a global per-user location
  (`~/.local/state/pult/` keyed by repo path, or platform equivalent) rather
  than inside the repo — repos aren't always writable, and run history in a
  worktree is history you lose with the worktree.
- Crash detection: a `running` status with a dead pid (liveness-check on
  discovery) renders as "crashed" — no heartbeat machinery needed at this
  scale.
- Retention: keep last N runs per command, prune on write. Simple beats
  clever here.

**Desktop app changes:** stop being the pipe-owner. Spawn runs detached,
discover runs by watching the journal directory, tail `events.jsonl` for
live ones. This buys, in one move:

1. **CLI runs appear in the app**, live (file tailing is well within the
   latency needs of build output).
2. **Quit/reopen stops being special** — history is on disk; still-live runs
   are re-discovered and re-tailed. The current orphaned-child-on-quit
   behavior disappears as a side effect.
3. **Stop works for runs the app didn't spawn** — `meta.json` records the
   pgid; the app signals it, same SIGTERM→SIGKILL ladder as today.

**Decisions to make now so later horizons don't force rework:**

- Run ids globally unique and stable (they become URL/subscription keys in
  Horizons 2–4).
- `events.jsonl` schema versioned from day one (schema field in meta), same
  additive-only discipline as the listing schema.
- Param values in the journal are the *audit record* — design the
  secret-redaction rule now, because Horizon 4 turns this file into a
  compliance feature.

## Horizon 1 — "tell me when it's done" (cheap, ship early)

A run finishing is the single highest-value event in the system. Before any
companion app exists: an optional notifier in the desktop app (or in pult
itself) that fires a webhook on run completion — ntfy.sh / a Slack hook /
anything user-configured. Failure and success, command title, duration,
exit code. Nearly zero effort, immediately useful, and it validates demand
for Horizon 2 with real usage instead of guesses.

## Horizon 2 — companion app (monitor + stop)

The handheld remote for your rack. **Monitoring is the product; triggering
is a feature that comes later** (Horizon 3) — the away-from-keyboard
scenario is overwhelmingly "is my deploy green yet?".

- Scope for v1: paired devices see the rack (devices → commands → runs),
  live run output/stages/meters, push notification on completion, and a
  **Stop** button. Stop is the one control worth shipping early: real value
  ("kill the runaway deploy from the train"), near-zero attack surface.
- Architecture: the machine-side publisher tails the journal (Horizon 0)
  and forwards events. First version: the desktop app *is* the publisher —
  companion works while the app runs, which is fine because the app spawned
  the runs anyway. Later: a tiny menubar/launchd agent so CLI runs publish
  with the app closed. That agent is a sync relay, **not** a run owner —
  keep it that way as long as possible.
- Push + away-from-LAN requires a relay server (APNs won't talk to a
  laptop). Keep it dumb: authenticated event forwarding, no state beyond
  delivery. Self-hostable, hosted-by-us as the default. This relay is the
  seed of Horizon 4's control plane — design its event schema as "journal
  entries over the wire," nothing more.
- Pairing: QR code from the desktop app, per-device keypair, E2E encryption
  of run content through the relay (the relay routes, it doesn't read).
- Client tech: Tauri 2 does iOS/Android — the meters, towers, and CRT
  screens are Svelte components and can be reused outright. A running
  command is a textbook iOS Live Activity (lock-screen progress, green/red
  resolution); that's the flagship moment of the whole app.

## Horizon 3 — remote triggering

Remote execution of shell commands is RCE by design; it ships last among
the single-user features and gated hard:

- Per-command opt-in in the manifest (`remote: true` in pult.yaml) — the
  manifest is already the trust surface; extend it, don't invent a second
  one. Default is not-exposed.
- Trigger requires a paired device (Horizon 2 keys) + explicit confirmation
  on the phone. Params yes, but secret params never sync to or from the
  phone — a remote trigger of a command needing a secret uses the value
  remembered on the machine or refuses.
- Every remote trigger lands in the journal with its origin device — the
  audit story starts here, not in Horizon 4.

## Horizon 4 — the team service (hosted pult)

The idea: a hosted service where a team shares commands *and their
executions* — someone starts a deploy, everyone sees it running; new
teammates run day-one commands without environment setup; desktop + mobile
apps as first-class clients.

Honest assessment, so this section stays useful when the excitement wears
off:

**The strong half is shared *visibility*, not shared *compute*.** "Who is
deploying what right now" is a real, painful, universally-hacked-around
problem (every team's Slack deploy-bot is this product, badly). The journal
protocol makes it almost free: a team control plane is the Horizon-2 relay
grown up — runs published to an org instead of a person, everyone
subscribes to the same streams. RBAC on `remote: true` commands, and the
journal's meta/audit trail becomes a genuine selling point (who ran what,
when, with which params — compliance people pay for exactly this).

**The hard half is "I provide the runtime environment."** That's building a
CI-runner service: sandboxing, secrets management, per-team isolation,
caching, compute billing, cold starts — competing with CI vendors on their
home turf, as infrastructure. It also inverts pult's current identity
(local-first, your machine, your env, zero setup beyond one binary).

**Recommended wedge: hosted control plane, BYO runners.** The
Buildkite/self-hosted-runner model: the service owns identity, visibility,
audit, notifications, and routing; execution happens on machines the team
already has (dev laptops via the Horizon-2 agent, or a shared box running
the same agent). This keeps the local-first soul, avoids owning compute,
and ships the actually-differentiated part first. Hosted runners become a
premium tier *if* the control plane proves out — by then the agent protocol
is the runner protocol.

**Differentiation vs CI:** CI is commit-triggered, async, and config-heavy;
pult is *human-triggered operational commands with a control-panel UX*. The
rack/instrument identity is not decoration here — "operations you can
watch like hardware" is the brand, and nobody else in the space looks or
feels like this. Cautionary comp: Airplane.dev (closest analog — hosted
internal runbooks/tasks) had real traction and still exited into a
shutdown; the lesson is less "no market" than "the market buys the
visibility/guardrails story, and compute is a cost center."

**Grows-the-scope-warning:** Horizon 4 adds orgs, auth, billing, a web
client, and on-call-shaped reliability expectations. It's a company, not a
feature. Do not start it before Horizon 2 has real users.

---

## Non-goals (for now)

- **A run-owning daemon on the local machine.** The journal + detached
  spawns + a dumb sync agent cover every current need. A daemon becomes
  worth it only for interactive attach (remote stdin, tmux-style) — revisit
  when that's actually demanded.
- **Interactive commands remotely.** `interactive: true` stays
  terminal-only everywhere.
- **Windows event parity before the journal.** `PULT_EVENTS` is Unix-only
  today; the journal spec should define the Windows story (lines + exit
  only is acceptable) rather than inheriting the gap silently.

## Sequencing summary

```
H0 journal ──► H1 notify ──► H2 companion (monitor+stop) ──► H3 remote trigger ──► H4 team plane
   │                              │                                                    │
   └── unlocks CLI visibility,    └── relay server is the seed                         └── control plane first,
       restart persistence            of the team control plane                            BYO runners, hosted
       (desktop app features              (design events as                                compute last
        land immediately)                  "journal over the wire")
```

Every horizon leans on the journal being the single source of truth. If a
design choice ever forces a second source of truth for runs, that choice is
wrong.
