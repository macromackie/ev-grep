# Overview

ev-grep finds source files from a description of their behavior. Use it to locate an implementation or narrow a code review.

[Install ev-grep and set an API key](./installation.md) before running these searches.

## Find an implementation

Locate retry logic without knowing the function names:

```sh
ev-grep 'Retries failed HTTP requests' src/
```

## Check error handling

Find code that hides a database failure:

```sh
ev-grep 'Catches a database failure and returns an empty result' src/
```

Illustrative terminal output:

```text
src/users.py     92% match
src/actions.py   uncertain (model chose uncertain)
```

Open the matching files to check them. Keep uncertain files in the review: they may need more context. Nonmatches are omitted from terminal output; `--json` includes every assessment.

## Narrow a review

Look for database writes in a few files you're reviewing, or in the lines that changed:

```sh
ev-grep 'Writes to the database' src/users.py src/actions.py:40-72
```

Each assessment reads the full file, even when a range narrows it. It does not compare revisions or establish whether a change introduced a problem. [Use with other tools](./pipelines.md) gets changed lines from `git diff`.

If the selected code calls a helper in another file, attach it explicitly:

```sh
ev-grep 'Writes to the database' src/users.py:40-72 --context src/storage.py
```

Repeat `--context` to add sources, or select supporting ranges with `PATH:N-M`. ev-grep does not follow imports.

## Preview what gets sent

```sh
ev-grep 'Retries failed HTTP requests' src/ --dry-run
```

A dry run lists candidates and source sizes without making requests. During a search, the query, candidate source,
and any supporting sources go to Jev through your chosen provider. Requests use your API key and may incur charges.

See [Concepts](./concepts.md) for file context and decisions, [CLI](./cli.md) for filters, ranges, and query files, [Use with other tools](./pipelines.md) for pipelines, or [Output](./output.md) for JSON and exit codes. Model results can be wrong; use them to decide where to look.
