# Request limits

| Resource | Limit |
| --- | --- |
| Concurrent requests | 4 by default; `--jobs 1..256` |
| Connection timeout | 10 seconds |
| Full request timeout | 60 seconds |
| File | 64 KiB |
| Query | 8 KiB |
| Encoded request | 96 KiB |
| Response | 64 KiB |

Exceeding a limit produces an error. Content is never silently truncated.

A [line range](./cli.md#search-part-of-a-file) must lie within its file. A ranged request carries the whole file and a copy of the selected lines, so a large range in a large file can reach the encoded-request limit.

Binary detection checks the first 8 KiB before enforcing the file-size limit, then checks the rest of an eligible file. A large binary without an early NUL byte may therefore produce a size error.

## Context

Each assessment sees one file, or part of one with the rest of that file as context. A question about architecture, relationships between files, or whether a diff introduced a bug may need more context than ev-grep provides.

Use `uncertain` results as a reason to investigate. A `no_match` is a model assessment, not proof that a codebase has no issues.
