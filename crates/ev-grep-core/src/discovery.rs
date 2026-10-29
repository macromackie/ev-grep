use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use ignore::{WalkBuilder, overrides::OverrideBuilder};

use crate::{Error, FileError, LineRange, Target};

#[derive(Debug, Default)]
pub struct Discovery {
    pub targets: Vec<Target>,
    /// Symbolic links that directory searches found and did not follow, which callers report as skipped.
    pub symlinks: Vec<String>,
    pub errors: Vec<FileError>,
}

type Selected = BTreeMap<(PathBuf, Option<LineRange>), Target>;

/// Explicit roots bypass ignore files, while globs still filter every candidate. A directory that a glob excludes is
/// not read. A line range selects part of one regular file; the same file and range is selected once, but different
/// ranges are kept apart.
pub fn discover(arguments: &[String], globs: &[String]) -> Result<Discovery, Error> {
    let cwd = std::env::current_dir().map_err(Error::CurrentDirectory)?;
    let mut overrides = OverrideBuilder::new(&cwd);
    for glob in globs {
        overrides.add(glob).map_err(|source| Error::Glob {
            glob: glob.clone(),
            source,
        })?;
    }
    let overrides = overrides.build().map_err(Error::Globs)?;
    let mut result = Discovery::default();
    let mut selected = Selected::new();
    // Resolving a link would follow it, so overlapping roots name the same link by its path from the current directory.
    let mut symlinks = BTreeMap::new();
    for argument in arguments {
        let target = match Target::parse(argument) {
            Ok(target) => target,
            Err(error) => {
                result.errors.push(FileError::new(argument, None, &error));
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
                    &Error::RangeNeedsRegularFile,
                )),
                Err(error) => {
                    result
                        .errors
                        .push(FileError::new(argument, None, &Error::Inspect(error)))
                }
            }
            continue;
        }
        // The walker's own `overrides` would let an including glob override ignore files, so excluded directories are
        // pruned here instead, after ignore rules.
        let excluded = overrides.clone();
        let walker = WalkBuilder::new(&root)
            .follow_links(false)
            .filter_entry(move |entry| {
                !(entry.file_type().is_some_and(|kind| kind.is_dir())
                    && excluded.matched(entry.path(), true).is_ignore())
            })
            .build();
        for entry in walker {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    result
                        .errors
                        .push(FileError::new(argument, None, &Error::Walk(error)));
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
                        &Error::NotRegularFileOrDirectory,
                    ));
                } else if entry.path_is_symlink() {
                    symlinks.insert(cwd.join(path), path.display().to_string());
                }
                continue;
            }
            select(&mut selected, &mut result.errors, path, None);
        }
    }
    result.targets = selected.into_values().collect();
    result.symlinks = symlinks.into_values().collect();
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
        errors.push(FileError::new(&label, lines, &Error::PathNotUtf8));
        return;
    }
    match path.canonicalize() {
        Ok(identity) => {
            selected.entry((identity, lines)).or_insert_with(|| Target {
                path: path.to_path_buf(),
                lines,
            });
        }
        Err(error) => errors.push(FileError::new(&label, lines, &Error::Resolve(error))),
    }
}
