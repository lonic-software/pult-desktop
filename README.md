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
- `?theme=light|dark` — force a theme regardless of OS preference

These are inert outside `VITE_MOCK=1`. Used to script the canonical
screenshot set: board light, board dark, run view, and a search-filtered
board.

## Design tokens

An instrument-panel identity — precise, quiet, engineered. Amber is the only
interactive accent (focus rings, the primary Run button); green/red/gray are
reserved for the readiness lamp alone. Full token values live in
`src/lib/styles/tokens.css`; summary:

| Token | Light | Dark |
|---|---|---|
| `--bg` | `#F7F8FA` | `#141619` |
| `--panel` | `#FFFFFF` | `#1C1F24` |
| `--ink` | `#1A1D23` | `#E9EBEE` |
| `--muted` | `#5C6470` | `#8A919C` |
| `--line` | `#E3E6EB` | `#2A2E35` |
| `--accent` (amber) | `#B8770A` | `#E5A93D` |
| `--lamp-green` | `#2FA463` | `#3FBF77` |
| `--lamp-red` | `#D14343` | `#E05C5C` |
| `--lamp-off` | `#9AA1AB` | `#9AA1AB` |

Radii: 6px controls, 10px panels. Hairline (1px) borders, no shadows except
the trust modal's soft elevation. 4px spacing grid. Type: IBM Plex Sans for
UI text (13px body, 15px/600 command titles, 11px/500 uppercase group
labels), IBM Plex Mono for ids/param names/check commands/output. Fonts are
vendored as local woff2 files under `static/fonts/` — Tauri's CSP forbids
remote assets, and this keeps the app fully self-contained offline too (see
`static/fonts/LICENSE-IBMPlex{Sans,Mono}.txt`, SIL OFL).

The signature element is the **readiness lamp**: an 8px dot (12px in the
run view header) with a soft glow when lit — green for a passing `check:`, red
for a failing one, flat unlit gray for "no check" or "untrusted". On first
load or a doctor refresh, lamps power on with a 40ms-per-row stagger
(capped at 24 rows); `prefers-reduced-motion` collapses this to instant.

## Layout

**Board home** replaces the old permanent sidebar. One section per display
group (same grouping rule as always — see below), headed by the silkscreen
label. Each section is a card grid — `repeat(auto-fill, minmax(240px, 1fr))`,
16px gaps. A card is a panel with a hairline border and 10px radius:
readiness lamp + title on the first line;
an optional 1–2 sentence description (`description`, an additive field —
absent/null renders a deliberate title-only card, not a bug); a Plex Mono
footer with the command id and, right-aligned, either `terminal-only` (an
`interactive` command) or a param count. A card is a single focusable
button (tab + Enter, amber focus ring); hovering shifts the border toward
`--ink`. A command currently running shows a slim indeterminate amber strip
along the card's bottom edge and swaps its footer marker for "Running…" —
and that state survives navigating away, since run state lives above the
board/run-view switch (see `src/routes/+page.svelte`), letting more than one
command run at once. The lamp power-on stagger (40ms/row, capped at 24 rows,
instant under `prefers-reduced-motion`) plays across the whole board in
row-major order. The toolbar (repo name, "Open repository…", search) is
unchanged; search filters cards live across sections, hides empty sections,
and shows a "no matches" state.

**Run view** is what clicking a card opens: a focused takeover of the
content area (not a modal) with a "← Board" control (also `Esc`), an
enlarged lamp + title + description + a plain status line, the generated
param form, the Run button, and the streamed output pane.

Design rationale: the board *is* the product's own name — Hungarian for
dashboard — so the UI leans into that metaphor directly. A glance at the
board should read as state-at-a-glance, the way a real instrument panel
does, rather than as a settings list you have to click into to understand.

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
"Mock mode" above for the URL params used to script them. The canonical set:

- `board-light.png` — `?mockstate=trusted&theme=light`
- `board-dark.png` — `?mockstate=trusted&theme=dark`
- `run-view.png` — `?mockstate=trusted&theme=light&select=<command-id>`
- `board-search.png` — `?mockstate=trusted&theme=light&search=<query>`
