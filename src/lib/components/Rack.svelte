<script lang="ts">
  import type { RackDevice } from "../types";

  interface Props {
    devices: RackDevice[];
    /** Path of the currently-open device (repo), or null when none is. */
    activePath: string | null;
    /** Devices with at least one in-flight run — their lamp burns amber so
     *  a run left streaming in a background device stays visible from the
     *  rack (runs survive switching; see +page.svelte's per-repo maps). */
    runningPaths: ReadonlySet<string>;
    collapsed: boolean;
    onSelect: (path: string) => void;
    onMountDevice: () => void;
    onEject: (path: string) => void;
    onToggleCollapsed: () => void;
  }

  let {
    devices,
    activePath,
    runningPaths,
    collapsed,
    onSelect,
    onMountDevice,
    onEject,
    onToggleCollapsed,
  }: Props = $props();

  // Purely decorative repetition (mounting holes / empty bays) — rendered
  // in fixed excess and clipped by their overflow:hidden containers, same
  // trick the design template uses, so nothing needs to measure the
  // sidebar's height.
  const RAIL_HOLES = Array.from({ length: 12 }, (_, i) => i);
  const EMPTY_SLOTS = Array.from({ length: 10 }, (_, i) => i);

  function bayLabel(n: number): string {
    return `BAY ${String(n).padStart(2, "0")}`;
  }

  // Lamp priority: a broken/untrusted device always shows red; otherwise a
  // background run burns amber (the active device's own runs already show on
  // its board, so its lamp stays green); active is green; idle is unlit.
  function lampFor(d: RackDevice): "green" | "amber" | "red" | "off" {
    if (d.status === "untrusted" || d.status === "error") return "red";
    if (runningPaths.has(d.path) && d.path !== activePath) return "amber";
    if (d.path === activePath) return "green";
    return "off";
  }

  function metaFor(d: RackDevice): { text: string; tone: "muted" | "red" | "amber" } | null {
    if (runningPaths.has(d.path)) return { text: "running…", tone: "amber" };
    switch (d.status) {
      case "ok":
        return d.instruments === null
          ? null
          : { text: `${d.instruments} instrument${d.instruments === 1 ? "" : "s"}`, tone: "muted" };
      case "untrusted":
        return { text: "not trusted", tone: "red" };
      case "error":
        return { text: "unavailable", tone: "red" };
      default:
        return null;
    }
  }
</script>

{#if collapsed}
  <aside class="rack-collapsed">
    <button type="button" class="expand micro" title="Open rack" onclick={onToggleCollapsed}
      >▸</button
    >
    <span class="vertical-label">Rack</span>
    <div class="rail rail-collapsed">
      {#each RAIL_HOLES as h (h)}
        <span class="hole" aria-hidden="true"></span>
      {/each}
    </div>
  </aside>
{:else}
  <aside class="rack">
    <div class="rack-header">
      <span class="screw" aria-hidden="true"></span>
      <span class="rack-title">Rack</span>
      <span class="screw" aria-hidden="true"></span>
      <button type="button" class="collapse micro" title="Collapse rack" onclick={onToggleCollapsed}
        >◂</button
      >
    </div>

    <div class="rack-body">
      <div class="rail">
        {#each RAIL_HOLES as h (h)}
          <span class="hole" aria-hidden="true"></span>
        {/each}
      </div>

      <div class="slots">
        {#each devices as device (device.path)}
          {@const active = device.path === activePath}
          {@const lamp = lampFor(device)}
          {@const meta = metaFor(device)}
          <div class="device-slot">
            <button
              type="button"
              class="device micro"
              class:active
              aria-current={active ? "true" : undefined}
              onclick={() => onSelect(device.path)}
            >
              <span class="screw screw-tr" aria-hidden="true"></span>
              <span class="screw screw-br" aria-hidden="true"></span>
              <span class="device-name-row">
                <span class="lamp lamp-{lamp}" aria-hidden="true"></span>
                <span class="device-name">{device.name}</span>
              </span>
              <span class="device-path mono">{device.path}</span>
              {#if meta}
                <span class="device-meta meta-{meta.tone}">{meta.text}</span>
              {/if}
            </button>
            <button
              type="button"
              class="eject micro"
              title="Eject device"
              aria-label="Eject {device.name}"
              onclick={() => onEject(device.path)}>⏏</button
            >
          </div>
        {/each}

        <button type="button" class="mount micro" onclick={onMountDevice}>
          <span class="well-hole hole-tl" aria-hidden="true"></span>
          <span class="well-hole hole-tr" aria-hidden="true"></span>
          <span class="well-hole hole-bl" aria-hidden="true"></span>
          <span class="well-hole hole-br" aria-hidden="true"></span>
          <span class="mount-plus" aria-hidden="true">＋</span>
          <span class="mount-label">Mount device</span>
        </button>

        <div class="empty-bays" aria-hidden="true">
          {#each EMPTY_SLOTS as i (i)}
            <div class="bay">
              <span class="well-hole hole-tl"></span>
              <span class="well-hole hole-tr"></span>
              <span class="well-hole hole-bl"></span>
              <span class="well-hole hole-br"></span>
              <div class="bay-slits">
                <span></span>
                <span></span>
                <span></span>
              </div>
              <span class="bay-label">{bayLabel(devices.length + i + 2)}</span>
            </div>
          {/each}
        </div>
      </div>
    </div>
  </aside>
{/if}

<style>
  .rack,
  .rack-collapsed {
    position: relative; /* containing block for `.rack::after`'s shine-back
    below */
    flex: none;
    display: flex;
    flex-direction: column;
    background: color-mix(in srgb, var(--bg) 72%, var(--panel));
    border-right: 1px solid var(--line);
    box-shadow: inset -1px 0 0 var(--emboss-light);
  }

  .rack {
    width: 216px;
  }

  .rack-collapsed {
    width: 34px;
    align-items: center;
  }

  /* Sidebar shine-back (visual-polish addendum, mirrors crt.css's
     `.pult-crt::before` screen reflection almost verbatim — same
     transitioned-`background-color`-under-a-fixed-mask technique, same
     white-fading-to-transparent mask gotcha, see that file's comment for
     the full "why white, not black" rationale). Exists because the details
     page's tower glow can't reach this sidebar any other way: `.board` is
     `overflow-y: auto` and `.run-view` is `overflow: hidden`, so nothing
     painted by either content-pane view (including Meter's/Tower's own
     ambient washes) can ever bleed across into this sibling element — the
     wash has to be re-created here from the same two vars instead, exactly
     like the CRT screens already do.
     `--meter-glow-color`/`--meter-glow-level` come from +page.svelte's
     `.body` (RunView forwards its own `meterGlow` up there via
     `onGlowChange` — see +page.svelte's `bodyGlow` for why it isn't
     recomputed here), NOT from this component — Rack.svelte has no view of
     the tower at all. Unset/zero (the board view, and this component's
     initial render before any run view has ever mounted) collapses this to
     fully transparent, so the board page — deliberately, see +page.svelte's
     `bodyGlow` comment — never shows any of this.
     Enters from the RIGHT edge (the edge facing the content pane/tower, the
     mirror image of the CRT reflection's left-edge entry) and reaches a
     fixed ~72px into the sidebar regardless of `.rack`'s own width — a
     length-unit gradient stop (not a percentage one) is what makes that
     reach fixed rather than proportional to the box. Noticeably dimmer
     ceiling (0.14 vs the CRT glass's 0.22) since this is spill light doubly
     removed from the source (through the tower's wash, reflected again off
     a sidebar it never directly touches) — it should read as a faint
     awareness of the glow next door, not a second version of it. */
  .rack::after,
  .rack-collapsed::after {
    content: "";
    position: absolute;
    inset: 0;
    z-index: 1;
    pointer-events: none;
    background-color: var(--meter-glow-color, transparent);
    opacity: calc(var(--meter-glow-level, 0) * 0.14);
    mask-image: linear-gradient(to left, white, transparent 72px);
    -webkit-mask-image: linear-gradient(to left, white, transparent 72px);
    transition: background-color 420ms ease, opacity 420ms ease;
  }

  .expand {
    flex: none;
    width: 100%;
    height: 46px;
    display: flex;
    align-items: center;
    justify-content: center;
    border: none;
    border-bottom: 1px solid var(--line);
    background: var(--panel);
    color: var(--muted);
    font-size: 11px;
    box-shadow: inset 0 1px 0 var(--emboss-light);
  }

  .expand:hover {
    color: var(--ink);
  }

  .vertical-label {
    margin-top: 14px;
    font-family: var(--font-mono);
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.3em;
    text-transform: uppercase;
    color: var(--muted);
    writing-mode: vertical-rl;
    text-shadow: 0 1px 0 var(--emboss-light);
  }

  .rack-header {
    position: relative;
    height: 46px;
    flex: none;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    border-bottom: 1px solid var(--line);
    background: var(--panel);
    box-shadow: inset 0 1px 0 var(--emboss-light);
  }

  .rack-title {
    font-family: var(--font-mono);
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.3em;
    text-transform: uppercase;
    color: var(--ink);
    text-shadow: 0 1px 0 var(--emboss-light);
  }

  .rack-header > .screw {
    position: static;
  }

  .collapse {
    position: absolute;
    right: 8px;
    top: 50%;
    transform: translateY(-50%);
    width: 22px;
    height: 22px;
    display: flex;
    align-items: center;
    justify-content: center;
    border: 1px solid var(--line);
    border-radius: 4px;
    background: var(--panel);
    color: var(--muted);
    font-size: 10px;
    box-shadow:
      inset 0 1px 0 var(--emboss-light),
      0 1px 2px rgba(0, 0, 0, 0.18);
  }

  .collapse:hover {
    color: var(--ink);
  }

  .rack-body {
    flex: 1;
    display: flex;
    min-height: 0;
  }

  /* Mounting rail: hairline rungs every 24px with recessed square holes —
     the "you could bolt a unit in here" texture from the design. */
  .rail {
    flex: none;
    width: 14px;
    border-right: 1px solid var(--line);
    background: repeating-linear-gradient(180deg, transparent 0 24px, var(--line) 24px 25px);
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 56px;
    padding-top: 26px;
    overflow: hidden;
  }

  .rail-collapsed {
    flex: 1;
    border-right: none;
    margin: 14px 0 10px;
    padding-top: 10px;
  }

  .hole {
    flex: none;
    width: 6px;
    height: 6px;
    border-radius: 2px;
    background: var(--led-well);
    box-shadow:
      inset 0 1px 2px rgba(0, 0, 0, 0.7),
      0 1px 0 var(--emboss-light);
  }

  .slots {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 12px 12px 12px 10px;
    overflow-y: auto;
  }

  .screw {
    position: absolute;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--screw);
    box-shadow:
      inset 0 1px 1.5px rgba(0, 0, 0, 0.5),
      0 1px 0 var(--emboss-light);
  }

  .screw-tr {
    top: 7px;
    right: 7px;
  }

  .screw-br {
    bottom: 7px;
    right: 7px;
  }

  .device-slot {
    position: relative;
    flex: none;
  }

  .device {
    position: relative;
    width: 100%;
    display: flex;
    flex-direction: column;
    gap: 5px;
    text-align: left;
    padding: 11px 12px 10px;
    background: var(--pad);
    border: 1px solid var(--line);
    border-radius: var(--radius-control);
    color: var(--ink);
    font-family: inherit;
    box-shadow:
      inset 0 1px 0 var(--emboss-light),
      0 1px 3px rgba(0, 0, 0, 0.28);
  }

  .device:hover {
    transform: translateY(1px);
    box-shadow:
      inset 0 1px 0 var(--emboss-light),
      0 1px 1px rgba(0, 0, 0, 0.2);
  }

  .device.active {
    background: var(--panel);
    border-color: var(--accent);
  }

  .device-name-row {
    display: flex;
    align-items: center;
    gap: 7px;
    min-width: 0;
  }

  .lamp {
    flex: none;
    width: 7px;
    height: 7px;
    border-radius: 50%;
  }

  .lamp-off {
    background: var(--seg-off);
    box-shadow: inset 0 1px 2px rgba(0, 0, 0, 0.4);
  }

  .lamp-green {
    background: var(--lamp-green);
    box-shadow:
      0 0 8px -1px color-mix(in srgb, var(--lamp-green) 70%, transparent),
      inset 0 1px 1px rgba(255, 255, 255, 0.35);
  }

  .lamp-amber {
    background: var(--accent);
    box-shadow:
      0 0 8px -1px color-mix(in srgb, var(--accent) 70%, transparent),
      inset 0 1px 1px rgba(255, 255, 255, 0.35);
  }

  .lamp-red {
    background: var(--lamp-red);
    box-shadow:
      0 0 8px -1px color-mix(in srgb, var(--lamp-red) 65%, transparent),
      inset 0 1px 1px rgba(255, 255, 255, 0.35);
  }

  .device-name {
    font-size: 13.5px;
    font-weight: 600;
    letter-spacing: -0.01em;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .device-path {
    font-size: 10.5px;
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    padding-right: 10px;
  }

  .device-meta {
    font-family: var(--font-mono);
    font-size: 10px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .meta-muted {
    color: var(--muted);
  }

  .meta-red {
    color: var(--lamp-red);
  }

  .meta-amber {
    color: var(--accent);
  }

  /* Eject sits where the top-right screw is and swaps in on hover — a
     nested control can't live inside the device <button>, so it's an
     absolutely-positioned sibling. */
  .eject {
    position: absolute;
    top: 3px;
    right: 3px;
    width: 16px;
    height: 16px;
    display: flex;
    align-items: center;
    justify-content: center;
    border: none;
    border-radius: 3px;
    background: transparent;
    color: var(--muted);
    font-size: 9px;
    opacity: 0;
  }

  .device-slot:hover .eject,
  .eject:focus-visible {
    opacity: 1;
  }

  .device-slot:hover .screw-tr {
    opacity: 0;
  }

  .eject:hover {
    color: var(--lamp-red);
  }

  /* The add-repo affordance: a recessed, empty mounting bay. */
  .mount {
    position: relative;
    flex: none;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-1);
    height: 74px;
    background: color-mix(in srgb, var(--well-inset, rgba(0, 0, 0, 0.4)) 25%, transparent);
    border: 1px solid var(--line);
    border-radius: var(--radius-control);
    color: var(--muted);
    font-family: inherit;
    box-shadow:
      inset 0 2px 4px var(--well-inset, rgba(0, 0, 0, 0.3)),
      0 1px 0 var(--emboss-light);
  }

  .mount:hover {
    color: var(--ink);
  }

  .mount-plus {
    font-size: 16px;
    line-height: 1;
  }

  .mount-label {
    font-family: var(--font-mono);
    font-size: 10px;
    letter-spacing: 0.18em;
    text-transform: uppercase;
  }

  .well-hole {
    position: absolute;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--led-well);
    box-shadow:
      inset 0 1px 2px rgba(0, 0, 0, 0.8),
      0 1px 0 var(--emboss-light);
  }

  .hole-tl {
    top: 7px;
    left: 7px;
  }

  .hole-tr {
    top: 7px;
    right: 7px;
  }

  .hole-bl {
    bottom: 7px;
    left: 7px;
  }

  .hole-br {
    bottom: 7px;
    right: 7px;
  }

  /* Unpopulated bays below the mount slot — pure texture, clipped by the
     overflow so they fill whatever vertical space is left. */
  .empty-bays {
    flex: 1 0 0;
    min-height: 0;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .bay {
    position: relative;
    flex: none;
    height: 74px;
    border: 1px solid var(--line);
    border-radius: var(--radius-control);
    background: color-mix(in srgb, var(--well-inset, rgba(0, 0, 0, 0.4)) 25%, transparent);
    box-shadow:
      inset 0 2px 4px var(--well-inset, rgba(0, 0, 0, 0.3)),
      0 1px 0 var(--emboss-light);
    opacity: 0.8;
  }

  .bay-slits {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 5px;
  }

  .bay-slits span {
    width: 64px;
    height: 2px;
    border-radius: 1px;
    background: var(--led-well);
    box-shadow:
      inset 0 1px 1px rgba(0, 0, 0, 0.8),
      0 1px 0 var(--emboss-light);
  }

  .bay-label {
    position: absolute;
    bottom: 6px;
    left: 50%;
    transform: translateX(-50%);
    font-family: var(--font-mono);
    font-size: 8.5px;
    letter-spacing: 0.2em;
    color: var(--muted);
    opacity: 0.55;
    text-shadow: 0 1px 0 var(--emboss-light);
  }
</style>
