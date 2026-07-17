# Design language — lamps, gauges, and the meter

The app's UI is a hardware faceplate: modules, pads, engraved labels, LED
instruments. This doc pins down what those instruments *mean*, so every
surface (board card, details page, anything future) renders the same physics.
When a new surface needs a status element, check it against these rules
instead of inventing a variant.

## The meter is one instrument

Every vertical LED column in the app — the small board meter, the large
details-page meter — is the **same instrument**: the command's condition
lamp and gauge. It never means different things on different pages.

One rule per dimension:

- **Color = what.** Green is good/ready, red is bad/failed, amber is
  activity, gray is neutral (powered, no probe).
- **Level = how much.** Levels are *fractions of the column*, not segment
  counts, so meters of any size stay in the same language: readiness
  standing levels are no-check 20%, running floor 60%, ready 80%,
  check-failed 100%. While a run reports progress, the level is the
  progress fraction.
- **Levels slew.** A level never jumps, in either direction. Any change to
  what's lit — ready dropping to running's starting level while it turns
  amber, a progress jump from 33% to 62%, a run ending and the meter
  recovering to readiness, a doctor refresh, no-check swapping with ready,
  even the very first mount — animates segment by segment through every
  intermediate value, needle-physics style, not a cross-fade or a pop. Color
  changes ride along for free: each segment's own color transition fires as
  it lights or dims mid-climb, so "climb down through green while turning
  amber" falls out of the level chase plus the color transition, not a
  bespoke effect. Reduced motion may snap straight to the target instead of
  chasing it.
- **Solid = state.** A steady lamp is a standing condition (ready, check
  failing, no probe). It stays as long as the condition does.
- **Blink = event mode.** A blink isn't ambient texture and it isn't a
  partial state — it's a mode the whole column enters: the ambient flicker
  suspends entirely, and every segment alternates together between fully dim
  and fully lit in the event color, sharp on/off, no fade. Only three things
  ever blink — success, a failed run, and a stop — and only failure latches
  (see below).

## State table

| Meter shows | Pattern | Meaning |
|---|---|---|
| Solid green (80%) | steady | Ready — checks pass |
| Solid red (100%) | steady | Check failing (doctor says so; stays while true) |
| Solid gray (20%) | steady | Powered, no `check:` probe |
| Dark | steady | Untrusted / no answer yet |
| Amber, level climbing | slewing level | Running, determinate progress |
| Amber 60% + sweep strip | steady + sweep | Running, no progress data (indeterminate) |
| A few full-column green blinks | ~1-1.2s transient | Run succeeded — self-clears, then slews back to readiness |
| **Full-column red blink** | latched (board) / a few blinks (details page) | **Run failed** — unacknowledged on the board; on the command's own page, being there already counts as seen |
| Brief full-column amber blink | pre-acknowledged, brief | User stopped the run — calm, not an alarm |

## Only failures latch

Success and failure are not symmetric events:

- **Success informs.** A finished run blinks the column green a few times,
  then slews back to solid readiness. There is never a *standing* green
  that means "succeeded" — solid green always, only, means ready. If you
  missed the blink: no news is good news.
- **Failure demands acknowledgment.** On the board, a failed run latches the
  meter blinking red until the user has *seen* it: opening (or being on)
  that command's details page, or starting a new run. Nothing else clears
  it. The latch is per command, session-scoped. On the command's own
  details page, a failure isn't latched — it gets a few (slightly more
  insistent) red blinks and then slews back to readiness, because being on
  that page while it happens *is* the acknowledgment.
- **A user stop is pre-acknowledged.** The user caused it; a brief, calm
  amber blink, then straight back to readiness. No latch, and it must never
  read as an alarm.

The run's outcome doesn't vanish once the blink ends — it keeps living in
words: the details page's last-run line ("finished/stopped HH:MM:SS" for
this visit, "last run Nm ago · outcome" otherwise), the output pane's ✓/✗
summary line, and the stage cards. Only the *lamp* stops standing; the
record of what happened stays legible.

Blink is a sharper, full-column cadence than the ambient analog flicker
(which only ever touches the tip segment); the two must never be
confusable. Flicker is texture; blink is signal.

## Lamps vs gauges vs words

- **Lamps signal** (the stage ladder's per-step dots: done/active/failed/
  pending).
- **Gauges measure** (the meter's level; there is exactly one gauge per
  quantity on a page — never two elements showing the same number).
- **Words engrave** (the status word under a meter always names what the
  meter currently shows; run-state words live with run instruments).

The stage ladder is not redundant with the meter: the meter is continuous
("how much"), stages are discrete milestones ("which part").

## Screens

The details page's three internal surfaces — Parameters, Stages, Output —
are **screens**, not panels: CRT phosphor tubes bolted into the faceplate,
distinct from the pads/wells/engraved labels the rest of the app is built
from (see `crt.css` for the shared `.pult-crt`/`.pult-screen` vocabulary and
`docs/../src/lib/components/RunView.svelte`'s file comment for how the three
modules use it).

- **The palette is fixed, not themed.** A physical tube doesn't relight
  itself when the room does — the phosphor ink, dims, and hues (`--crt-*`
  custom properties, scoped under `.pult-crt`) are literal values that don't
  change between light and dark app theme, the one deliberate exception to
  every other surface in this doc being theme-token-driven.
- **Scan lines are fixed to the bezel, not the content.** Each screen is two
  layers: a non-scrolling `.pult-crt` wrapper carrying the scan-line/sheen/
  vignette overlay, and a `.pult-screen` child that actually scrolls.
  Dragging content past the glass must never drag the glass's own texture
  with it — that would break the illusion of a tube with paper behind it
  instead of one continuous scrolling graphic.
- **Screens scroll; the rack never does.** Same rule as the page itself
  (see the file-level comment at the top of `RunView.svelte`): the details
  page is a fixed rack that never document-scrolls, and each screen caps
  itself and scrolls internally as the backstop for whatever doesn't fit —
  params vertically (with a short 168px cap and a "scroll ↕" hint once it
  actually overflows), stages horizontally (a card strip, "scroll ↔"),
  output vertically, filling whatever room the module has. The hints are
  conditional on real overflow, not always-on chrome — an instrument that
  claims more content than it has is worse than one that says nothing.

Screens don't replace lamps/gauges/words above — a stage card inside the
STAGES screen still carries the same lamp semantics (done/active/failed/
pending) as the rest of the app, just recolored to the phosphor hues; only
the glass they sit behind is different.

## Analog liveness

Every lit LED element carries the analog texture system (fast shimmer,
slow level-wobble events, mount climb — see `Meter.svelte`'s comments for
the mechanics) whenever it isn't blinking (see "Blink = event mode" above —
a blink suspends this entirely, it never layers on top). Liveness applies
uniformly: an instrument that stops flickering stops looking powered.
Reduced-motion collapses all of it to clean steady lamps.
