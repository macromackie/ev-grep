use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use ignore::{WalkBuilder, overrides::OverrideBuilder};

use crate::{FileError, LineRange, Target};

#[derive(Debug, Default)]
pub struct Discovery {
    pub targets: Vec<Target>,
    pub errors: Vec<FileError>,
}

type Selected = BTreeMap<(PathBuf, Option<LineRange>), Target>;

/// Explicit roots bypass ignore files, while globs still filter every candidate. A line range selects part of
/// one regular file; the same file and range is selected once, but different ranges are kept apart.
pub fn discover(arguments: &[String], globs: &[String]) -> Result<Discovery> {
    let cwd = std::env::current_dir()?;
    let mut overrides = OverrideBuilder::new(&cwd);
    for glob in globs {
        overrides
            .add(glob)
            .with_context(|| format!("invalid glob: {glob}"))?;
    }
    let overrides = overrides.build()?;
    let mut result = Discovery::default();
    let mut selected = Selected::new();
    for argument in arguments {
        let target = match Target::parse(argument) {
            Ok(target) => target,
            Err(error) => {
                result.errors.push(FileError::new(argument, None, error));
                continue;
            }
        };
        let root = target.path;
        if let Some(lines) = target.lines {
            if overrides.matched(&root, false).is_ignore() {
                continue;
            }
            match root.symlink_metadata() {
                Ok(metadata) if metadata.is_file() => {
                    select(&mut selected, &mut result.errors, &root, Some(lines))
                }
                Ok(_) => result.errors.push(FileError::new(
                    argument,
                    None,
                    "line ranges need a regular file; directories and symbolic links are not supported",
                )),
                Err(error) => result.errors.push(FileError::new(argument, None, error)),
            }
            continue;
        }
        let walker = WalkBuilder::new(&root).follow_links(false).build();
        for entry in walker {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    result.errors.push(FileError::new(argument, None, error));
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
                    result.errors.push(FileError::new(
                        &path.display().to_string(),
                        None,
                        "expected a regular file or directory; symbolic links are not followed",
                    ));
                }
                continue;
            }
            select(&mut selected, &mut result.errors, path, None);
        }
    }
    result.targets = selected.into_values().collect();
    Ok(result)
}

fn select(
    selected: &mut Selected,
    errors: &mut Vec<FileError>,
    path: &Path,
    lines: Option<LineRange>,
) {
    let label = path.display().to_string();
    if path.to_str().is_none() {
        errors.push(FileError::new(
            &label,
            lines,
            "file path is not valid UTF-8",
        ));
        return;
    }
    match path.canonicalize() {
        Ok(identity) => {
            selected.entry((identity, lines)).or_insert_with(|| Target {
                path: path.to_path_buf(),
                lines,
            });
        }
        Err(error) => errors.push(FileError::new(&label, lines, error)),
    }
}
