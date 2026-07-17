// VITE_MOCK=1 stand-in for the real rack persistence (../real/rackStore.ts)
// — in-memory only, same as mock/paramStore.ts: a page reload starts with an
// empty rack, which is exactly what the `?mockstate=…` screenshot flows
// expect (they mount the fixture repo themselves via the rack's Mount
// device button path).

import type { RackState } from "../types";

let state: RackState = { devices: [], activePath: null };

export async function loadRack(): Promise<RackState> {
  return { devices: [...state.devices], activePath: state.activePath };
}

export async function saveRack(next: RackState): Promise<void> {
  state = { devices: [...next.devices], activePath: next.activePath };
}
