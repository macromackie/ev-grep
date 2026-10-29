use alloc::collections::BTreeMap;
use core::str;

use ev_grep_core::{
    Assessment,
    Candidate,
    ErrorKind,
    FileError,
    LineRange,
    Outcome,
    Position,
    Region,
    ScanEvent,
};
use serde_json::Value;

use super::{ExitStatus, Format, Output, Sort};

/// The value at a JSON pointer, or null when it is missing, as `Value`'s index reads it.
fn read_at(value: &Value, pointer: &str) -> Value {
    value.pointer(pointer).cloned().unwrap_or(Value::Null)
}

fn build_candidate(path: &str, requested: Option<LineRange>) -> Candidate {
    let lines = requested.unwrap_or(LineRange { start: 1, end: 1 });
    Candidate {
        file: Some(path.into()),
        text: "\n".repeat((lines.start..=lines.end).count()),
        range: Region {
            start: Position {
                line: lines.start.checked_sub(1).unwrap(),
                column: 0,
            },
            end: Position {
                line: lines.end,
                column: 0,
            },
            byte_offset: None,
        },
        context: None,
        supporting: Vec::new(),
        meta_variables: None,
    }
}

fn build_assessment(outcome: Outcome, confidence: f64) -> Assessment {
    Assessment {
        choice: outcome,
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

#[expect(
    clippy::too_many_lines,
    reason = "one table of output cases, checked case by case"
)]
#[test]
fn score_order_preserves_assessments_and_summary() {
    let mut runs = Vec::new();
    for sort in [Sort::None, Sort::Score] {
        let mut output = Output::new(Vec::new(), Format::Json, false, false, sort, 0.8);
        for (path, line, probability, confidence) in [
            ("a.rs", 10, 0.9_f64, 0.9_f64),
            ("d.rs", 1, 0.01_f64, 0.98_f64),
            ("c.rs", 1, 0.7_f64, 0.55_f64),
            ("b.rs", 1, 0.95_f64, 0.92_f64),
            ("a.rs", 2, 0.9_f64, 0.9_f64),
        ] {
            let choice = if probability > 0.5_f64 {
                Outcome::Match
            } else {
                Outcome::NoMatch
            };
            let mut answer = build_assessment(choice, confidence);
            answer.probabilities = BTreeMap::from([
                (Outcome::Match, probability),
                (Outcome::NoMatch, 1.0_f64 - probability),
                (Outcome::Uncertain, 0.0_f64),
            ]);
            output
                .scan_event(ScanEvent::Result {
                    candidate: build_candidate(
                        path,
                        Some(LineRange {
                            start: line,
                            end: line,
                        }),
                    ),
                    assessment: Box::new(answer),
                })
                .unwrap();
        }
        output
            .write_error(
                Some("failed.rs"),
                None,
                ErrorKind::Transport,
                "request failed",
            )
            .unwrap();
        assert_eq!(output.finish().unwrap(), ExitStatus::Error);
        let records = str::from_utf8(&output.writer)
            .unwrap()
            .lines()
            .map(serde_json::from_str::<Value>)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        runs.push(records);
    }
    let results = |run: usize| {
        runs.get(run)
            .expect("an index in range")
            .iter()
            .filter(|record| read_at(record, "/type") == "result")
            .collect::<Vec<_>>()
    };
    let streamed = results(0);
    assert_eq!(
        results(1),
        [3, 4, 0, 2, 1].map(|index| *streamed.get(index).expect("an index in range"))
    );
    assert_eq!(
        runs.first().unwrap().last(),
        runs.get(1).expect("the second record").last()
    );
    assert_eq!(
        read_at(
            runs.get(1).expect("the second record").first().unwrap(),
            "/type"
        ),
        "error"
    );
}

#[expect(
    clippy::too_many_lines,
    reason = "one table of output cases, checked case by case"
)]
#[test]
fn json_retains_uncertainty_and_errors_take_exit_precedence() {
    let mut output = Output::new(Vec::new(), Format::Json, false, false, Sort::None, 0.8);
    output
        .scan_event(ScanEvent::Result {
            candidate: build_candidate("a.py", Some(LineRange { start: 3, end: 9 })),
            assessment: Box::new(Assessment {
                choice: Outcome::NoMatch,
                confidence: 0.5,
                probabilities: BTreeMap::from([
                    (Outcome::NoMatch, 0.7_f64),
                    (Outcome::Match, 0.2_f64),
                    (Outcome::Uncertain, 0.1_f64),
                ]),
                model: "fixture".into(),
                request: Some(ev_grep_core::RequestInfo {
                    input_tokens: 1,
                    output_tokens: 1,
                    ..Default::default()
                }),
            }),
        })
        .unwrap();
    output
        .scan_event(ScanEvent::Error(FileError {
            path: "b.py".into(),
            lines: None,
            kind: ErrorKind::Timeout,
            message: "request failed".into(),
        }))
        .unwrap();
    assert_eq!(output.finish().unwrap(), ExitStatus::Error);
    let records: Vec<Value> = str::from_utf8(&output.writer)
        .unwrap()
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(read_at(records.first().unwrap(), "/data/file"), "a.py");
    assert_eq!(
        read_at(records.first().unwrap(), "/data/assessment/choice"),
        "no_match"
    );
    assert!(
        read_at(records.first().unwrap(), "/data/assessment")
            .get("outcome")
            .is_none()
    );
    assert_eq!(
        read_at(records.first().unwrap(), "/data/routing/outcome"),
        "uncertain"
    );
    assert_eq!(
        read_at(records.first().unwrap(), "/data/routing/reason"),
        "low_confidence"
    );
    assert_eq!(
        read_at(records.get(1).expect("the second record"), "/data/kind"),
        "timeout"
    );
    assert_eq!(
        read_at(records.get(2).expect("the third record"), "/type"),
        "summary"
    );
    assert_eq!(
        read_at(records.get(2).expect("the third record"), "/data/errors"),
        1_i32
    );
    assert_eq!(
        read_at(records.get(2).expect("the third record"), "/data/selected"),
        2_i32
    );
}

#[expect(
    clippy::too_many_lines,
    reason = "one table of output cases, checked case by case"
)]
#[test]
fn uncertainty_reports_the_model_choice_or_the_exact_routing_threshold() {
    for (choice, confidence, threshold, reason, explanation) in [
        (
            Outcome::Uncertain,
            0.99_f64,
            0.0_f64,
            "model_uncertain",
            "model chose uncertain",
        ),
        (
            Outcome::NoMatch,
            0.800_1_f64,
            0.800_2_f64,
            "low_confidence",
            "model chose no_match; confidence 0.8001 < 0.8002",
        ),
    ] {
        let answer = build_assessment(choice, confidence);
        let original = serde_json::to_value(&answer).unwrap();
        for format in [Format::Text, Format::Json] {
            let mut output = Output::new(Vec::new(), format, false, false, Sort::None, threshold);
            output
                .scan_event(ScanEvent::Result {
                    candidate: build_candidate("sample.py", None),
                    assessment: Box::new(answer.clone()),
                })
                .unwrap();
            assert_eq!(output.finish().unwrap(), ExitStatus::Uncertain);
            if format == Format::Text {
                assert!(
                    str::from_utf8(&output.writer)
                        .unwrap()
                        .contains(explanation)
                );
            } else {
                let first = str::from_utf8(&output.writer)
                    .unwrap()
                    .lines()
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("missing result"))
                    .unwrap();
                let record: Value = serde_json::from_str(first).unwrap();
                assert_eq!(read_at(&record, "/data/routing/reason"), reason);
                assert_eq!(
                    read_at(&record, "/data/assessment/choice"),
                    read_at(&original, "/choice")
                );
                assert_eq!(
                    read_at(&record, "/data/assessment/probabilities"),
                    read_at(&original, "/probabilities")
                );
            }
        }
        assert_eq!(serde_json::to_value(&answer).unwrap(), original);
    }
}
