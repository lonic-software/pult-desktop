//! Typed mirrors of pult's documented JSON surfaces (schema 1).
//!
//! These structs deserialize `pult --list --json` and `pult doctor --json`.
//! Per the reference (docs/reference.md in the pult repo), schema 1 changes
//! are additive-only, so unknown fields are ignored (serde's default
//! behavior) rather than rejected — we never use `deny_unknown_fields`.

use serde::{Deserialize, Serialize};

/// One `<param>` entry from a command's `params` array.
///
/// A param is exactly one of `pick` (with `options` or `source`) or `input`;
/// rather than model that as an enum keyed on `kind`, we keep every field
/// optional and let callers branch on `kind` + field presence. This is more
/// forgiving of additive schema changes than a strict tagged enum.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Param {
    pub name: String,
    /// `"pick"` or `"input"`.
    pub kind: String,
    /// `pick` with a static list.
    #[serde(default)]
    pub options: Option<Vec<String>>,
    /// `pick` with a dynamic shell source (its stdout lines become options).
    #[serde(default)]
    pub source: Option<String>,
    /// Params this `pick.source` interpolates; supply those first.
    #[serde(default)]
    pub depends_on: Option<Vec<String>>,
    /// `input` default value, if any.
    #[serde(default)]
    pub default: Option<String>,
    /// `input.secret` — render as a password field, never log or persist.
    #[serde(default)]
    pub secret: Option<bool>,
}

/// One entry in `includes[]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncludeInfo {
    pub source: String,
    pub kind: String,
    /// The include's declared `name:` (var-substituted), or `null`.
    pub name: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub rev: Option<String>,
    #[serde(default)]
    pub rev_kind: Option<String>,
    #[serde(default)]
    pub resolved_sha: Option<String>,
}

/// One entry in `commands[]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandInfo {
    pub id: String,
    pub title: String,
    /// The include source this command came from; `null` = root manifest.
    pub origin: Option<String>,
    /// The raw `category:` the author declared; `null` = none. This is the
    /// grouping rule's *input*, not its computed group — grouping happens in
    /// the frontend (see src/lib/grouping.ts), mirroring the rule in
    /// docs/reference.md so it stays in one place, documented, and testable.
    pub category: Option<String>,
    pub params: Vec<Param>,
    /// The readiness probe; `null` = none declared.
    pub check: Option<String>,
    #[serde(default)]
    pub interactive: bool,
    /// Step labels a live run emits as `step k/n <name>`; `null` for a
    /// string-form `run:`.
    #[serde(default)]
    pub steps: Option<Vec<String>>,
}

/// `pult --list --json` — the stable listing surface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Listing {
    pub schema: u32,
    pub pult_version: String,
    pub name: String,
    pub manifest: String,
    pub dir: String,
    pub run_dir: String,
    pub scope: String,
    /// Whether this manifest is trusted at its current resolved hash.
    pub trusted: bool,
    pub includes: Vec<IncludeInfo>,
    pub commands: Vec<CommandInfo>,
}

/// One entry in `pult doctor --json`'s `commands[]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorEntry {
    pub id: String,
    pub title: String,
    pub check: Option<String>,
    /// `null` when the command declares no `check:` — nothing ran, not a failure.
    pub ready: Option<bool>,
    pub exit_code: Option<i32>,
}

/// `pult doctor --json` — the stable readiness surface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorReport {
    pub schema: u32,
    pub name: String,
    pub manifest: String,
    pub commands: Vec<DoctorEntry>,
}

/// A line of output streamed while a command runs, plus the terminal exit
/// event. Emitted on the `pult://run-output` Tauri event channel.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunEvent {
    Line { stream: String, text: String },
    Exit { code: Option<i32> },
}
