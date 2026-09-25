use std::{collections::BTreeMap, fs, path::PathBuf};

use anyhow::Result;
use ev_grep_core::{
    Assessment, Evaluator, MAX_FILE_BYTES, Outcome, ScanEvent, Source, SourceRead, discover,
    read_source, scan,
};

#[test]
fn discovery_respects_scope_and_deduplicates_explicit_files() -> Result<()> {
    let dir = tempfile::tempdir()?;
    fs::create_dir(dir.path().join(".git"))?;
    fs::write(dir.path().join(".gitignore"), "ignored.py\n")?;
    fs::write(dir.path().join("ignored.py"), "ignored")?;
    fs::write(dir.path().join("selected.py"), "selected")?;
    fs::write(dir.path().join("other.rs"), "other")?;
    let roots = vec![dir.path().to_path_buf(), dir.path().join("selected.py")];
    let files = discover(&roots, &["*.py".into()])?;
    assert!(files.errors.is_empty());
    assert_eq!(files.paths.len(), 1);
    assert!(files.paths[0].ends_with("selected.py"));
    assert_eq!(
        discover(&[dir.path().join("ignored.py")], &[])?.paths.len(),
        1
    );
    assert!(
        !discover(&[dir.path().join("missing")], &[])?
            .errors
            .is_empty()
    );
    Ok(())
}

struct Fixture;
impl Evaluator for Fixture {
    async fn assess(&self, _: &str, _: &Source) -> Result<Assessment> {
        Ok(Assessment {
            outcome: Outcome::NoMatch,
            reason: None,
            choice: Outcome::NoMatch,
            confidence: 1.0,
            probabilities: BTreeMap::from([(Outcome::NoMatch, 1.0)]),
            model: "fixture".into(),
            input_tokens: 0,
            output_tokens: 0,
        })
    }
}

#[tokio::test]
async fn scan_reports_every_file_without_truncating_or_hiding_failures() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let paths: Vec<PathBuf> = ["text", "binary", "large", "missing"]
        .map(|p| dir.path().join(p))
        .to_vec();
    fs::write(&paths[0], "hello")?;
    fs::write(&paths[1], b"\0\x01")?;
    fs::write(&paths[2], vec![b'x'; MAX_FILE_BYTES + 1])?;
    assert!(matches!(read_source(&paths[1])?, SourceRead::Binary));
    let mut counts = [0; 3];
    scan(paths, "query", &Fixture, |event| {
        match event {
            ScanEvent::Result { assessment, .. } => {
                assert_eq!(assessment.outcome, Outcome::NoMatch);
                counts[0] += 1;
            }
            ScanEvent::Skipped { .. } => counts[1] += 1,
            ScanEvent::Error(_) => counts[2] += 1,
        }
        Ok(())
    })
    .await?;
    assert_eq!(counts, [1, 1, 2]);
    Ok(())
}
