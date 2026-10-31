use ev_grep_core::{Candidate, Source};

pub(in crate::client::tests) fn build_cached_source() -> Candidate {
    Candidate::from_source(Source {
        path: "cache.ts".into(),
        text: "return cached;".into(),
        focus: None,
    })
    .unwrap()
}
