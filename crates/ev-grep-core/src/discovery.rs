use std::{collections::BTreeMap, path::PathBuf};

use anyhow::{Context, Result};
use ignore::{WalkBuilder, overrides::OverrideBuilder};

use crate::FileError;

#[derive(Debug, Default)]
pub struct Discovery {
    pub paths: Vec<PathBuf>,
    pub errors: Vec<FileError>,
}

/// Explicit roots bypass ignore files, while globs still filter every candidate.
pub fn discover(roots: &[PathBuf], globs: &[String]) -> Result<Discovery> {
    let cwd = std::env::current_dir()?;
    let mut overrides = OverrideBuilder::new(&cwd);
    for glob in globs {
        overrides
            .add(glob)
            .with_context(|| format!("invalid glob: {glob}"))?;
    }
    let overrides = overrides.build()?;
    let mut result = Discovery::default();
    let mut paths = BTreeMap::new();
    for root in roots {
        let walker = WalkBuilder::new(root).follow_links(false).build();
        for entry in walker {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    result.errors.push(FileError {
                        path: root.display().to_string(),
                        message: error.to_string(),
                    });
                    continue;
                }
            };
            let path = entry.path();
            if entry.file_type().is_some_and(|kind| kind.is_dir()) {
                continue;
            }
            if overrides.matched(path, false).is_ignore() {
                continue;
            }
            if !entry.file_type().is_some_and(|kind| kind.is_file()) {
                if entry.depth() == 0 {
                    result.errors.push(FileError {
                        path: path.display().to_string(),
                        message:
                            "expected a regular file or directory; symbolic links are not followed"
                                .into(),
                    });
                }
                continue;
            }
            if path.to_str().is_none() {
                result.errors.push(FileError {
                    path: path.display().to_string(),
                    message: "file path is not valid UTF-8".into(),
                });
                continue;
            }
            match path.canonicalize() {
                Ok(identity) => {
                    paths.entry(identity).or_insert_with(|| path.to_path_buf());
                }
                Err(error) => result.errors.push(FileError {
                    path: path.display().to_string(),
                    message: error.to_string(),
                }),
            }
        }
    }
    result.paths = paths.into_values().collect();
    Ok(result)
}
