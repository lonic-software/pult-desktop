// Fixture data for VITE_MOCK=1 — lets the UI be developed and screenshotted
// in a plain browser, no Tauri runtime needed. Shaped to exercise every v0
// surface: a secret param, a failing check, an interactive command, dynamic
// pick options, and starts untrusted so the trust modal flow is reachable.
// Most commands carry a realistic operator-voice `description` for the
// board card; "status" is deliberately left without one, to prove the
// title-only card looks intentional rather than like a bug. Titles follow
// pult's authoring convention (1-2 words; the description carries the
// explanation) — "import" carries a deliberately long (4-5 line) description
// to exercise the card's hover/focus tooltip for text the 3-line clamp
// can't fit.
//
// Two includes ("AWS Tooling", "Test Harness") plus a categorized local
// command give this listing 3 sources and at least one category — the
// least-nesting rule in grouping.ts engages, so the default mock board is
// the nested ("2a") shape: one panel per source, category sub-groups inside
// (Local: General/Deploy, AWS Tooling: Identity/Deploy, Test Harness:
// Test/Deploy — the same shape as the design reference's demo data). This
// also exercises categories NOT merging across sources in nested mode: two
// separate "Deploy" sub-groups, one under Local and one under Test Harness.

import type { DoctorReport, Listing } from "../types";

const AWS_ORIGIN = "github.com/opskit/aws-common@v1.4.2";
const TEST_ORIGIN = "github.com/opskit/test-harness@v0.9.0";

export const mockListingUntrusted: Listing = {
  schema: 1,
  pult_version: "0.4.0",
  name: "acme-ops",
  manifest: "/Users/operator/acme-ops/pult.yaml",
  dir: "/Users/operator/acme-ops",
  run_dir: "/Users/operator/acme-ops",
  scope: "repo",
  trusted: false,
  includes: [
    {
      source: AWS_ORIGIN,
      kind: "git",
      url: "https://github.com/opskit/aws-common",
      rev: "v1.4.2",
      rev_kind: "tag",
      resolved_sha: "8a6e6fd4e2c1f9b7a0d3c5e6f7081920a3b4c5d6",
      name: "AWS Tooling",
    },
    {
      source: TEST_ORIGIN,
      kind: "git",
      url: "https://github.com/opskit/test-harness",
      rev: "v0.9.0",
      rev_kind: "tag",
      resolved_sha: "3f1c2b9d8e7a6f5c4b3a2d1e0f9c8b7a6d5e4f3c",
      name: "Test Harness",
    },
  ],
  commands: [
    {
      id: "shell",
      title: "Shell",
      origin: null,
      category: null,
      description:
        "Drops you into an interactive shell inside the chosen environment, with this repo's tooling already on PATH.",
      params: [{ name: "env", kind: "pick", options: ["dev", "uat", "pre"] }],
      check: "command -v aws",
      interactive: true,
      steps: null,
    },
    {
      id: "status",
      title: "Status",
      origin: null,
      category: null,
      // Deliberately no description — proves the title-only card looks
      // intentional rather than like a missing-data bug.
      params: [],
      check: "command -v sh",
      interactive: false,
      steps: null,
    },
    {
      id: "import",
      title: "Import",
      origin: null,
      category: "Deploy",
      description:
        "Pulls the latest export from the vendor's reporting API, validates it against the local dataset's schema, and loads every changed record into place. Needs a fresh access token scoped to the export endpoint — expired or under-scoped tokens fail loudly rather than silently skipping rows.",
      params: [
        { name: "token", kind: "input", default: null, secret: true },
        { name: "note", kind: "input", default: "", secret: false },
      ],
      check: "command -v this-tool-does-not-exist",
      interactive: false,
      steps: null,
    },
    {
      id: "aws:whoami",
      title: "Identity",
      origin: AWS_ORIGIN,
      category: "Identity",
      description: "Prints the AWS identity pult will run subsequent commands as.",
      params: [],
      check: "command -v aws",
      interactive: false,
      steps: null,
    },
    {
      id: "aws:deploy",
      title: "Deploy stack",
      origin: AWS_ORIGIN,
      category: "Deploy",
      description:
        "Builds, pushes, and releases the stack to the chosen region. Safe to re-run — steps report progress as they complete. Declares more steps than the mock script emits events for (see mock/backend.ts's aws:deploy script) on purpose — deriveStages marks every step 'done' on a successful exit regardless of whether its own event arrived (see stages.ts), and the extra steps exercise the STAGES screen's horizontal scroll at a realistic-ish count.",
      params: [
        { name: "region", kind: "pick", options: ["eu-west-1", "us-east-1"] },
        {
          name: "customer",
          kind: "pick",
          source: "./bin/impl list --env {region}",
          depends_on: ["region"],
        },
      ],
      check: null,
      interactive: false,
      steps: ["build", "push", "release", "migrate", "smoke", "canary", "promote"],
    },
    {
      id: "test:smoke",
      title: "Smoke test",
      origin: TEST_ORIGIN,
      category: "Test",
      description: "Runs the fast smoke suite against the currently-selected environment.",
      params: [{ name: "env", kind: "pick", options: ["dev", "uat", "pre"] }],
      check: "command -v pytest",
      interactive: false,
      steps: null,
    },
    {
      id: "test:deploy",
      title: "Deploy fixtures",
      origin: TEST_ORIGIN,
      category: "Deploy",
      description:
        "Loads the harness's canned fixture data into the target environment — separate from (and not to be confused with) the AWS stack deploy above, hence its own Deploy sub-group under Test Harness rather than merging with it.",
      params: [],
      check: "command -v pytest",
      interactive: false,
      steps: null,
    },
    {
      id: "configure",
      title: "Configure",
      origin: null,
      category: "Deploy",
      description:
        "Tunes the environment's full parameter set in one pass — most operators only ever touch the first few; everything else is fine at its default. Exercises the parameters module's fold (17 params, 3 required) at the board-app's largest realistic count.",
      params: [
        { name: "target", kind: "input", default: null },
        { name: "version", kind: "input", default: null },
        { name: "environment", kind: "pick", options: ["dev", "staging", "prod"], default: null },
        { name: "region", kind: "input", default: "eu-west-1" },
        { name: "replicas", kind: "input", default: "3" },
        { name: "timeout", kind: "input", default: "30s" },
        { name: "retries", kind: "input", default: "2" },
        { name: "log_level", kind: "pick", options: ["debug", "info", "warn", "error"], default: "info" },
        { name: "dry_run", kind: "input", default: "false" },
        { name: "cache", kind: "input", default: "true" },
        { name: "batch_size", kind: "input", default: "100" },
        { name: "concurrency", kind: "input", default: "4" },
        { name: "notify", kind: "input", default: "slack" },
        { name: "tag", kind: "input", default: "latest" },
        { name: "profile", kind: "input", default: "default" },
        { name: "token", kind: "input", default: "", secret: true },
        { name: "extra_notes", kind: "input", default: "" },
      ],
      check: "command -v sh",
      interactive: false,
      steps: null,
    },
  ],
};

export const mockListingTrusted: Listing = { ...mockListingUntrusted, trusted: true };

export const mockDoctorReport: DoctorReport = {
  schema: 1,
  name: "acme-ops",
  manifest: mockListingUntrusted.manifest,
  commands: [
    { id: "shell", title: "Shell", check: "command -v aws", ready: true, exit_code: 0 },
    { id: "status", title: "Status", check: "command -v sh", ready: true, exit_code: 0 },
    {
      id: "import",
      title: "Import",
      check: "command -v this-tool-does-not-exist",
      ready: false,
      exit_code: 1,
    },
    { id: "aws:whoami", title: "Identity", check: "command -v aws", ready: true, exit_code: 0 },
    // check: null / ready: null — doctor confirming there's nothing to
    // probe, not "hasn't answered yet". Exercises the new "no-check" single
    // neutral-gray-segment meter state (see readiness.ts).
    { id: "aws:deploy", title: "Deploy stack", check: null, ready: null, exit_code: null },
    {
      id: "test:smoke",
      title: "Smoke test",
      check: "command -v pytest",
      ready: true,
      exit_code: 0,
    },
    {
      id: "test:deploy",
      title: "Deploy fixtures",
      check: "command -v pytest",
      ready: true,
      exit_code: 0,
    },
    { id: "configure", title: "Configure", check: "command -v sh", ready: true, exit_code: 0 },
  ],
};
