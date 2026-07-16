//! Parser for pult's `PULT_EVENTS` progress-events protocol (v1) — see the
//! pult repo's `docs/reference.md`, "Events protocol — `PULT_EVENTS`"
//! section. This app is what claims that channel now (see
//! `pult_bin::run_streaming`'s doc comment for the fd-passing mechanism), so
//! parsing here has to match pult's own leniency precisely — this mirrors
//! pult's own `src/events.rs` grammar exactly (same three verbs, same
//! clamping/rejection rules), the same "replicate pult's documented
//! semantics rather than invent new ones" approach `pult_bin::resolve_pick_source`
//! already takes for `pick.source`.
//!
//! Three verbs, v1 (quoting the reference doc):
//!
//! ```text
//! progress <0-100|?> [text]    # determinate percent, or ? = indeterminate
//! status <text>                # transient activity line
//! step <k>/<n> <name>          # entering step k of n
//! ```
//!
//! "Unknown verbs and malformed lines are silently ignored — never an error,
//! in either direction." — [`parse`] never panics and never returns an
//! error; a line that doesn't fit the grammar is just dropped.

/// One parsed protocol line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PultEvent {
    /// `progress <0-100|?> [text]` — `pct: None` is the indeterminate `?` form.
    Progress { pct: Option<u8>, text: Option<String> },
    /// `status <text>` — a transient activity line; the plain CLI consumes
    /// and drops it, it exists for richer surfaces like this one.
    Status(String),
    /// `step <k>/<n> <name>` — entering step `k` of `n` (1-based, `k <= n`).
    Step { k: u32, n: u32, name: String },
}

/// Parse one line of the protocol. Returns `None` for anything malformed or
/// an unrecognized verb — callers must never error on this (a run must never
/// crash because a script wrote a garbled or forward-looking event line).
pub fn parse(line: &str) -> Option<PultEvent> {
    let line = line.trim();
    let (verb, rest) = split_first(line);
    match verb {
        "progress" => parse_progress(rest),
        "status" => {
            if rest.is_empty() {
                None
            } else {
                Some(PultEvent::Status(rest.to_string()))
            }
        }
        "step" => parse_step(rest),
        _ => None,
    }
}

fn parse_progress(rest: &str) -> Option<PultEvent> {
    let (pct_str, text) = split_first(rest);
    let text = if text.is_empty() { None } else { Some(text.to_string()) };
    if pct_str == "?" {
        return Some(PultEvent::Progress { pct: None, text });
    }
    // Parsed as u32 (not u8): a percent above 255 (e.g. `300`) must still
    // clamp to 100 rather than fail the parse and drop the event entirely —
    // a stuck progress indicator is worse than an over-clamped one.
    let pct: u32 = pct_str.parse().ok()?;
    Some(PultEvent::Progress {
        pct: Some(pct.min(100) as u8),
        text,
    })
}

fn parse_step(rest: &str) -> Option<PultEvent> {
    let (kn, name) = split_first(rest);
    let (k_str, n_str) = kn.split_once('/')?;
    let k: u32 = k_str.parse().ok()?;
    let n: u32 = n_str.parse().ok()?;
    if k == 0 || n == 0 || k > n || name.is_empty() {
        return None;
    }
    Some(PultEvent::Step {
        k,
        n,
        name: name.to_string(),
    })
}

/// Split on the first space; `("", "")` for an empty input, `(whole, "")`
/// when there's no space.
fn split_first(s: &str) -> (&str, &str) {
    match s.split_once(' ') {
        Some((a, b)) => (a, b.trim_start()),
        None => (s, ""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_determinate_progress() {
        assert_eq!(
            parse("progress 40 restoring"),
            Some(PultEvent::Progress {
                pct: Some(40),
                text: Some("restoring".to_string())
            })
        );
    }

    #[test]
    fn parses_progress_without_text() {
        assert_eq!(
            parse("progress 40"),
            Some(PultEvent::Progress {
                pct: Some(40),
                text: None
            })
        );
    }

    #[test]
    fn parses_indeterminate_progress() {
        assert_eq!(
            parse("progress ?"),
            Some(PultEvent::Progress { pct: None, text: None })
        );
        assert_eq!(
            parse("progress ? thinking"),
            Some(PultEvent::Progress {
                pct: None,
                text: Some("thinking".to_string())
            })
        );
    }

    #[test]
    fn clamps_percent_over_100() {
        assert_eq!(
            parse("progress 150 uploading"),
            Some(PultEvent::Progress {
                pct: Some(100),
                text: Some("uploading".to_string())
            })
        );
    }

    #[test]
    fn clamps_percent_far_over_100() {
        // `300` doesn't fit in a u8, so parsing it as one used to fail the
        // event entirely (a frozen progress bar); parsed as u32 first, it
        // clamps to 100 like any other over-range value.
        assert_eq!(
            parse("progress 300 uploading"),
            Some(PultEvent::Progress {
                pct: Some(100),
                text: Some("uploading".to_string())
            })
        );
    }

    #[test]
    fn parses_status() {
        assert_eq!(
            parse("status restoring the database"),
            Some(PultEvent::Status("restoring the database".to_string()))
        );
    }

    #[test]
    fn status_without_text_is_malformed() {
        assert_eq!(parse("status"), None);
        assert_eq!(parse("status "), None);
    }

    #[test]
    fn parses_step() {
        assert_eq!(
            parse("step 2/5 run-migrations"),
            Some(PultEvent::Step {
                k: 2,
                n: 5,
                name: "run-migrations".to_string()
            })
        );
    }

    #[test]
    fn rejects_step_zero_of_n() {
        assert_eq!(parse("step 0/5 x"), None);
    }

    #[test]
    fn rejects_step_zero_total() {
        assert_eq!(parse("step 1/0 x"), None);
    }

    #[test]
    fn rejects_k_greater_than_n() {
        assert_eq!(parse("step 6/5 x"), None);
    }

    #[test]
    fn allows_k_equal_n() {
        assert!(parse("step 5/5 last").is_some());
    }

    #[test]
    fn junk_lines_are_ignored() {
        assert_eq!(parse(""), None);
        assert_eq!(parse("hello world"), None);
        assert_eq!(parse("progress abc"), None);
        assert_eq!(parse("step notaslash x"), None);
        assert_eq!(parse("step 1/2"), None); // missing name
    }

    #[test]
    fn unknown_verb_is_ignored() {
        assert_eq!(parse("frobnicate 42"), None);
    }
}
