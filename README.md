# ev-grep

Find files by meaning. Ask a question about each complete file; get `match`, `no_match`, or `uncertain`.

```sh
ev-grep 'Performs database operations' src/
ev-grep -f query.txt src/ --glob '*.rs'
ev-grep --provider typesafe --model jev-1.13.0 'Hides database failures' src/users.py --json
ev-grep -f - src/ --dry-run < query.txt
```

Set `OPENROUTER_API_KEY` for the default OpenRouter connection, or select `--provider typesafe` and set
`TYPESAFE_API_KEY`. Source files are sent to the selected provider. No requests occur during `--dry-run`.

The whole query file is one multiline question. With `-f`, all positional arguments are search paths.
Without `-f`, the first positional argument is the query. Paths default to the current directory.

This first release evaluates files independently. It does not retrieve helpers, run tests, or determine whether a
change introduced a bug. An uncertain result preserves missing context or low confidence. A no-match result is a
model assessment, not a proof of correctness. Tenet can orchestrate applicability and verification calls separately.

## Install

Download a prebuilt binary for macOS or Linux (Apple Silicon/ARM64 and x86-64):

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://ev-grep.com/install.sh | sh
```

Or pin the release with mise:

```sh
mise use -g github:macromackie/ev-grep@0.1.0
```

See [releases](https://github.com/macromackie/ev-grep/releases) for archives and SHA-256 checksums.
The installer puts the binary in `~/.local/bin`.

## Build and check

Install [mise](https://mise.jdx.dev/), then:

```sh
mise trust
mise install
mise run check
mise run build
cargo install --path crates/ev-grep --locked
```

The pinned Rust toolchain includes rustfmt and Clippy. Package-local mise tasks are available in `crates/*`.
Builds produce `target/release/ev-grep`; `cargo install` is optional. Crates.io publishing is not configured.

- [CLI, limits, exit codes, and JSONL](docs/cli.md)
- [Offline tests and live evals](docs/evaluation.md)
- [Contributor contracts](.contracts/index.md)

Released under the [MIT license](LICENSE).
