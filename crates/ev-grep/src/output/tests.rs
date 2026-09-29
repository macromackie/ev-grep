use std::collections::BTreeMap;

use ev_grep_core::{
    Assessment, Candidate, LineRange, Outcome, Position, Region, ScanEvent, Uncertainty,
};
use serde_json::{Value, json};

use super::{Format, Output, Sort};

fn candidate(path: &str, lines: Option<LineRange>) -> Candidate {
    let lines = lines.unwrap_or(LineRange { start: 1, end: 1 });
    Candidate {
        file: Some(path.into()),
        text: "\n".repeat(lines.end - lines.start + 1),
        range: Region {
            start: Position {
                line: lines.start - 1,
                column: 0,
            },
            end: Position {
                line: lines.end,
                column: 0,
            },
            byte_offset: None,
        },
        context: None,
    }
}

fn assessment(outcome: Outcome, reason: Option<Uncertainty>, confidence: f64) -> Assessment {
    Assessment {
        outcome,
        reason,
        choice: if outcome == Outcome::Uncertain {
            Outcome::Match
        } else {
            outcome
        },
        confidence,
        probabilities: BTreeMap::from([(outcome, confidence)]),
        model: "fixture".into(),
        request: Some(ev_grep_core::RequestInfo {
            input_tokens: 1,
            output_tokens: 1,
            ..Default::default()
        }),
    }
}

#[test]
fn score_order_preserves_assessments_and_summary() -> anyhow::Result<()> {
    let mut runs = Vec::new();
    for sort in [Sort::None, Sort::Score] {
        let mut output = Output::new(Vec::new(), Format::Json, false, sort);
        for (path, line, probability, confidence) in [
            ("a.rs", 10, 0.9, 0.9),
            ("d.rs", 1, 0.01, 0.98),
            ("c.rs", 1, 0.7, 0.55),
            ("b.rs", 1, 0.95, 0.92),
            ("a.rs", 2, 0.9, 0.9),
        ] {
            let choice = if probability > 0.5 {
                Outcome::Match
            } else {
                Outcome::NoMatch
            };
            let mut answer = assessment(choice, None, confidence);
            answer.probabilities = BTreeMap::from([
                (Outcome::Match, probability),
                (Outcome::NoMatch, 1.0 - probability),
                (Outcome::Uncertain, 0.0),
            ]);
            output.summary.selected += 1;
            output.scan_event(ScanEvent::Result {
                candidate: candidate(
                    path,
                    Some(LineRange {
                        start: line,
                        end: line,
                    }),
                ),
                assessment: Box::new(answer.with_min_confidence(0.8)),
            })?;
        }
        assert_eq!(output.writer.is_empty(), sort == Sort::Score);
        assert_eq!(output.summary.exit_code(), 3);
        output.error(Some("failed.rs"), None, "request failed")?;
        assert_eq!(output.finish()?, 2);
        let records = std::str::from_utf8(&output.writer)?
            .lines()
            .map(serde_json::from_str::<Value>)
            .collect::<Result<Vec<_>, _>>()?;
        runs.push(records);
    }
    let results = |run: usize| {
        runs[run]
            .iter()
            .filter(|r| r["type"] == "result")
            .collect::<Vec<_>>()
    };
    let streamed = results(0);
    assert_eq!(results(1), [3, 4, 0, 2, 1].map(|i| streamed[i]));
    assert_eq!(runs[0].last(), runs[1].last());
    assert_eq!(runs[1][0]["type"], "error");
    Ok(())
}

#[test]
fn json_retains_uncertainty_and_errors_take_exit_precedence() -> anyhow::Result<()> {
    let mut output = Output::new(Vec::new(), Format::Json, false, Sort::None);
    output.summary.selected = 2;
    output.scan_event(ScanEvent::Result {
        candidate: candidate("a.py", Some(LineRange { start: 3, end: 9 })),
        assessment: Box::new(Assessment {
            outcome: Outcome::Uncertain,
            reason: Some(Uncertainty::LowConfidence),
            choice: Outcome::NoMatch,
            confidence: 0.5,
            probabilities: BTreeMap::from([
                (Outcome::NoMatch, 0.7),
                (Outcome::Match, 0.2),
                (Outcome::Uncertain, 0.1),
            ]),
            model: "fixture".into(),
            request: Some(ev_grep_core::RequestInfo {
                input_tokens: 1,
                output_tokens: 1,
                ..Default::default()
            }),
        }),
    })?;
    assert_eq!(output.summary.exit_code(), 3);
    output.error(Some("b.py"), None, "request failed")?;
    assert_eq!(output.finish()?, 2);
    let records: Vec<Value> = std::str::from_utf8(&output.writer)?
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?;
    assert_eq!(records[0]["data"]["file"], "a.py");
    assert_eq!(records[0]["data"]["range"]["start"]["line"], 2);
    assert_eq!(records[0]["data"]["range"]["end"]["line"], 9);
    assert!(records[1]["data"].get("start_line").is_none());
    assert_eq!(records[0]["data"]["assessment"]["choice"], "no_match");
    assert!(records[0]["data"]["assessment"].get("outcome").is_none());
    assert_eq!(records[2]["type"], "summary");
    assert_eq!(records[2]["data"]["errors"], 1);
    Ok(())
}

#[test]
fn sarif_reports_matches_and_uncertain_results_with_locations() -> anyhow::Result<()> {
    let mut output = Output::new(Vec::new(), Format::Sarif, false, Sort::None);
    let query = "Catches a database failure\nand returns an empty result.\n";
    output.begin(query, json!({"provider": "openrouter", "dry_run": false}))?;
    output.summary.selected = 5;
    for (path, lines, assessment) in [
        (
            "./src/users.py",
            Some(LineRange { start: 20, end: 56 }),
            assessment(Outcome::Match, None, 0.93),
        ),
        (
            "src/odd:12 x.py",
            None,
            assessment(Outcome::Uncertain, Some(Uncertainty::LowConfidence), 0.62),
        ),
        (
            "src/cart.py",
            None,
            assessment(Outcome::NoMatch, None, 0.97),
        ),
    ] {
        output.scan_event(ScanEvent::Result {
            candidate: candidate(path, lines),
            assessment: Box::new(assessment),
        })?;
    }
    output.scan_event(ScanEvent::Skipped {
        path: "/abs/image.py".into(),
        lines: None,
    })?;
    output.error(Some("src/big.py"), None, "file exceeds 65536 bytes")?;
    assert_eq!(output.finish()?, 2);

    let log: Value = serde_json::from_slice(&output.writer)?;
    assert_eq!(log["version"], "2.1.0");
    let run = &log["runs"][0];
    let rule = &run["tool"]["driver"]["rules"][0];
    // Code scanning tracks alerts by rule ID, so the ID for a query must never change.
    assert_eq!(rule["id"], "query/9884e336a13bc935");
    assert_eq!(
        rule["shortDescription"]["text"],
        "Catches a database failure"
    );
    let results = run["results"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    assert_eq!(results.len(), 2, "nonmatches are left out");
    assert!(results.iter().all(|result| result["ruleId"] == rule["id"]));
    let location = |index: usize| &results[index]["locations"][0]["physicalLocation"];
    assert_eq!(results[0]["level"], "warning");
    assert_eq!(
        location(0)["artifactLocation"],
        json!({"uri": "src/users.py", "uriBaseId": "%SRCROOT%"})
    );
    assert_eq!(
        location(0)["region"],
        json!({"startLine": 20, "startColumn": 1, "endLine": 57, "endColumn": 1})
    );
    assert_eq!(results[1]["level"], "note");
    assert_eq!(
        location(1)["artifactLocation"]["uri"],
        "src/odd%3A12%20x.py"
    );
    assert_eq!(
        location(1)["region"],
        json!({"startLine": 1, "startColumn": 1, "endLine": 2, "endColumn": 1})
    );
    assert_eq!(
        results[1]["message"]["text"],
        "Catches a database failure (ev-grep uncertain: low confidence 0.62)"
    );
    assert_eq!(results[1]["properties"]["assessment"]["choice"], "match");

    let invocation = &run["invocations"][0];
    assert_eq!(invocation["executionSuccessful"], false);
    let notifications = invocation["toolExecutionNotifications"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    assert_eq!(notifications.len(), 2);
    assert_eq!(notifications[0]["level"], "note");
    assert_eq!(
        notifications[0]["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
        "file:///abs/image.py"
    );
    assert_eq!(notifications[1]["level"], "error");
    assert_eq!(run["properties"]["evGrep"]["summary"]["errors"], 1);
    Ok(())
}
