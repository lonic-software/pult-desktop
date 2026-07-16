// Single entry point the UI imports. Picks the mock backend (VITE_MOCK=1,
// plain browser, fixture data — see src/lib/mock) or the real Tauri backend
// (src/lib/real) at build time, so nothing else in the app needs to branch.

export const isMock = import.meta.env.VITE_MOCK === "1";

export * from "./types";

import * as mockBackend from "./mock/backend";
import * as realBackend from "./real/backend";

const backend = isMock ? mockBackend : realBackend;

export const pickFolder = backend.pickFolder;
export const openRepo = backend.openRepo;
export const trustRepo = backend.trustRepo;
export const doctorCheck = backend.doctorCheck;
export const pultVersion = backend.pultVersion;
export const getPultPath = backend.getPultPath;
export const setPultPath = backend.setPultPath;
export const runCommand = backend.runCommand;
export const resolvePickSource = backend.resolvePickSource;
