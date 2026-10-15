//! Typed mirrors of pult's documented JSON surfaces (schema 2).
//!
//! Every `--json` output is one document in the Lonic envelope, its payload
//! under `data` on success or an `error` on failure; [`parse_document`]
//! unwraps it. The structs below deserialize `data` for `pult --list --json`
//! and `pult doctor --json`. Per the reference (docs/reference.md in the pult
//! repo), changes within a schema are additive-only, so unknown fields are
//! ignored (serde's default behavior) rather than rejected — we never use
//! `deny_unknown_fields`. A different schema is refused outright.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// The envelope schema this app reads.
pub const SCHEMA: u32 = 2;

/// One `<param>` entry from a command's `params` array.
///
/// A param's `kind` is open: `pick` (with `options` or `source`), `input`,
/// `flag` and `repeat` today, and a later pult may add more within schema 2.
/// Rather than model that as an enum keyed on `kind`, we keep every field
/// optional and let callers branch on `kind` + field presence; a kind the
/// frontend doesn't know makes its command unrunnable here, nothing more.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Param {
    pub name: String,
    /// `"pick"`, `"input"`, `"flag"` or `"repeat"` — or a kind this app
    /// doesn't know yet.
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
    /// `input.secret` / `repeat.secret` — render as a password field, never
    /// log or persist.
    #[serde(default)]
    pub secret: Option<bool>,
    /// `flag`: the word the tool receives when set. `repeat`: the word before
    /// each item, or `null` for a positional list. Informational only — pult
    /// renders it; values are passed by `name`.
    #[serde(default)]
    pub arg: Option<String>,
    /// `repeat`: the fewest items accepted.
    #[serde(default)]
    pub min: Option<u32>,
    /// `repeat`: the most items accepted; `null` = unbounded.
    #[serde(default)]
    pub max: Option<u32>,
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
    /// Operator-facing one-liner shown on the board card. Additive schema
    /// field — pult is gaining it in parallel with this app, so it may be
    /// absent or `null` on any given pult release; treat that as "no
    /// description" (a title-only card), never as an error.
    #[serde(default)]
    pub description: Option<String>,
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

/// `pult --list --json`'s `data` — the stable listing surface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Listing {
    pub pult_version: String,
    pub name: String,
    pub manifest: String,
    pub dir: String,
    pub run_dir: String,
    pub scope: String,
    /// Whether this manifest is trusted at its current resolved hash.
    pub trusted: bool,
    /// The manifest's current trust hash — what the trust button grants, as
    /// `--trust=<hash>`, so a manifest that changes after the human reviewed
    /// it is refused rather than trusted.
    pub trust_hash: String,
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

/// `pult doctor --json`'s `data` — the stable readiness surface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorReport {
    pub name: String,
    pub manifest: String,
    pub commands: Vec<DoctorEntry>,
}

/// The `error` of a failure document. Classify by `code`, never by the
/// exit status or the prose; an unknown code is treated as `generic`.
#[derive(Debug, Clone, Deserialize)]
pub struct PultError {
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub details: Option<serde_json::Value>,
}

impl PultError {
    /// `details.what_changed` of a `trust_changed` refusal.
    pub fn what_changed(&self) -> Option<&str> {
        self.details.as_ref()?.get("what_changed")?.as_str()
    }
}

#[derive(Deserialize)]
struct Envelope {
    lonic_json: u32,
    tool: String,
    schema: u32,
    ok: bool,
    #[serde(default)]
    data: Option<serde_json::Value>,
    #[serde(default)]
    error: Option<PultError>,
}

/// Unwrap one `--json` document from pult's stdout: `Ok(Ok(data))` for a
/// success, `Ok(Err(error))` for a failure pult reported, and `Err` when the
/// output isn't a document this app can read — not JSON, not pult's, or
/// another schema. That last case is never guessed through: a pult too old
/// or too new for this app says so.
pub fn parse_document<T: DeserializeOwned>(stdout: &[u8]) -> Result<Result<T, PultError>, String> {
    let raw: serde_json::Value = serde_json::from_slice(stdout)
        .map_err(|e| format!("pult didn't answer with a JSON document ({e})"))?;
    // pult before schema 2 wrote its success documents bare, with only a
    // `schema` field: the same refusal as any other schema.
    let schema = raw.get("schema").and_then(|v| v.as_u64());
    if raw.get("lonic_json").is_none() && schema.is_some_and(|s| s != u64::from(SCHEMA)) {
        return Err(schema_mismatch(schema.unwrap_or_default()));
    }
    let doc: Envelope = serde_json::from_value(raw)
        .map_err(|e| format!("pult answered with a document this app can't read ({e})"))?;
    if doc.lonic_json != 1 || doc.tool != "pult" {
        return Err(format!(
            "pult answered with a document this app can't read (lonic_json {}, tool {:?})",
            doc.lonic_json, doc.tool
        ));
    }
    if doc.schema != SCHEMA {
        return Err(schema_mismatch(u64::from(doc.schema)));
    }
    match (doc.ok, doc.data, doc.error) {
        (true, Some(data), _) => serde_json::from_value(data)
            .map(Ok)
            .map_err(|e| format!("pult's document is missing something this app needs ({e})")),
        (false, _, Some(error)) => Ok(Err(error)),
        _ => Err("pult's document has neither data nor an error".to_string()),
    }
}

fn schema_mismatch(schema: u64) -> String {
    format!(
        "This pult speaks JSON schema {schema}, and pult-desktop needs schema {SCHEMA}. \
         Install a pult that matches this app, or set its path in Settings."
    )
}

/// A line of output streamed while a command runs, the machine events pult
/// may emit on its `PULT_EVENTS` channel (see `crate::events` and
/// `pult_bin::run_streaming`), plus the terminal exit event. Emitted on the
/// `pult://run-output` Tauri event channel.
///
/// `run_id` is a client-generated id (one per `run_command` invocation),
/// threaded through so the frontend can tell concurrent runs apart — the
/// event channel is shared across every in-flight run, not per-command, so
/// without this a second run's output could get attributed to the first.
///
/// `tail_gen` (additive, `Option<u64>` on every variant but `TailStart`
/// itself) is the generation-fencing field fix round 2 added
/// (`crate::journal::TailRegistry`): every event a tail emits is stamped
/// with the generation of that tail (see `RunEvent::stamp_tail_gen`), and a
/// tail's very first emission is always `TailStart`, carrying that same
/// generation as its own required field rather than the additive one — a
/// frontend router uses it to adopt "the current generation" for a run_id
/// and drop any later-arriving event whose `tail_gen` doesn't match (a
/// straggler from a tail that's since been cancelled and superseded). Absent
/// `tail_gen` (older/other producers, and every non-`TailStart` event this
/// reader constructs directly before stamping — see `journal.rs`'s
/// `map_event_line`/`synthesize_exit`/`emit_never_journaled`) means "no
/// fence to check," same as a match.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunEvent {
    /// Emitted before any other event in a tail's lifetime (see
    /// `crate::journal::tail_run`) — signals that a fresh generation of
    /// tailing for `run_id` has begun, so a frontend router mid-stream can
    /// reset a record's replayable fields and adopt `tail_gen` as the
    /// record's current generation before backlog starts arriving.
    TailStart { run_id: String, tail_gen: u64 },
    Line {
        run_id: String,
        stream: String,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tail_gen: Option<u64>,
    },
    /// `step <k>/<n> <name>` from the `PULT_EVENTS` channel — entering step
    /// `k` of `n` (1-based). Unix only: this app only claims the channel on
    /// unix (see `pult_bin::run_streaming`), so this never fires on Windows.
    Step {
        run_id: String,
        k: u32,
        n: u32,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tail_gen: Option<u64>,
    },
    /// `progress <0-100|?> [text]` from the `PULT_EVENTS` channel —
    /// `pct: None` is the indeterminate `?` form. Unix only, same caveat as
    /// `Step`. `pct` is the child's own figure, within the current step;
    /// `run_pct` is pult's whole-run figure, the step's band of the run
    /// (`None` when `pct` is, or from a journal that predates it).
    Progress {
        run_id: String,
        pct: Option<u8>,
        run_pct: Option<u8>,
        text: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tail_gen: Option<u64>,
    },
    /// `status <text>` from the `PULT_EVENTS` channel — a transient activity
    /// line. Unix only, same caveat as `Step`.
    Status {
        run_id: String,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tail_gen: Option<u64>,
    },
    Exit {
        run_id: String,
        code: Option<i32>,
        /// Whether this run ended because `stop_run` was called, rather than
        /// the command exiting on its own — lets the UI show "stopped" as
        /// distinct from a natural (possibly non-zero, possibly signal-killed
        /// from outside this app) exit.
        stopped: bool,
        /// Reader-derived: `meta.json` said `"running"` but the journaling
        /// pult process was dead (crash detection — see
        /// `crate::journal::synthesize_exit`). Never set by a journaled
        /// `exit` line itself, only synthesized by the tail when no such
        /// line ever arrives. Additive: defaults to `false` so older/other
        /// producers of this event don't need to know about it.
        #[serde(default)]
        crashed: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tail_gen: Option<u64>,
    },
}

impl RunEvent {
    /// Stamp (or overwrite) this event's `tail_gen` with `gen` — the one
    /// place every event a tail constructs gets tagged with its generation
    /// right before reaching the Tauri channel (see
    /// `crate::journal::tail_run`). `TailStart` already carries its
    /// generation as a required field; stamping it again here is a harmless
    /// no-op (same value).
    pub fn stamp_tail_gen(self, gen: u64) -> Self {
        match self {
            RunEvent::TailStart { run_id, .. } => RunEvent::TailStart {
                run_id,
                tail_gen: gen,
            },
            RunEvent::Line {
                run_id,
                stream,
                text,
                ..
            } => RunEvent::Line {
                run_id,
                stream,
                text,
                tail_gen: Some(gen),
            },
            RunEvent::Step {
                run_id, k, n, name, ..
            } => RunEvent::Step {
                run_id,
                k,
                n,
                name,
                tail_gen: Some(gen),
            },
            RunEvent::Progress {
                run_id,
                pct,
                run_pct,
                text,
                ..
            } => RunEvent::Progress {
                run_id,
                pct,
                run_pct,
                text,
                tail_gen: Some(gen),
            },
            RunEvent::Status { run_id, text, .. } => RunEvent::Status {
                run_id,
                text,
                tail_gen: Some(gen),
            },
            RunEvent::Exit {
                run_id,
                code,
                stopped,
                crashed,
                ..
            } => RunEvent::Exit {
                run_id,
                code,
                stopped,
                crashed,
                tail_gen: Some(gen),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(doc: &str) -> Result<Result<serde_json::Value, PultError>, String> {
        parse_document(doc.as_bytes())
    }

    #[test]
    fn a_schema_2_document_yields_its_data_or_its_error() {
        let ok = parse(
            r#"{"lonic_json":1,"tool":"pult","schema":2,"command":"list","ok":true,"data":{"a":1}}"#,
        );
        assert_eq!(ok.unwrap().unwrap(), serde_json::json!({"a": 1}));

        let refused = parse(
            r#"{"lonic_json":1,"tool":"pult","schema":2,"command":"list","ok":false,
                "error":{"code":"trust_changed","message":"m",
                         "details":{"what_changed":"unexpected"}}}"#,
        )
        .unwrap()
        .unwrap_err();
        assert_eq!(refused.code, "trust_changed");
        assert_eq!(refused.what_changed(), Some("unexpected"));
    }

    /// pult 0.4: bare success documents and schema-1 error envelopes.
    #[test]
    fn another_schema_is_refused_by_name_never_read() {
        let bare = parse(r#"{"schema":1,"name":"x","manifest":"m","commands":[]}"#).unwrap_err();
        assert!(bare.contains("schema 1"), "{bare}");

        let old_error = parse(
            r#"{"lonic_json":1,"tool":"pult","schema":1,"command":"list","ok":false,
                "error":{"code":"no_manifest","message":"m"}}"#,
        )
        .unwrap_err();
        assert!(old_error.contains("schema 1"), "{old_error}");

        let newer = parse(
            r#"{"lonic_json":1,"tool":"pult","schema":3,"command":"list","ok":true,"data":{}}"#,
        )
        .unwrap_err();
        assert!(newer.contains("schema 3"), "{newer}");
    }

    #[test]
    fn output_that_is_not_pults_document_is_refused() {
        assert!(parse("not json").is_err());
        assert!(parse(
            r#"{"lonic_json":1,"tool":"forklift","schema":2,"command":null,"ok":true,"data":{}}"#
        )
        .is_err());
    }
}
