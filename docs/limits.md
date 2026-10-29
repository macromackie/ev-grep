# Request limits

| Resource | Limit |
| --- | --- |
| Concurrent requests | 4 by default; `--jobs 1..256` |
| Individual attempt timeout | 5 seconds |
| Assessment timeout, including retries | 15 seconds |
| Each source file read from disk | 64 KiB |
| Candidate original source plus all supporting text | 64 KiB |
| Encoded candidate JSON line | 512 KiB |
| Query | 8 KiB |
| Encoded request | 96 KiB |
| Response | 64 KiB |

A searched file over 64 KiB is [skipped](./cli.md#skip-large-files) as `too_large` and counted in the summary; it
exits 2 only with `--strict`. A `--context` or `--state` file over 64 KiB, or exceeding any other limit, produces an
error. Content is never silently truncated.

The combined source limit counts `context` when present, otherwise candidate `text`, plus the `text` of every
`supporting` entry. A supporting line range sends only those lines, but the complete file must pass the source-file
limit before selection. The separate encoded-request limit includes the query, labels, and JSON encoding.

Transient connection, body-read, and HTTP errors get up to three attempts within one 15-second budget.
After one second, ev-grep may start one duplicate request if the shared `--jobs` pool has spare capacity.
The first valid response wins, including `uncertain`; remaining attempts are cancelled.
Rate limits pause new requests. Retry delays include jitter and honor `Retry-After` seconds or dates.
When a rate limit would end after an assessment's 15-second budget, the assessment fails at once with a
`rate_limited` error instead of waiting, and the search continues. A `Retry-After: 20` reply fails the files
queued behind it without sending more requests.
A response that fails validation, such as probabilities that do not sum to one, gets the same bounded attempts; it never
becomes a result. Authentication errors, TLS failures, and a response from a different model are not retried. All
attempts use the same input, provider, and model.

HTTP 401, 402, and a 403 that names no moderation reason stop the search with one `credential_rejected` error. It
names the provider and its key variable, such as `OPENROUTER_API_KEY`. No request starts after the first one, so a
search with a key rejected from the start sends at most `--jobs` requests. A 403 that names moderation fails only its
file.

TypeSafe publishes per-account limits of 40 requests and 100K input tokens per second and adjusts them with demand.
Function candidates with file context resend the whole file and supporting sources for each function. A high `--jobs`
value can reach the token limit. If searches report HTTP 429 or `rate_limited`, lower `--jobs`.

Duplicate or interrupted requests may still incur charges. JSON request metadata reports attempts, hedges,
elapsed time, and usage from the winning response; it is not a complete billing record.
Final HTTP errors include safe request identifiers when available. For a 401, 402, or 403, ev-grep reads the body only
for the provider's error code and moderation reason codes, and reports them. Provider messages and other response text
are never printed. A transport error names its cause: `timeout`, `connect`, `tls`, or
`transport`.

A [line range](./cli.md#search-part-of-a-file) must lie within its file. A ranged request carries the whole file and a copy of the selected lines, so a large range in a large file can reach the encoded-request limit.

Binary detection checks the first 8 KiB before enforcing the file-size limit, then checks the rest of an eligible file. A large binary without an early NUL byte may therefore be skipped as `too_large` rather than `binary`.

Stdin and JSON candidates containing NUL bytes are errors. Empty stdin is one text candidate.

## Context

Each assessment sees the supplied candidate, its original file context when provided, and any explicit supporting
sources. ev-grep does not follow imports. A question about architecture or whether a diff introduced a bug may need
more evidence than the supplied sources provide.

Use `uncertain` results as a reason to investigate. A `no_match` is a model assessment, not proof that a codebase has no issues.
