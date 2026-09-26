//! SARIF 2.1.0 output for code scanning tools. A SARIF log is one document, so results are collected during the
//! search and written when it ends.

use std::{collections::BTreeMap, path::Path};

use ev_grep_core::{Assessment, LineRange, Outcome, Uncertainty};
use serde_json::{Value, json};

const SCHEMA: &str = "https://json.schemastore.org/sarif-2.1.0.json";
// GitHub code scanning shows at most 1024 characters of a rule description.
const DESCRIPTION_CHARS: usize = 1024;
const TITLE_CHARS: usize = 200;

#[derive(Default)]
pub(crate) struct Sarif {
    query: Option<String>,
    properties: Value,
    results: Vec<Value>,
    notifications: Vec<Value>,
    artifacts: BTreeMap<String, usize>,
}

impl Sarif {
    pub(crate) fn begin(&mut self, query: &str, properties: Value) {
        self.query = Some(query.trim().to_owned());
        self.properties = properties;
    }

    /// Matches become warnings and uncertain results become notes. Nonmatches are left out, as in terminal output.
    pub(crate) fn result(&mut self, path: &str, lines: Option<LineRange>, assessment: &Assessment) {
        let Some(query) = &self.query else {
            return;
        };
        let (level, detail) = match (assessment.outcome, assessment.reason) {
            (Outcome::NoMatch, _) => return,
            (Outcome::Match, _) => (
                "warning",
                format!("ev-grep match, confidence {:.2}", assessment.confidence),
            ),
            (Outcome::Uncertain, Some(Uncertainty::LowConfidence)) => (
                "note",
                format!(
                    "ev-grep uncertain: low confidence {:.2}",
                    assessment.confidence
                ),
            ),
            (Outcome::Uncertain, _) => ("note", "ev-grep uncertain: insufficient context".into()),
        };
        self.results.push(json!({
            "ruleId": rule_id(query),
            "ruleIndex": 0,
            "level": level,
            "message": { "text": format!("{} ({detail})", clip(title(query), TITLE_CHARS)) },
            "locations": [location(path, lines)],
            "properties": { "assessment": assessment }
        }));
    }

    pub(crate) fn notification(
        &mut self,
        level: &str,
        path: Option<&str>,
        lines: Option<LineRange>,
        message: &str,
    ) {
        let mut notification = json!({ "level": level, "message": { "text": message } });
        if let Some(path) = path {
            notification["locations"] = json!([location(path, lines)]);
        }
        self.notifications.push(notification);
    }

    /// Dry runs list the selected files as artifacts, because there are no results to report.
    pub(crate) fn artifact(&mut self, path: &str, bytes: usize) {
        self.artifacts.insert(path.to_owned(), bytes);
    }

    pub(crate) fn document(&self, summary: Value, cwd: Option<&Path>) -> Value {
        let rules: Vec<Value> = self.query.iter().map(|query| rule(query)).collect();
        let mut properties = self.properties.clone();
        properties["summary"] = summary;
        let mut run = json!({
            "tool": { "driver": {
                "name": "ev-grep",
                "version": env!("CARGO_PKG_VERSION"),
                "semanticVersion": env!("CARGO_PKG_VERSION"),
                "informationUri": "https://ev-grep.com",
                "rules": rules
            }},
            "invocations": [{
                "executionSuccessful": self.notifications.iter().all(|n| n["level"] != "error"),
                "toolExecutionNotifications": self.notifications
            }],
            "results": self.results,
            "properties": { "evGrep": properties }
        });
        if let Some(cwd) = cwd.and_then(Path::to_str) {
            run["originalUriBaseIds"] = json!({ "%SRCROOT%": { "uri": format!("file://{}/", encode(cwd.trim_end_matches('/'))) } });
        }
        if !self.artifacts.is_empty() {
            let artifacts: Vec<Value> = self
                .artifacts
                .iter()
                .map(
                    |(path, bytes)| json!({ "location": artifact_location(path), "length": bytes }),
                )
                .collect();
            run["artifacts"] = json!(artifacts);
        }
        json!({ "$schema": SCHEMA, "version": "2.1.0", "runs": [run] })
    }
}

fn rule(query: &str) -> Value {
    json!({
        "id": rule_id(query),
        "name": "EvGrepQuery",
        "shortDescription": { "text": clip(title(query), DESCRIPTION_CHARS) },
        "fullDescription": { "text": clip(query, DESCRIPTION_CHARS) },
        "help": { "text": format!(
            "ev-grep asked Jev whether each selected file or line range matches this query:\n\n{query}\n\n\
             Matches are warnings. Uncertain results are notes: Jev lacked context or confidence, so a person \
             should check them. Model results can be wrong; use them to decide what to read."
        )},
        "defaultConfiguration": { "level": "warning" }
    })
}

/// A stable ID for the query, so code scanning tracks the same rule across runs. FNV-1a keeps it independent of
/// Rust's hasher, which may change between releases.
fn rule_id(query: &str) -> String {
    let hash = query.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    });
    format!("query/{hash:016x}")
}

fn title(query: &str) -> &str {
    query
        .lines()
        .find(|line| !line.trim().is_empty())
        .map_or(query, str::trim)
}

/// Shortens display text for code scanning's limits. The full query stays in the rule's help text.
fn clip(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_owned();
    }
    let end = text
        .char_indices()
        .nth(limit.saturating_sub(1))
        .map_or(text.len(), |(index, _)| index);
    format!("{}…", &text[..end])
}

/// Whole-file results point at line 1, because code scanning needs a line to display a result.
fn location(path: &str, lines: Option<LineRange>) -> Value {
    let region = match lines {
        Some(lines) => json!({ "startLine": lines.start, "endLine": lines.end }),
        None => json!({ "startLine": 1 }),
    };
    json!({ "physicalLocation": { "artifactLocation": artifact_location(path), "region": region } })
}

/// Relative paths resolve against `%SRCROOT%`, the directory ev-grep ran in; absolute paths become file URIs.
fn artifact_location(path: &str) -> Value {
    if Path::new(path).is_absolute() {
        json!({ "uri": format!("file://{}", encode(path)) })
    } else {
        let mut relative = path;
        while let Some(rest) = relative.strip_prefix("./") {
            relative = rest;
        }
        json!({ "uri": encode(relative), "uriBaseId": "%SRCROOT%" })
    }
}

/// Percent-encodes everything but unreserved characters and `/`. Encoding `:` keeps a name like `odd:12` from
/// reading as a URI scheme.
fn encode(path: &str) -> String {
    let mut encoded = String::with_capacity(path.len());
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::{clip, encode, title};

    #[test]
    fn display_text_is_clipped_by_characters_and_paths_are_encoded() {
        assert_eq!(clip("abcdef", 6), "abcdef");
        assert_eq!(clip("abcdefg", 6), "abcde…");
        assert_eq!(clip("ééééé", 5), "ééééé");
        assert_eq!(clip("éééééé", 5), "éééé…");
        assert_eq!(title("\n  First line \nsecond"), "First line");
        assert_eq!(encode("src/new orders%.py"), "src/new%20orders%25.py");
        assert_eq!(encode("src/café.py"), "src/caf%C3%A9.py");
    }
}
