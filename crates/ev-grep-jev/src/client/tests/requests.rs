use ev_grep_core::{Candidate, Focus, LineRange, Source, SupportingSource};
use serde_json::{Map, json};

use super::support::read_at;
use crate::{JevError, protocol};

/// `users.py` with its `save` function in focus.
fn build_users_source() -> Candidate {
    Candidate::from_source(Source {
        path: "users.py".into(),
        text: "import db\n\ndef save(user):\n    db.insert(user)\n".into(),
        focus: Some(Focus {
            lines: LineRange { end: 4, start: 3 },
            text: "def save(user):\n    db.insert(user)\n".into(),
        }),
    })
    .unwrap()
}

#[test]
fn embedded_prompt_fills_the_request() {
    let prompt = protocol::prompt().unwrap();
    let source = Candidate::from_source(Source {
        path: "retry.ts".into(),
        text: "retry()\n".into(),
        focus: None,
    })
    .unwrap();
    let request = protocol::request("fixture", &prompt, "Retries requests", &source);
    // A whole-file search sends only the path and source, with no focus rule.
    assert_eq!(
        read_at(&request, "/state").as_object().map(Map::len),
        Some(2)
    );
    let question = read_at(&request, "/questions/match");
    assert_eq!(
        read_at(question, "/instructions").as_object().map(Map::len),
        Some(2)
    );
    assert_eq!(read_at(question, "/instructions/task"), &json!(prompt.task));
    assert_eq!(read_at(question, "/instructions/query"), "Retries requests");
    assert_eq!(
        read_at(question, "/criteria/match"),
        &json!(prompt.criteria.matches)
    );
    assert_eq!(
        read_at(question, "/criteria/no_match"),
        &json!(prompt.criteria.no_match)
    );
    assert_eq!(
        read_at(question, "/criteria/uncertain"),
        &json!(prompt.criteria.uncertain)
    );
    assert_eq!(
        read_at(question, "/criteria").as_object().map(Map::len),
        Some(3)
    );
}

#[test]
fn incomplete_prompts_are_rejected() {
    for invalid in [
        concat!(
            r#"{"supporting": "s", "task": "t", "focus": "f", "#,
            r#""criteria": {"match": "m", "no_match": "n"}}"#,
        ),
        concat!(
            r#"{"supporting": "s", "task": "t", "focus": "f", "#,
            r#""criteria": {"match": "m", "no_match": "n", "uncertain": "u", "other": "o"}}"#,
        ),
        concat!(
            r#"{"supporting": "s", "task": " ", "focus": "f", "#,
            r#""criteria": {"match": "m", "no_match": "n", "uncertain": "u"}}"#,
        ),
        concat!(
            r#"{"supporting": "s", "task": "t", "#,
            r#""criteria": {"match": "m", "no_match": "n", "uncertain": "u"}}"#,
        ),
    ] {
        assert!(
            matches!(
                protocol::parse_prompt(invalid),
                Err(JevError::Prompt(_) | JevError::EmptyPrompt)
            ),
            "{invalid:?}"
        );
    }
}

#[test]
fn ranged_request_sends_the_whole_file_and_the_focus_lines() {
    let prompt = protocol::prompt().unwrap();
    let source = build_users_source();
    let request = protocol::request("fixture", &prompt, "Writes to the database", &source);
    assert_eq!(
        read_at(&request, "/state/source").as_str(),
        source.context.as_deref()
    );
    assert_eq!(read_at(&request, "/state/focus/start_line"), 3_u64);
    assert_eq!(read_at(&request, "/state/focus/end_line"), 4_u64);
    assert_eq!(
        read_at(&request, "/state/focus/text"),
        "def save(user):\n    db.insert(user)\n"
    );
    assert_eq!(
        read_at(&request, "/questions/match/instructions/focus"),
        &json!(prompt.focus)
    );
}

#[test]
fn supporting_sources_join_the_ranged_request() {
    let prompt = protocol::prompt().unwrap();
    let mut source = build_users_source();
    let request = protocol::request("fixture", &prompt, "Writes to the database", &source);
    source.supporting.push(
        SupportingSource::from_source(Source {
            path: "db.py".into(),
            text: "unrelated()\ndef insert(user): write(user)\n".into(),
            focus: Some(Focus {
                lines: LineRange { end: 2, start: 2 },
                text: "def insert(user): write(user)\n".into(),
            }),
        })
        .unwrap(),
    );
    let supported = protocol::request("fixture", &prompt, "Writes to the database", &source);
    assert_eq!(
        read_at(&supported, "/state/focus"),
        read_at(&request, "/state/focus")
    );
    assert_eq!(
        read_at(&supported, "/state/source"),
        read_at(&request, "/state/source")
    );
    assert_eq!(
        read_at(&supported, "/state/supporting/0/text"),
        "def insert(user): write(user)\n"
    );
    assert_eq!(
        read_at(&supported, "/state/supporting/0/range/start/line"),
        1_u64
    );
    assert_eq!(
        read_at(&supported, "/questions/match/instructions/supporting"),
        &json!(prompt.supporting)
    );
}
