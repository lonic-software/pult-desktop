// Per-repo, per-command param-value persistence — backs the parameters
// module's "remembered per repo" promise (see RunView). One shared JSON
// store file via tauri-plugin-store (already a dependency and already
// registered in src-tauri/src/lib.rs — `.plugin(tauri_plugin_store::Builder
// ::new().build())` — this is simply its first real use), keyed by
// `<repoPath>\0<commandId>` so entries for different repos/commands
// never collide; each entry is a flat `{ paramName: value }` object so one
// command's worth of params reads/writes as a single store operation.
//
// Secret params (Param.secret) are never passed to `saveParamValue` — the
// caller (RunView) filters them out before calling, same contract the mock
// implementation (../mock/paramStore.ts) relies on; this file doesn't
// re-check `secret` itself since it only ever sees a `paramName`/`value`
// pair, not the full `Param` shape.

import { load, type Store } from "@tauri-apps/plugin-store";

const STORE_FILE = "param-values.json";
let storePromise: Promise<Store> | null = null;

function getStore(): Promise<Store> {
  // Lazily opened once per app session and reused — `load` with the same
  // file name returns the same underlying store instance, but there's no
  // reason to re-resolve that on every read/write. No `options` passed
  // (tauri-plugin-store's `StoreOptions.defaults` is mandatory in its own
  // types when an options object is passed at all, and this store has no
  // meaningful defaults to seed — a missing key already reads back as
  // `undefined`, which every caller here already treats as "nothing saved
  // yet") — that also leaves the plugin's own default ~100ms autosave
  // debounce in place, so the explicit `store.save()` after each write in
  // `saveParamValue` below is a cheap, idempotent belt-and-suspenders call,
  // not load-bearing.
  if (!storePromise) storePromise = load(STORE_FILE);
  return storePromise;
}

function key(repoPath: string, commandId: string): string {
  return `${repoPath}\0${commandId}`;
}

export async function loadParamValues(
  repoPath: string,
  commandId: string,
): Promise<Record<string, string>> {
  const store = await getStore();
  return (await store.get<Record<string, string>>(key(repoPath, commandId))) ?? {};
}

export async function saveParamValue(
  repoPath: string,
  commandId: string,
  paramName: string,
  value: string,
): Promise<void> {
  const store = await getStore();
  const k = key(repoPath, commandId);
  const existing = (await store.get<Record<string, string>>(k)) ?? {};
  await store.set(k, { ...existing, [paramName]: value });
  await store.save();
}
