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
also do standing alone. The one documented exception is resolving a dynamic
`pick.source`'s live options: pult 0.4 has no CLI surface for that (`--print`
is explicitly side-effect-free and never runs one), so the app replicates
pult's own documented `pick.from` semantics itself — see "Dynamic pick
sources" below.

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

### Dynamic pick sources

A `pick` param with `source` (`pick.from` in the manifest) has no resolve
subcommand in pult 0.4 to defer to, so `src-tauri/src/pult_bin.rs`'s
`resolve_pick_source` shells the source command out itself, replicating
pult's documented `pick.from` / `run:` semantics from `docs/reference.md`
rather than inventing new ones: `sh -c`, strict `{param}` interpolation over
only the param's declared `depends_on` values (shell-quoted, `{{`/`}}` escape
literal braces), stdout lines (trimmed, non-empty) become options, and a
non-zero exit or empty result is an error. It's gated on trust the same way
`pult doctor` gates `check:` — both are manifest-authored shell code — except
this call never hands off to `pult` to enforce that itself (there's nothing
to hand off to), so it re-checks `trusted` via its own `pult --list --json`
call rather than accepting a frontend-supplied flag.

## Dev setup

Requires Node 20+ and a Rust toolchain (stable). `pult` doesn't need to be
installed separately — `npm run tauri dev`/`tauri build` fetch a pinned,
checksummed release binary automatically and bundle it as a sidecar (see
"Sidecar bundling" below); a `PATH` install or a path set in Settings is
still preferred over it when present.

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

`cargo` invoked directly (as above) skips the Tauri CLI's fetch hook, so the
sidecar binary needs to already exist once — run `npm run tauri dev` first,
or `node scripts/fetch-pult-sidecar.mjs` directly. This is because
`bundle.externalBin` (see "Sidecar bundling") makes Tauri's build script
validate that the resource exists on *every* build of the `src-tauri` crate,
not just `tauri build`.

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
- `?mockstate=untrusted` — open the fixture repo and dismiss the trust modal
  (as if "Not now" were clicked), landing on the read-only board itself,
  still untrusted — for a clean all-dark-meters screenshot without the
  modal sitting on top
- `?select=<command-id>` — additionally open a command's run view (with `trusted`)
- `?search=<query>` — additionally filter the board (with `trusted`)
- `?run=<command-id>` — additionally kick off a mock run in the background
  (with `trusted`), without navigating into its run view — lets a board
  screenshot catch a card mid-run (amber meter, running strip); combine
  with a short wait after page load, since the run completes on its own
  mock timers (see `handleRun` in `src/routes/+page.svelte`)
- `?tooltip=<command-id>` — force that card's description tooltip open on
  mount (skipping the real hover delay), for scripting a tooltip screenshot
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
bottom-up. State → segments:

| State | Segments | Glow |
|---|---|---|
| Untrusted | none lit | none — a dark board reads as "nothing known yet," not broken |
| Trusted, no `check:` declared | 1, neutral gray (`--muted`) | none — "powered, no probe" |
| Ready (`check:` passing) | 4, green | yes |
| Check failed | 5, red | yes |
| Running | 3, amber | yes |

Running always wins over the last known readiness. The single gray segment
is a deliberate third state, not a dimmer "off": it's only reachable once
trust and doctor both agree there's genuinely no `check:` to run, and its
gray (`--muted`) is chosen specifically to read as *lit* against `--seg-off`
in both themes — darker than `--seg-off` in light mode, lighter than it in
dark mode, the same relationship colored segments have to it.

The well itself (dark recess, chrome, unlit segments) renders immediately
with the card and never animates — a doctor-latency gap should look like
dark hardware waiting for power, not missing UI. Only the *illumination*
animates: when doctor state arrives or trust flips, newly-lit segments
transition off→color (with the glow fading in alongside) with a
40ms-per-row stagger (capped at 24 rows), 200ms per meter; a later doctor
refresh only animates the meters whose state actually changed, since
unchanged segments have nothing to transition. `prefers-reduced-motion`
collapses all of this — and the running strip's sweep animation — to
instant.

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
pad: a segmented LED meter on the left, then a short title (pult's
authoring convention is 1-2 words — nowrap-ellipsis, single line, no
reserved 2-line box), an optional description (`description`, an additive
field — absent/null renders a deliberate title-only card, not a bug,
3-line clamp otherwise) carrying the actual explanation, and a Plex Mono
footer with the command id and, when relevant, either `terminal-only` (an
`interactive` command) or a param count — the id keeps a readable floor and
the marker gives way first, since the narrowest rack modules don't leave
much room for both. A description long enough that the 3-line clamp still
truncates it gets a custom tooltip on hover (after a short delay) or
keyboard focus — a small `--panel`-styled bubble (hairline border, soft
shadow, capped width), positioned to flip above/below so it never clips at
the viewport edge, rendered only when the text actually overflows (never
the native `title` attribute). A pad is a single focusable button (tab +
Enter, amber focus ring). A command
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
  select populated by live resolution — see "Dynamic pick sources" above;
  loading/unmet-`depends_on`/resolve-failure states, debounced re-resolve
  when a `depends_on` value changes, input → text, `secret: true` →
  password), values sent via `--params-json` over stdin, stdout/stderr
  streamed live via Tauri events into a mono output pane, exit code shown at
  the end. Each run gets a client-generated `run_id` threaded through every
  event on the shared `pult://run-output` channel, so more than one command
  can run at once without their output getting cross-attributed (see
  `RunEvent` in `src-tauri/src/types.rs` and `src/routes/+page.svelte`'s
  `runs` map).
- `interactive: true` commands refuse to run in-app with an explanatory hint
  (run it in a real terminal instead — no pty in v0)
- `pult` binary resolution: settings override → `which pult` on PATH → a
  bundled sidecar binary, so the app works with no separate install (see
  "Sidecar bundling" below)

**Not wired up yet** (see "Next steps"):

- A pty-backed runner for `interactive` commands
- `PULT_EVENTS` step-ladder rendering (progress/status/step events)

## Sidecar bundling

pult-desktop ships a checksummed `pult` release binary inside the app
bundle, so it works with no separate `pult` install. `src-tauri/src/pult_bin.rs`'s
`resolve_pult` resolves the binary to run in this order:

1. A path saved in Settings (`tauri-plugin-store`) — an explicit user choice
   always wins.
2. `which pult` on the user's `PATH` — a system install is still preferred
   over the bundled copy.
3. The bundled sidecar, next to the app's own executable — the fallback
   that makes the app work out of the box.

The sidecar itself: `src-tauri/sidecar.json` pins the bundled pult version
and the sha256 of each target triple's release asset (independently
computed against the downloaded bytes when pinned — not just copied from
the release's own `checksums.txt`, since that only guards against
corruption, not a compromised upload). `scripts/fetch-pult-sidecar.mjs`
reads that manifest, downloads the asset for one target triple (arg,
`$TAURI_ENV_TARGET_TRIPLE`, or host detection), verifies its checksum
(hard failure on mismatch), extracts it, and writes
`src-tauri/binaries/pult-<target-triple>[.exe]` — the naming convention
`tauri.conf.json`'s `bundle.externalBin` expects, which it later strips
back down to `pult`/`pult.exe` when copying the resource next to the app's
executable in every bundle type we ship (`.app`, NSIS, `.deb`, AppImage).
Already-correct downloads/binaries are skipped, so re-running the script
locally is cheap. Checksums are verified once, at package time — not
re-checked at runtime, since by the time the sidecar is sitting inside a
distributed bundle, that would only catch tampering with the user's own
install.

The fetch script is wired into both `beforeDevCommand` and
`beforeBuildCommand` in `tauri.conf.json`: once `externalBin` is
configured, Tauri's build script validates the resource exists on *every*
`cargo build`/`check`/`clippy`/`test` of the `src-tauri` crate, not only
`tauri build` — so `tauri dev` needs it too, not just packaging. Running
`cargo` directly (bypassing the Tauri CLI, e.g. plain `cargo check`) skips
this hook, so the binary needs to already exist from a prior
`npm run tauri dev` or a manual `node scripts/fetch-pult-sidecar.mjs` run.

**Bumping the bundled pult version**: edit `src-tauri/sidecar.json` —
update `version`, then replace every asset's `sha256` with the checksum of
the new release's asset (download each one and run `shasum -a 256` on it
yourself). Nothing else needs to change.

CI's release workflow (`.github/workflows/release.yml`) needs no changes:
`tauri-action` runs `tauri build`, which fires `beforeBuildCommand`, and the
Tauri CLI sets `TAURI_ENV_TARGET_TRIPLE` to the actual build target even
when it differs from the runner's host architecture — verified directly by
running `tauri build --target x86_64-apple-darwin` on an aarch64 runner and
confirming the hook saw `TAURI_ENV_TARGET_TRIPLE=x86_64-apple-darwin`. This
is what makes the macOS matrix's two explicit `--target` builds (Apple
Silicon and Intel, both possibly running on the same arm64 runner) fetch
the correct sidecar for each. CI's lint workflow (`ci.yml`) *does* need a
step, though: `cargo clippy` runs directly, not through the Tauri CLI, so
nothing sets `TAURI_ENV_TARGET_TRIPLE` or runs the fetch hook for it — the
`rust` job fetches the sidecar for the runner's own host triple before
linting.

## Maintaining pult-desktop with pult

This repo dogfoods the tool it's a client for: `pult.yaml` declares this
repo's own maintainer commands. Open it in pult-desktop itself, or run
`pult` from the repo root for the guided menu (`pult --list` for the flat
list). Real logic lives in `./bin`; the yaml is one-liners over it.

- `pult dev` — `npm run tauri dev`. Declared `interactive: true`: it blocks
  and needs Ctrl+C, and pult-desktop has no pty runner yet (see "Next
  steps" below), so it refuses to run this one in-app — run `pult dev` from
  a real terminal instead.
- `pult build` — `npm run tauri build`.
- `pult check` — the pre-release gate (`./bin/check`): `npm ci`, `npm run
  check`, `npm run build`, fetch the pult sidecar, `cargo test`, then
  `cargo clippy --all-targets -- -D warnings`. This is exactly what `pult
  release` runs before it touches anything, and it's broader than CI
  (`ci.yml` splits frontend/rust into two parallel jobs and never runs
  `cargo test`).
- `pult test` — fast Rust test loop (`./bin/test`), forwards args to `cargo
  test`.
- `pult release` — picks the next patch/minor/major version (computed from
  the latest `v*` tag by `bin/release-candidates`), then `bin/release`
  bumps `package.json`, `src-tauri/tauri.conf.json`, and
  `src-tauri/Cargo.toml`/`Cargo.lock` together, runs the check gate, and
  commits/tags/pushes. Pushing the tag triggers
  `.github/workflows/release.yml`, which builds the four-platform installer
  matrix into a **draft** GitHub release — CI never publishes it for you;
  open https://github.com/lonic-software/pult-desktop/releases and publish
  it once you're happy with the build. All the safety checks (clean tree,
  on `main`, in sync with `origin/main`, tag doesn't already exist) run
  before anything is bumped, and `bin/release <version> --dry-run` runs the
  whole thing short of the commit/tag/push.
- `pult install` — also published as a module (`pult.module.yaml`), so
  anyone can install pult-desktop from the latest release without cloning:

  ```sh
  pult x github.com/lonic-software/pult-desktop install
  ```

  This dispatches (`bin/install`) to `install.sh` on macOS/Linux or
  `install.ps1` on Windows. Both resolve the latest *published* release via
  the GitHub API and error clearly if only a draft exists. The macOS path
  installs an unsigned app (no Apple Developer account yet), so it prints
  the Gatekeeper right-click-to-open workaround after installing.

## Next steps

- **pty runner** — a real terminal surface (portable-pty or similar) so
  `interactive: true` commands can run in-app instead of being refused.
- **`PULT_EVENTS` step ladder** — pult already exposes `steps` per command in
  `--list --json` and emits `step k/n <name>` / `progress` / `status` on the
  fd named by `$PULT_EVENTS` when nothing else claims it. The desktop app
  should claim that channel itself (pult passes it through untouched when
  already set — see docs/reference.md's Events protocol) and render a live
  step/percentage indicator instead of just raw output lines.
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
Tests skip (rather than fail) if no binary is found. `resolve_pick_source`
has no way to pass a per-call env override (unlike the other tests' local
`run_in_fixture` helper), since it goes through the same `run_capture` the
app uses; its test sets `PULT_TRUST_STORE` on the test process itself for
that reason, so it stays a single sequential `#[tokio::test]` rather than
several — see the comment on `resolve_pick_source_end_to_end`.

Mock-mode UI screenshots are the other half of manual verification — see
"Mock mode" above for the URL params used to script them. The current
canonical set (faceplate system, 1200×760 unless noted):

- `faceplate3-light.png` — `?mockstate=trusted&theme=light`
- `faceplate3-dark.png` — `?mockstate=trusted&theme=dark`
- `faceplate3-untrusted.png` — `?mockstate=untrusted&theme=light` — the
  all-dark board (nothing lit, no glow anywhere)
- `faceplate3-tooltip.png` — `?mockstate=trusted&theme=light&tooltip=import` —
  the "import" card's overflowing description tooltip open
- `faceplate2-running.png` — `?mockstate=trusted&theme=light&run=<command-id>`,
  screenshotted shortly after load so the run is still in flight
- `faceplate2-narrow.png` — same as light, at 760px wide
