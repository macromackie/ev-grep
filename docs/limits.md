# Request limits

| Resource | Limit |
| --- | --- |
| Concurrent requests | 4 |
| Connection timeout | 10 seconds |
| Full request timeout | 60 seconds |
| File | 64 KiB |
| Query | 8 KiB |
| Encoded request | 96 KiB |
| Response | 64 KiB |

Exceeding a limit produces an error. Content is never silently truncated.

Binary detection checks the first 8 KiB before enforcing the file-size limit, then checks the rest of an eligible file. A large binary without an early NUL byte may therefore produce a size error.

## Context

Each assessment sees one file. A question about architecture, relationships between files, or whether a diff introduced a bug may need more context than ev-grep provides.

Use `uncertain` results as a reason to investigate. A `no_match` is a model assessment, not proof that a codebase has no issues.
