# Benchmarks

Measured September 25, 2026 with ev-grep 0.1.4 on an Apple M5 Pro (48 GiB RAM, macOS 26.6.2, ARM64).
Rust 1.98.0, `cargo build --release --locked`, default release profile with thin LTO.

## Local

No model requests. Thirty measured runs after five warmups per case.

| Operation | Median | Range |
| --- | --- | --- |
| Startup (`--version`) | 3.2 ms | 2.7–4.1 ms |
| 100 files (`--dry-run`) | 7.0 ms | 6.1–9.7 ms |
| 1000 files (`--dry-run`) | 30.1 ms | 27.6–35.6 ms |

## Live search

OpenRouter, `typesafe/jev-1.13-20260917`, up to four concurrent requests. Five measured runs after one warmup per case.

| Files | Median | Range |
| --- | --- | --- |
| 1 | 220 ms | 204–232 ms |
| 4 | 256 ms | 210–279 ms |
| 16 | 690 ms | 616–1065 ms |

Times include startup, file reads, network requests, model responses, and captured JSONL output. All measured
searches completed without execution errors; all 105 measured file assessments returned `uncertain`. These are elapsed
search times, not isolated model inference times or timings for confirmed matches.

## Method

Every file contains this synthetic TypeScript function, padded to exactly 1,024 UTF-8 bytes with comments and spaces:

```typescript
export async function save(db, user) {
  return db.users.insert(user);
}
```

Files are named `corpus/file_0000.ts`, `file_0001.ts`, and so on. Each invocation uses a newly written corpus in a
temporary directory, so the filesystem cache is warm. Corpus creation and JSON validation are outside the timing.

```sh
ev-grep --version
ev-grep --provider openrouter --json 'Writes to the database' corpus/ --dry-run
ev-grep --provider openrouter --json 'Writes to the database' corpus/
```

Runs are sequential, without retries. Provider-side caching and other machine activity are uncontrolled. The samples
are too small for tail-latency claims, and repeated 1 KiB files do not represent a large repository. These measurements
do not test decision accuracy or compare ev-grep with other tools.

[Raw samples](https://ev-grep.com/benchmarks/v0.1.4.json) include warmups, measured runs, the exact fixture, model IDs,
and executable hash. See [Request limits](./limits.md) for file sizes, concurrency, and timeouts.
