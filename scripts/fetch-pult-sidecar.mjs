#!/usr/bin/env node
// Fetches the pinned `pult` release binary for one target triple and places
// it at src-tauri/binaries/pult-<target-triple>[.exe] — the naming
// convention Tauri's `bundle.externalBin` expects (see tauri.conf.json).
// Runs on every `cargo build`/`check`/`test`/`clippy` of the src-tauri
// crate, not just `tauri build`: once `externalBin` is configured, Tauri's
// build script validates the resource exists for *any* compile, dev or
// release. Wired in via `beforeBuildCommand`/`beforeDevCommand` locally and
// a CI step where cargo runs directly (see README's "Sidecar bundling").
//
// Version + per-target checksums are pinned in src-tauri/sidecar.json —
// bumping the bundled pult means editing that one file. This script
// verifies every download against those pinned checksums and hard-fails on
// a mismatch; it never trusts the release's own checksums.txt (that only
// guards against corruption, not a compromised upload).
//
// Usage: node scripts/fetch-pult-sidecar.mjs [target-triple]
// Falls back to $TAURI_ENV_TARGET_TRIPLE, then host detection.

import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync, copyFileSync, chmodSync, rmSync, readdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, '..');
const manifestPath = path.join(repoRoot, 'src-tauri', 'sidecar.json');
const binariesDir = path.join(repoRoot, 'src-tauri', 'binaries');
const cacheDir = path.join(binariesDir, '.cache');

function detectHostTriple() {
  const { platform, arch } = process;
  const table = {
    'darwin:arm64': 'aarch64-apple-darwin',
    'darwin:x64': 'x86_64-apple-darwin',
    'win32:x64': 'x86_64-pc-windows-msvc',
    'linux:x64': 'x86_64-unknown-linux-gnu',
    'linux:arm64': 'aarch64-unknown-linux-gnu',
  };
  const key = `${platform}:${arch}`;
  const triple = table[key];
  if (!triple) {
    fail(
      `Can't detect a pult sidecar target for host platform "${platform}"/"${arch}". ` +
        `Pass a target triple explicitly: node scripts/fetch-pult-sidecar.mjs <target-triple>`
    );
  }
  return triple;
}

function fail(message) {
  console.error(`fetch-pult-sidecar: ${message}`);
  process.exit(1);
}

function sha256File(filePath) {
  return createHash('sha256').update(readFileSync(filePath)).digest('hex');
}

async function downloadToFile(url, destPath) {
  const res = await fetch(url);
  if (!res.ok) {
    fail(`Download failed (${res.status} ${res.statusText}): ${url}`);
  }
  const buf = Buffer.from(await res.arrayBuffer());
  writeFileSync(destPath, buf);
}

function extractBinary(archivePath, assetName, outputPath) {
  const extractDir = path.join(tmpdir(), `pult-sidecar-extract-${Date.now()}-${Math.random().toString(36).slice(2)}`);
  mkdirSync(extractDir, { recursive: true });
  try {
    if (assetName.endsWith('.tar.gz')) {
      execFileSync('tar', ['xzf', archivePath, '-C', extractDir], { stdio: 'inherit' });
    } else if (assetName.endsWith('.zip')) {
      try {
        execFileSync('unzip', ['-o', archivePath, '-d', extractDir], { stdio: 'inherit' });
      } catch (err) {
        if (err.code !== 'ENOENT') throw err;
        // No `unzip` on this host (e.g. stock Windows) — bsdtar (Windows's
        // built-in `tar.exe` since Win10 1803, also what macOS ships) reads
        // zip archives too.
        execFileSync('tar', ['xf', archivePath, '-C', extractDir], { stdio: 'inherit' });
      }
    } else {
      fail(`Don't know how to extract "${assetName}" — expected a .tar.gz or .zip asset`);
    }

    const entries = readdirSync(extractDir);
    const binaryName = entries.find((e) => e === 'pult' || e === 'pult.exe');
    if (!binaryName) {
      fail(
        `Expected a "pult" or "pult.exe" file in ${assetName}, found: ${entries.join(', ') || '(empty archive)'}`
      );
    }
    copyFileSync(path.join(extractDir, binaryName), outputPath);
    chmodSync(outputPath, 0o755);
  } finally {
    rmSync(extractDir, { recursive: true, force: true });
  }
}

async function main() {
  const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
  const triple = process.argv[2] || process.env.TAURI_ENV_TARGET_TRIPLE || detectHostTriple();

  const entry = manifest.assets[triple];
  if (!entry) {
    const supported = Object.keys(manifest.assets).join(', ');
    fail(`No pinned pult release asset for target triple "${triple}". Supported: ${supported}`);
  }

  const outputName = triple.includes('windows') ? `pult-${triple}.exe` : `pult-${triple}`;
  const outputPath = path.join(binariesDir, outputName);
  const stampPath = `${outputPath}.sha256`;
  const stamp = `${manifest.version} ${entry.sha256}`;

  if (existsSync(outputPath) && existsSync(stampPath) && readFileSync(stampPath, 'utf8').trim() === stamp) {
    console.log(`fetch-pult-sidecar: ${outputName} already up to date (pult ${manifest.version}), skipping.`);
    return;
  }

  mkdirSync(binariesDir, { recursive: true });
  mkdirSync(cacheDir, { recursive: true });

  const cachedArchivePath = path.join(cacheDir, `${entry.sha256}-${entry.asset}`);
  if (existsSync(cachedArchivePath) && sha256File(cachedArchivePath) === entry.sha256) {
    console.log(`fetch-pult-sidecar: using cached download for ${entry.asset}.`);
  } else {
    const url = `https://github.com/${manifest.repo}/releases/download/v${manifest.version}/${entry.asset}`;
    console.log(`fetch-pult-sidecar: downloading ${url}`);
    const tmpPath = `${cachedArchivePath}.part`;
    await downloadToFile(url, tmpPath);
    const actualSha256 = sha256File(tmpPath);
    if (actualSha256 !== entry.sha256) {
      rmSync(tmpPath, { force: true });
      fail(
        `Checksum mismatch for ${entry.asset}: expected ${entry.sha256}, got ${actualSha256}. ` +
          `Refusing to use this download — the pinned checksum in src-tauri/sidecar.json didn't match.`
      );
    }
    // Rename only after the checksum is confirmed good, so a half-verified
    // file never lands at the cache's canonical (trusted-by-presence) path.
    copyFileSync(tmpPath, cachedArchivePath);
    rmSync(tmpPath, { force: true });
  }

  extractBinary(cachedArchivePath, entry.asset, outputPath);
  writeFileSync(stampPath, `${stamp}\n`);
  console.log(`fetch-pult-sidecar: wrote ${path.relative(repoRoot, outputPath)} (pult ${manifest.version}, verified sha256).`);
}

main().catch((err) => fail(err.stack || String(err)));
