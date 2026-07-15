// Fixture data for VITE_MOCK=1 — lets the UI be developed and screenshotted
// in a plain browser, no Tauri runtime needed. Shaped to exercise every v0
// surface: 3 display groups (one from a module, "AWS Tooling"), a secret
// param, a failing check, an interactive command, dynamic pick options, and
// starts untrusted so the trust modal flow is reachable. Most commands carry
// a realistic operator-voice `description` for the board card; "status" is
// deliberately left without one, to prove the title-only card looks
// intentional rather than like a bug.

import type { DoctorReport, Listing } from "../types";

const AWS_ORIGIN = "github.com/opskit/aws-common@v1.4.2";

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
  ],
  commands: [
    {
      id: "shell",
      title: "Open a shell",
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
      title: "Show status",
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
      title: "Import data",
      origin: null,
      category: "Deploy",
      description:
        "Pulls the latest export from the vendor and loads it into the local dataset. Needs a fresh access token.",
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
      title: "Show caller identity",
      origin: AWS_ORIGIN,
      category: null,
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
      category: null,
      description:
        "Builds, pushes, and releases the stack to the chosen region. Safe to re-run — steps report progress as they complete.",
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
      steps: ["build", "push", "release"],
    },
  ],
};

export const mockListingTrusted: Listing = { ...mockListingUntrusted, trusted: true };

export const mockDoctorReport: DoctorReport = {
  schema: 1,
  name: "acme-ops",
  manifest: mockListingUntrusted.manifest,
  commands: [
    { id: "shell", title: "Open a shell", check: "command -v aws", ready: true, exit_code: 0 },
    { id: "status", title: "Show status", check: "command -v sh", ready: true, exit_code: 0 },
    {
      id: "import",
      title: "Import data",
      check: "command -v this-tool-does-not-exist",
      ready: false,
      exit_code: 1,
    },
    { id: "aws:whoami", title: "Show caller identity", check: "command -v aws", ready: true, exit_code: 0 },
    { id: "aws:deploy", title: "Deploy stack", check: null, ready: null, exit_code: null },
  ],
};
