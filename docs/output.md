# Output

Terminal output lists matching and uncertain locations. For example:

```text
src/users.py:20-56    92% match
src/actions.py:4-12   uncertain
```

The percentage is the model's probability for `match`. It is not the probability that a review is correct.
Nonmatches are omitted. Counts and errors go to stderr.

Use `--sort score` to put higher match probabilities first. Results wait until the search finishes; errors remain
immediate. Sorting preserves the model's scores and keeps uncertain results. It makes no extra model requests.

`--min-confidence` controls routing (default `0.8`). Below that provider confidence, an answer stays uncertain.
An explicit uncertain answer stays uncertain at any threshold. JSON preserves the original scores.

## JSON Lines

```sh
ev-grep 'Hides a database failure' src/ --json > results.jsonl
```

Every record has `schema_version: 2`, `type`, and `data`. A result has this shape (illustrative scores):

```json
{
  "schema_version": 2,
  "type": "result",
  "data": {
    "file": "src/users.py",
    "range": {
      "start": { "line": 19, "column": 0 },
      "end": { "line": 20, "column": 0 },
      "byteOffset": { "start": 380, "end": 394 }
    },
    "text": "    return []\n",
    "assessment": {
      "choice": "match",
      "confidence": 0.86,
      "probabilities": { "match": 0.92, "no_match": 0.03, "uncertain": 0.05 },
      "model": "typesafe/jev-1.13",
      "input_tokens": 280,
      "output_tokens": 12
    }
  }
}
```

Positions count from zero; columns count Unicode characters. The end is exclusive. Optional `byteOffset` counts
UTF-8 bytes. `file` is null for stdin. Optional `context` carries the complete file used to assess a range.
Results contain source text; store them as carefully as the source itself.

`probabilities` answers one question with three alternatives. Low match probability alone cannot distinguish a clear
negative from missing context. `confidence` is the provider's separate confidence measure. There is no routed
`outcome` or duplicate `match_probability` field in v2.

| Type | Data |
| --- | --- |
| `begin` | Provider, requested model, confidence threshold, dry-run flag, jobs, sort order |
| `selected` | Dry run: candidate `file`, `range`, `text`, optional `context`, and input `bytes` |
| `result` | Candidate fields and raw `assessment` |
| `skipped` | File, optional requested lines, and binary reason |
| `error` | File or input identifier, optional requested lines, and message |
| `summary` | Selected, evaluated, match, no-match, uncertain, skipped, and error counts; dry-run flag |

Results arrive as requests finish, or in descending match probability with `--sort score`. All successful assessments appear, including nonmatches. Completed runs end with
one summary. Interrupted runs may not. Argument parsing errors print usage and exit 2 before starting the stream.
Sorted runs buffer results, so interruption may leave only configuration, errors, and skips.

## Select by score

Keep candidates with at least 90% match probability:

```sh
jq -c 'select(.type == "result" and .data.assessment.probabilities.match >= 0.9) | .data' results.jsonl
```

For review, keep everything except confident negative answers:

```sh
jq -c 'select(.type == "result") | .data
  | select(.assessment.choice != "no_match" or .assessment.confidence < 0.9)' results.jsonl
```

These records can feed `--candidates`. Check the original exit status and summary before filtering: jq cannot recover
an error record you discarded. A score cutoff is a caller policy, not a completeness guarantee.

## Exit codes

The first matching row wins:

| Code | Meaning |
| --- | --- |
| `2` | An execution or argument error; earlier results may still be available |
| `3` | At least one answer routes to uncertain |
| `0` | At least one match, with no errors or uncertainty |
| `1` | No matches, with no errors or uncertainty; includes empty selections |

Dry runs return 0 for eligible text, 1 for an empty selection, and 2 for errors. They need no key and make no requests.

## SARIF

```sh
ev-grep 'Hides a database failure' src/ --sarif > ev-grep.sarif
```

`--sarif` writes one SARIF 2.1.0 log at completion. Matches become warnings, uncertain answers become notes, and
nonmatches are omitted. Results retain source locations and raw assessments. Stdin results have no file location.
Errors become execution notifications. Relative paths resolve from the working directory. Run from the repository root
when uploading to code scanning. An interrupted search may write no log. `--sarif` and `--json` are mutually exclusive.
