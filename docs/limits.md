# Request limits

| Resource | Limit |
| --- | --- |
| Concurrent requests | 4 by default; `--jobs 1..256` |
| Connection timeout | 10 seconds |
| Assessment timeout, including retries | 60 seconds |
| File, stdin text, or candidate source including context | 64 KiB |
| Encoded candidate JSON line | 512 KiB |
| Query | 8 KiB |
| Encoded request | 96 KiB |
| Response | 64 KiB |

Exceeding a limit produces an error. Content is never silently truncated.

Transient HTTP errors (408, 429, 500, 502, 503, 504, 520, 522, 524) get up to two retries,
after one and two seconds. A numeric `Retry-After` replaces that delay; other values stop retries.
All attempts share the same 60-second budget and use the same provider, model, and input.
Retries can add charges when an upstream service processed a request before returning an error.
Authentication errors, transport failures, and invalid responses are not retried.
Final HTTP errors include request identifiers when available; provider response bodies are never printed.

A [line range](./cli.md#search-part-of-a-file) must lie within its file. A ranged request carries the whole file and a copy of the selected lines, so a large range in a large file can reach the encoded-request limit.

Binary detection checks the first 8 KiB before enforcing the file-size limit, then checks the rest of an eligible file. A large binary without an early NUL byte may therefore produce a size error.

Stdin and JSON candidates containing NUL bytes are errors. Empty stdin is one text candidate.

## Context

Each assessment sees the supplied candidate and any explicit file context. A question about architecture, relationships between files, or whether a diff introduced a bug may need more context than ev-grep provides.

Use `uncertain` results as a reason to investigate. A `no_match` is a model assessment, not proof that a codebase has no issues.
