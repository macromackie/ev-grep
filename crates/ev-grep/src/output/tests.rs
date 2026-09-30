use std::collections::BTreeMap;

use ev_grep_core::{
    Assessment, Candidate, LineRange, Outcome, Position, Region, ScanEvent, Uncertainty,
};
use serde_json::Value;

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
    output.scan_event(ScanEvent::Error(ev_grep_core::FileError::new(
        "b.py",
        None,
        "request failed",
    )))?;
    assert_eq!(output.finish()?, 2);
    let records: Vec<Value> = std::str::from_utf8(&output.writer)?
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?;
    assert_eq!(records[0]["data"]["file"], "a.py");
    assert_eq!(records[0]["data"]["assessment"]["choice"], "no_match");
    assert!(records[0]["data"]["assessment"].get("outcome").is_none());
    assert_eq!(records[2]["type"], "summary");
    assert_eq!(records[2]["data"]["errors"], 1);
    assert_eq!(records[2]["data"]["selected"], 2);
    Ok(())
}
