# pult-desktop

A desktop companion for [`pult`](https://github.com/lonic-software/pult) — a
console for the commands a repository already declares in its `pult.yaml`.
Open a repository, review what it's offering (and where those commands come
from), trust it once, and run commands through a proper form instead of
memorizing flags.

"Pult" is Hungarian for **dashboard** — the board of controls in front of an
operator. The app takes that literally: opening a repository surfaces a
**board** of command cards, grouped and lit up by readiness, not a sidebar
list with a detail pane bolted on. See "Layout" below.

This app doesn't reimplement any of pult's logic. It's a thin client over
pult's **documented machine surfaces** — everything it does, pult's CLI could
also do standing alone.

## The sidecar contract

pult-desktop consumes pult **≥ 0.4** via exactly these interfaces (see the
pult repo's `docs/reference.md` for the authoritative spec):

- `pult --list --json` — the listing: commands, params, includes, trust state
- `pult --trust --list` — record trust for the current manifest
- `pult doctor --json` — readiness (`check:` results) for every command
- `pult <id> --params-json` — run a command, values fed on stdin (keeps
  secrets out of argv and shell history)
- `pult --version` — sanity-check the resolved binary

Schema 1 is additive-only; this app deserializes leniently (unknown fields
are ignored, never rejected) so it keeps working across pult point releases.

## Dev setup

Requires Node 20+ and a Rust toolchain (stable). `pult` itself is **not**
bundled — see "Sidecar bundling" below — so you need a `pult` binary on your
`PATH`, or point Settings at one.

```sh
npm install
npm run tauri dev     # full app: Rust backend + Svelte frontend, real pult
```

Other useful commands:

```sh
npm run build         # vite build (frontend only)
npm run check         # svelte-check
cd src-tauri && cargo check   # Rust typecheck
cd src-tauri && cargo test    # backend integration tests (see below)
```

## Mock mode

```sh
VITE_MOCK=1 npm run dev
```

Runs the frontend alone in a plain browser against fixture data in
`src/lib/mock/fixtures.ts` — no Tauri runtime, no real `pult` needed. Useful
for UI work and screenshotting. The fixture models a realistic listing: three
display groups (a local group, a `Deploy` category, and a group from an
included module named "AWS Tooling"), a secret param, a failing readiness
check, an interactive command, a dynamic (`pick.from`) param, and starts
untrusted so the trust flow is reachable by just opening the repo.

Mock mode also understands a few URL params, used to script screenshots
without manual clicking (see `onMount` in `src/routes/+page.svelte`):

- `?mockstate=modal` — open the fixture repo and stop at the trust modal
- `?mockstate=trusted` — open and trust it, landing on the board
- `?select=<command-id>` — additionally open a command's run view (with `trusted`)
- `?search=<query>` — additionally filter the board (with `trusted`)
- `?run=<command-id>` — additionally kick off a mock run in the background
  (with `trusted`), without navigating into its run view — lets a board
  screenshot catch a card mid-run (amber meter, running strip); combine
  with a short wait after page load, since the run completes on its own
  mock timers (see `handleRun` in `src/routes/+page.svelte`)
- `?theme=light|dark` — force a theme regardless of OS preference

These are inert outside `VITE_MOCK=1`. Used to script the canonical
screenshot set: board light, board dark, a board with a card mid-run, and
a search-filtered / narrow-window board.

## Design tokens

**The "1b Faceplate" system.** The board's controls are meant to look
machined into a panel — raised pads, recessed wells, emboss highlights,
engraved labels, corner screws — rather than flat cards on a page. This is
ported faithfully from the user's own [Claude Design](https://claude.ai/design)
project, "Pult board redesign," variant 1b (see "Layout" below for the
rationale and provenance). Amber is still the only interactive accent;
green/red/amber are reserved for the readiness meter alone. Full token
values live in `src/lib/styles/tokens.css`; summary:

| Token | Light | Dark |
|---|---|---|
| `--bg` | `#DCDEE2` | `#0E0F12` |
| `--panel` | `#F4F5F7` | `#1C2026` |
| `--pad` | `#ECEEF1` | `#20242B` |
| `--ink` | `#1A1D23` | `#E9EBEE` |
| `--muted` | `#5C6470` | `#8A919C` |
| `--line` | `#DDE0E5` | `#31363E` |
| `--accent` (amber) | `#E0982A` | `#E5A93D` |
| `--lamp-green` | `#38C680` | `#3FBF77` |
| `--lamp-red` | `#E05555` | `#E05C5C` |
| `--seg-off` | `#C4C8CE` | `#2B3037` |
| `--engrave` | `#4E555F` | `#AAB0B9` |
| `--screw` | `#B6BAC1` | `#43484F` |
| `--emboss-light` | `rgba(255,255,255,.95)` | `rgba(255,255,255,.06)` |
| `--led-well` | `#565D66` | `#08090C` |
| `--well-inset` | `rgba(0,0,0,.3)` | *(unset — per-usage fallback)* |

Note `--bg` is deliberately a darker gray than `--panel`/`--pad` in light
mode — that's what makes panels and pads read as raised/recessed plates
against it rather than flat cards on a page, and the board itself carries a
subtle vertical pinstripe (a repeating gradient) reinforcing the machined-
panel surface. `--well-inset` is intentionally undefined in dark mode: two
different recessed elements (the LED well, the toolbar search input) each
supply their own fallback shadow depth via `var(--well-inset, <fallback>)`,
and only light mode overrides it uniformly.

Radii: 5-6px controls, 8px modules, 10px other panels (modals). Hairline
(1px) borders, emboss insets, and soft drop shadows on every raised
surface — no flat chips. 4px spacing grid. Type: IBM Plex Sans for UI text
(13px body, 15px/600 command titles, 10.5px/600 tracked-uppercase engraved
module labels), IBM Plex Mono for ids/param names/check commands/output.
Fonts are vendored as local woff2 files under `static/fonts/` — Tauri's CSP
forbids remote assets, and this keeps the app fully self-contained offline
too (see `static/fonts/LICENSE-IBMPlex{Sans,Mono}.txt`, SIL OFL).

The signature element is the **segmented LED meter** (replacing an earlier
round lamp): five vertical segments recessed into a dark well, lit
bottom-up — 4 green for a passing `check:`, 5 red for a failing one, 3 amber
(with a matching glow) while a command is actively running, all unlit for
"no check" or "untrusted." Running always wins over the last known
readiness. On first load or a doctor refresh, meters power on with a
40ms-per-row stagger (capped at 24 rows); `prefers-reduced-motion` collapses
this — and the running strip's sweep animation — to instant.

## Layout

**Board home** replaces the old permanent sidebar with a rack of bordered
modules — one per display group (same grouping rule as always — see
below) — packed left-to-right and wrapping, styled like an engraved
OSC/FILTER/ENV block on a synth faceplate: a hairline frame, an emboss
highlight + drop shadow, four corner screws, and a centered engraved label
breaking a hairline divider (plain markup, no positioning tricks — this
replaces the previous `fieldset`/`legend` construction from an earlier
iteration). The board is a grid of 150px rack columns; each module spans up
to 3 of them — one per card, capped — so a 1-command group is a narrow
module and a 7-command group is a wide one with extra internal rows,
sized to its own honest width rather than stretched to fill the row. Since
`grid-column: span N` won't shrink itself on a narrow window, the rack's
actual rendered width is measured (`bind:clientWidth`) and every module's
span is clamped to however many columns actually fit, so narrow windows
reflow instead of overflowing.

Inside a module, cards are pressable pads (`--pad` surface, emboss
highlight, drop shadow, 6px radius) that nudge down 1px with a flattened
shadow on hover/active — a physical press, not just a border change. Each
pad: a segmented LED meter on the left, then title (15/600, nowrap-
ellipsis), an optional 1–2 sentence description (`description`, an
additive field — absent/null renders a deliberate title-only card, not a
bug, 2-line clamp otherwise), and a Plex Mono footer with the command id
and, when relevant, either `terminal-only` (an `interactive` command) or a
param count — the id keeps a readable floor and the marker gives way first,
since the narrowest rack modules don't leave much room for both. A pad is a
single focusable button (tab + Enter, amber focus ring). A command
currently running shows a slim indeterminate amber strip along the pad's
bottom edge and swaps its footer marker for "Running…" (given more room
than the static marker it replaces) — and that state survives navigating
away, since run state lives above the board/run-view switch (see
`src/routes/+page.svelte`), letting more than one command run at once. The
toolbar (repo name, "Open repository…", search) keeps the same raised-
button / recessed-search-input language; search filters cards live across
modules, hides empty ones, and shows a "no matches" state.

**Run view** is what clicking a card opens: a focused takeover of the
content area (not a modal) with a "← Board" control (also `Esc`), an
enlarged LED meter + title + description + a plain status line, the
generated param form, the Run button, and the streamed output pane.

Design rationale: the board *is* the product's own name — Hungarian for
dashboard — so the UI leans into that metaphor directly, and the faceplate
system pushes it further: state-at-a-glance should look like reading an
actual instrument panel, not a settings list. The board's visual design was
imported from the user's Claude Design project "Pult board redesign,"
variant "1b Faceplate" — chosen by the user from several options and
ported here as closely as the app's real content (longer command ids and
descriptions than the design mockup's placeholders) allows.

## v0 scope

**Works end-to-end**, against a real `pult` binary:

- Open a repository (folder picker) → `pult --list --json` → the board,
  grouped per pult's documented rule (`category` → module name → include
  origin → implicit "local"; local-containing groups first, then include
  order — implemented once in `src/lib/grouping.ts`)
- Trust modal (manifest path + includes, pinned revs) → `pult --trust --list`
  → reload; "Not now" leaves the app read-only (forms visible, Run disabled)
- Readiness via `pult doctor --json` (trust-gated, matches pult's own gate)
- Run a command: generated param form (pick/options → select, pick/source →
  text input with a "comes from the repository at prompt time" hint,
  input → text, `secret: true` → password), values sent via
  `--params-json` over stdin, stdout/stderr streamed live via Tauri events
  into a mono output pane, exit code shown at the end. Each run gets a
  client-generated `run_id` threaded through every event on the shared
  `pult://run-output` channel, so more than one command can run at once
  without their output getting cross-attributed (see `RunEvent` in
  `src-tauri/src/types.rs` and `src/routes/+page.svelte`'s `runs` map).
- `interactive: true` commands refuse to run in-app with an explanatory hint
  (run it in a real terminal instead — no pty in v0)
- `pult` binary resolution: `which pult`, overridable in Settings (stored via
  `tauri-plugin-store`)

**Not wired up yet** (see "Next steps"):

- Sidecar bundling
- A pty-backed runner for `interactive` commands
- Dynamic `pick.from` resolution (v0 shows the raw source as a hint, doesn't
  shell out for live options)
- `PULT_EVENTS` step-ladder rendering (progress/status/step events)

## Next steps

- **pty runner** — a real terminal surface (portable-pty or similar) so
  `interactive: true` commands can run in-app instead of being refused.
- **`PULT_EVENTS` step ladder** — pult already exposes `steps` per command in
  `--list --json` and emits `step k/n <name>` / `progress` / `status` on the
  fd named by `$PULT_EVENTS` when nothing else claims it. The desktop app
  should claim that channel itself (pult passes it through untouched when
  already set — see docs/reference.md's Events protocol) and render a live
  step/percentage indicator instead of just raw output lines.
- **Dynamic pick sources** — actually shell out for `pick.source` options at
  prompt time (with the declared `depends_on` params filled in), instead of
  a plain text field with a hint.
- **Sidecar bundling** — ship a checksummed `pult` release binary alongside
  the app as a Tauri sidecar, so pult-desktop works with no separate
  install. Planned shape: fetch the platform-appropriate release asset at
  package time, verify its checksum, register it via `tauri.conf.json`'s
  `bundle.externalBin`, and prefer it in `pult_bin::resolve_pult` when no
  user override and no `PATH` binary are more specific. Left as comments in
  `src-tauri/src/pult_bin.rs` for now.
- **Packaging/signing** — code signing and notarization (macOS), and
  installer generation for the other platforms, once the app is otherwise
  stable.

## Testing notes

Driving the actual Tauri window headlessly isn't practical in this
environment (no display server), so the real-mode smoke test is a set of
`cargo test` integration tests (`src-tauri/tests/pult_backend.rs`) that call
the real `pult` binary against a fixture repo checked into
`src-tauri/tests/fixtures/repo/` — the same listing/trust/doctor/run JSON
contracts the Tauri commands use, minus the `AppHandle`. Each test uses its
own isolated `PULT_TRUST_STORE` temp file so nothing touches your real trust
store. They default to `../../tui/target/debug/pult` relative to this repo;
override with `PULT_DESKTOP_TEST_BIN` if your `tui` checkout lives elsewhere.
Tests skip (rather than fail) if no binary is found.

Mock-mode UI screenshots are the other half of manual verification — see
"Mock mode" above for the URL params used to script them. The current
canonical set (faceplate system, 1200×760 unless noted):

- `faceplate2-light.png` — `?mockstate=trusted&theme=light`
- `faceplate2-dark.png` — `?mockstate=trusted&theme=dark`
- `faceplate2-running.png` — `?mockstate=trusted&theme=light&run=<command-id>`,
  screenshotted shortly after load so the run is still in flight
- `faceplate2-narrow.png` — same as light, at 760px wide
