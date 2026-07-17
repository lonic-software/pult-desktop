// VITE_MOCK=1 stand-in for real per-repo param persistence (see
// ../real/paramStore.ts) — an in-memory nested map keyed the same way, since
// there's no meaningful "restart the app" story in a plain browser tab (a
// page reload loses this, same as every other piece of mock state). Never
// receives secret values — that's enforced by the caller (RunView never
// calls `saveParamValue` for a `param.secret` field), not re-checked here,
// same contract the real implementation relies on.

const store = new Map<string, Record<string, string>>();

function key(repoPath: string, commandId: string): string {
  return `${repoPath}\0${commandId}`;
}

export async function loadParamValues(
  repoPath: string,
  commandId: string,
): Promise<Record<string, string>> {
  return { ...(store.get(key(repoPath, commandId)) ?? {}) };
}

export async function saveParamValue(
  repoPath: string,
  commandId: string,
  paramName: string,
  value: string,
): Promise<void> {
  const k = key(repoPath, commandId);
  const existing = store.get(k) ?? {};
  store.set(k, { ...existing, [paramName]: value });
}
