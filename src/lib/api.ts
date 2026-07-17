// Single entry point the UI imports. Picks the mock backend (VITE_MOCK=1,
// plain browser, fixture data — see src/lib/mock) or the real Tauri backend
// (src/lib/real) at build time, so nothing else in the app needs to branch.

export const isMock = import.meta.env.VITE_MOCK === "1";

export * from "./types";

import * as mockBackend from "./mock/backend";
import * as realBackend from "./real/backend";
import * as mockParamStore from "./mock/paramStore";
import * as realParamStore from "./real/paramStore";

const backend = isMock ? mockBackend : realBackend;
const paramStore = isMock ? mockParamStore : realParamStore;

export const pickFolder = backend.pickFolder;
export const openRepo = backend.openRepo;
export const trustRepo = backend.trustRepo;
export const doctorCheck = backend.doctorCheck;
export const pultVersion = backend.pultVersion;
export const getPultPath = backend.getPultPath;
export const setPultPath = backend.setPultPath;
export const runCommand = backend.runCommand;
export const stopRun = backend.stopRun;
export const resolvePickSource = backend.resolvePickSource;

// Per-repo param-value persistence (see ./real/paramStore.ts and
// ./mock/paramStore.ts) — the parameters module's "remembered per repo"
// promise. Never call `saveParamValue` for a `param.secret` field.
export const loadParamValues = paramStore.loadParamValues;
export const saveParamValue = paramStore.saveParamValue;
