# Concepts

A query describes behavior you want to find. ev-grep reads each selected file and asks Jev whether it matches.

```text
query: "Retries failed HTTP requests"

  ├── retry.ts   ── Jev ── match
  ├── view.ts    ── Jev ── no_match
  └── client.ts  ── Jev ── uncertain
```

Illustrative results. The same query accompanies each file; files do not share context.

## File context

An assessment receives the path and complete text of one file. A retry loop may be visible in `retry.ts`, while
`client.ts` delegates to a transport defined elsewhere. The latter may need a person or review agent to investigate.

ev-grep does not fetch dependencies, build an index, or compare revisions. Searching files from a PR examines their
current contents, not whether the diff introduced a problem.

## Decisions

`match` retains a file. `no_match` omits it from terminal output. `uncertain` retains it for further review.
All three appear in [JSON output](./output.md). A failed request is an error, not a decision.

Model results can be wrong. A search helps choose what to read; it does not prove that code follows a rule.

## Requests

File discovery and glob filtering happen locally. Each selected text file then becomes one provider request, with up
to four requests in flight. Use `--dry-run` to inspect file selection without a key or API calls.

Requests use your provider account and may incur charges. See [Providers](./providers.md) for configuration,
[Request limits](./limits.md) for bounds, and [Benchmarks](./benchmarks.md) for measured timings.
