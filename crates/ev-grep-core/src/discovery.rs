use alloc::collections::BTreeMap;
use std::{
    env,
    path::{Path, PathBuf},
};

use ignore::{
    WalkBuilder,
    overrides::{Override, OverrideBuilder},
};

use crate::{FileError, InputError, LineRange, Target};

/// The files a search selected, and the errors and symbolic links it found while selecting them.
#[derive(Debug, Default)]
pub struct Discovery {
    pub errors: Vec<FileError>,
    /// Symbolic links that directory searches found and did not follow, which callers report as
    /// skipped.
    pub symlinks: Vec<String>,
    pub targets: Vec<Target>,
}

type Selected = BTreeMap<(PathBuf, Option<LineRange>), Target>;

/// Selects the files that arguments name, filtered by globs.
///
/// Explicit roots bypass ignore files, while globs still filter every candidate. A directory that a
/// glob excludes is not read. A line range selects part of one regular file; the same file and
/// range is selected once, but different ranges are kept apart.
#[inline]
pub fn discover(arguments: &[String], globs: &[String]) -> Result<Discovery, InputError> {
    let cwd = env::current_dir().map_err(InputError::CurrentDirectory)?;
    let mut builder = OverrideBuilder::new(&cwd);
    for glob in globs {
        let _: &mut OverrideBuilder = builder.add(glob).map_err(|source| {
            InputError::Glob {
                glob: glob.clone(),
                source,
            }
        })?;
    }
    let overrides = builder.build().map_err(InputError::Globs)?;
    let mut result = Discovery::default();
    let mut selected = Selected::new();
    // Resolving a link would follow it, so overlapping roots name the same link by its path from
    // the current directory.
    let mut symlinks = BTreeMap::new();
    for argument in arguments {
        let target = match Target::parse(argument) {
            Ok(target) => target,
            Err(error) => {
                result.errors.push(FileError::new(argument, None, &error));
                continue;
            },
        };
        match target.lines {
            Some(lines) => {
                if !overrides.matched(&target.path, false).is_ignore() {
                    select_range(
                        &mut selected,
                        &mut result.errors,
                        argument,
                        &target.path,
                        lines,
                    );
                }
            },
            None => {
                walk(
                    &mut selected,
                    &mut result.errors,
                    &mut symlinks,
                    &cwd,
                    argument,
                    &target.path,
                    &overrides,
                );
            },
        }
    }
    result.targets = selected.into_values().collect();
    result.symlinks = symlinks.into_values().collect();
    Ok(result)
}

/// Selects a line range of one regular file that an argument names.
fn select_range(
    selected: &mut Selected,
    errors: &mut Vec<FileError>,
    argument: &str,
    path: &Path,
    lines: LineRange,
) {
    match path.symlink_metadata() {
        Ok(metadata) if metadata.is_file() => select(selected, errors, path, Some(lines)),
        Ok(_) => {
            errors.push(FileError::new(
                argument,
                None,
                &InputError::RangeNeedsRegularFile,
            ));
        },
        Err(error) => errors.push(FileError::new(argument, None, &InputError::Inspect(error))),
    }
}

/// Selects the regular files under a root that ignore rules and globs keep, and records the
/// symbolic links found below it.
#[expect(
    clippy::filetype_is_file,
    reason = "only regular files are selected; links and special files are reported"
)]
fn walk(
    selected: &mut Selected,
    errors: &mut Vec<FileError>,
    symlinks: &mut BTreeMap<PathBuf, String>,
    cwd: &Path,
    argument: &str,
    root: &Path,
    overrides: &Override,
) {
    // The walker's own `overrides` would let an including glob override ignore files, so
    // excluded directories are pruned here instead, after ignore rules.
    let excluded = overrides.clone();
    let walker = WalkBuilder::new(root)
        .follow_links(false)
        .filter_entry(move |entry| {
            !(entry.file_type().is_some_and(|kind| kind.is_dir())
                && excluded.matched(entry.path(), true).is_ignore())
        })
        .build();
    for walked in walker {
        let entry = match walked {
            Ok(found) => found,
            Err(error) => {
                errors.push(FileError::new(argument, None, &InputError::Walk(error)));
                continue;
            },
        };
        let path = entry.path();
        if entry.file_type().is_some_and(|kind| kind.is_dir())
            || overrides.matched(path, false).is_ignore()
        {
            continue;
        }
        if entry.file_type().is_some_and(|kind| kind.is_file()) {
            select(selected, errors, path, None);
            continue;
        }
        match (entry.depth(), entry.path_is_symlink()) {
            (0, _) => {
                errors.push(FileError::new(
                    &path.display().to_string(),
                    None,
                    &InputError::NotRegularFileOrDirectory,
                ));
            },
            (_, true) => drop(symlinks.insert(cwd.join(path), path.display().to_string())),
            (_, false) => {},
        }
    }
}

fn select(
    selected: &mut Selected,
    errors: &mut Vec<FileError>,
    path: &Path,
    lines: Option<LineRange>,
) {
    let label = path.display().to_string();
    if path.to_str().is_none() {
        errors.push(FileError::new(&label, lines, &InputError::PathNotUtf8));
        return;
    }
    match path.canonicalize() {
        Ok(identity) => {
            let _: &mut Target = selected.entry((identity, lines)).or_insert_with(|| {
                Target {
                    lines,
                    path: path.to_path_buf(),
                }
            });
        },
        Err(error) => errors.push(FileError::new(&label, lines, &InputError::Resolve(error))),
    }
}
