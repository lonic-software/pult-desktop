// Persisted rack state (design 4a) — the list of mounted devices plus which
// one was active, so a launch restores the rack instead of starting from an
// empty "open a repository" screen. Same tauri-plugin-store mechanism as
// paramStore.ts (see there for the load/options/save notes), its own file so
// rack edits and param writes never contend on one store.

import { load, type Store } from "@tauri-apps/plugin-store";
import type { RackDevice, RackState } from "../types";

const STORE_FILE = "rack.json";
const KEY = "rack";
let storePromise: Promise<Store> | null = null;

function getStore(): Promise<Store> {
  if (!storePromise) storePromise = load(STORE_FILE);
  return storePromise;
}

export async function loadRack(): Promise<RackState> {
  const store = await getStore();
  const saved = await store.get<RackState>(KEY);
  if (!saved) return { devices: [], activePath: null };
  // Defensive shape check — the file is user-editable on disk, and a
  // malformed entry here would otherwise wedge the app at startup.
  const devices = Array.isArray(saved.devices)
    ? saved.devices.filter((d): d is RackDevice => typeof d?.path === "string")
    : [];
  const activePath =
    typeof saved.activePath === "string" && devices.some((d) => d.path === saved.activePath)
      ? saved.activePath
      : null;
  return { devices, activePath };
}

export async function saveRack(state: RackState): Promise<void> {
  const store = await getStore();
  await store.set(KEY, state);
  await store.save();
}
