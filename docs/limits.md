# Request limits

| Resource | Limit |
| --- | --- |
| Concurrent requests | 4 by default; `--jobs 1..256` |
| Individual attempt timeout | 5 seconds |
| Assessment timeout, including retries | 15 seconds |
| File, stdin text, or candidate source including context | 64 KiB |
| Encoded candidate JSON line | 512 KiB |
| Query | 8 KiB |
| Encoded request | 96 KiB |
| Response | 64 KiB |

Exceeding a limit produces an error. Content is never silently truncated.

Transient connection, body-read, and HTTP errors get up to three attempts within one 15-second budget.
After one second, ev-grep may start one duplicate request if the shared `--jobs` pool has spare capacity.
The first valid response wins, including `uncertain`; remaining attempts are cancelled.
Rate limits pause new requests. Retry delays include jitter and honor `Retry-After` seconds or dates.
A response that fails validation, such as probabilities that do not sum to one, gets the same bounded attempts; it never
becomes a result. Authentication errors are not retried. All attempts use the same input, provider, and model.

TypeSafe publishes per-account limits of 40 requests and 100K input tokens per second and adjusts them with demand.
Function candidates with file context resend the whole file for each function, so a high `--jobs` value can reach the
token limit. If searches report HTTP 429, lower `--jobs`.

Duplicate or interrupted requests may still incur charges. JSON request metadata reports attempts, hedges,
elapsed time, and usage from the winning response; it is not a complete billing record.
Final HTTP errors include safe request identifiers when available. Provider response bodies are never printed.

A [line range](./cli.md#search-part-of-a-file) must lie within its file. A ranged request carries the whole file and a copy of the selected lines, so a large range in a large file can reach the encoded-request limit.

Binary detection checks the first 8 KiB before enforcing the file-size limit, then checks the rest of an eligible file. A large binary without an early NUL byte may therefore produce a size error.

Stdin and JSON candidates containing NUL bytes are errors. Empty stdin is one text candidate.

## Context

Each assessment sees the supplied candidate and any explicit file context. A question about architecture, relationships between files, or whether a diff introduced a bug may need more context than ev-grep provides.

Use `uncertain` results as a reason to investigate. A `no_match` is a model assessment, not proof that a codebase has no issues.
