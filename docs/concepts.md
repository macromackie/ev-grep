# Concepts

A query describes what you want to find. ev-grep asks Jev about each selected file, text range, or stdin candidate.

The same query accompanies each candidate. Candidates are assessed independently, even when they use the same
supporting sources.

## File context

An assessment receives the path and complete text of one file. If `client.ts` delegates to a transport in another
file, add that source with `--context src/transport.ts`. You can repeat the option or select a supporting line range.
Supporting sources can explain helpers and dependencies the target calls. Unrelated behavior in those sources cannot
make the target match.

ev-grep does not follow imports, fetch dependencies, build an index, or compare revisions. Searching files from a PR
examines their current contents, not whether the diff introduced a problem.

## Part of a file

A path such as `src/users.py:20-56` narrows an assessment to those lines. Jev still receives the whole file as
context and judges what the selected lines do, including what same-file helpers they call do. Code elsewhere in the
file that the lines don't use does not make them match. A range of changed lines asks what that code does now; it
still does not show whether the change introduced it. [Use with other tools](./pipelines.md) shows how to get ranges
from `git diff` or ast-grep.

## Syntax candidates

Use ast-grep to select functions, types, or other syntax, then pass its JSONL to `--candidates -`.
ev-grep assesses those exact regions. It does not generate matchers or ask the model to find line numbers.
Candidate records may supply the complete original file as `context` and other sources as `supporting` entries.
Paths in records are labels and are never opened. See [Search candidates](./cli.md#search-candidates) for the format.

## Decisions

The model returns a choice, confidence, and probabilities for `match`, `no_match`, and `uncertain`. It returns no
free-text explanation. The CLI separately routes answers below `--min-confidence` to `uncertain`; the default is 0.8.
An explicit uncertain choice stays uncertain at any threshold.

The routed outcome determines terminal output and exit codes. `match` retains a candidate, `no_match` omits it, and
`uncertain` retains it for review. [JSON output](./output.md) includes every raw assessment and its routing decision.
A failed request is an error, not a decision.

Model results can be wrong. A search helps choose what to read; it does not prove that code follows a rule.

## Requests

File discovery and glob filtering happen locally. Each selected text file then becomes one provider request, with up
to four requests in flight by default (`--jobs` changes the limit). Use `--dry-run` to inspect file selection without a key or API calls.

```text diagram
   selected files             up to 4 requests
  ┌──────────────┐       ┏━━━━━━━━━━━━━━━━━━━━━━┓
  │ file A       │──────▶┃ query + A  →  Jev    ┃
  │ file B       │──────▶┃ query + B  →  Jev    ┃
  │ file C       │──────▶┃ query + C  →  Jev    ┃
  │ file D       │──────▶┃ query + D  →  Jev    ┃
  │ file E, …    │       ┗━━━━━━━━━━┳━━━━━━━━━━━┛
  └──────────────┘                  │
       waiting                      ▼
                          results as they finish
```

When a request finishes, the next file takes its place. JSONL includes every result; terminal output lists matches
and uncertain files.

With `--sort score`, ev-grep waits for completion and orders results by match probability. Each assessment stays
independent; sorting does not ask Jev to choose one winner among matching candidates.

Requests use your provider account and may incur charges. See [Providers](./providers.md) for configuration,
[Request limits](./limits.md) for bounds, and [Benchmarks](./benchmarks.md) for measured timings.
