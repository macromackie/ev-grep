# ev-grep

A command-line search tool for finding code by behavior.

## Install

Download a binary for macOS or Linux, on ARM64 or x86-64:

```sh
curl -fsSL https://ev-grep.com/install.sh | bash
```

The installer puts `ev-grep` in `~/.local/bin`. Add that directory to your `PATH` if needed.
You can also install a specific release with [mise](https://mise.jdx.dev/):

```sh
mise use -g github:macromackie/ev-grep@0.8.2
```

Archives and checksums are on the [releases page](https://github.com/macromackie/ev-grep/releases).

## Search

Set an OpenRouter API key, then describe what you want to find:

```sh
export OPENROUTER_API_KEY='your-key'
ev-grep 'Catches a database failure and returns an empty result' src/
```

Queries and supplied source or state data go to the provider. Requests use your account and may incur charges.
Use `--dry-run` to inspect file selection without an API key or network requests:

```sh
ev-grep 'Catches a database failure and returns an empty result' src/ --dry-run
```

Add `:N-M` to a file path to assess only those lines, such as lines a branch changed:
`ev-grep 'Writes to the database' src/users.py:40-72`. The [pipelines guide](docs/pipelines.md) gets paths and ranges
from `rg`, `git diff`, and ast-grep.

Supply helpers or dependencies explicitly when the selected code needs them:

```sh
ev-grep 'Retries failed requests' src/client.ts:20-45 --context src/transport.ts
```

Repeat `--context PATH[:N-M]` to attach more supporting files or line ranges to each candidate.

Ask a question about a JSON value with `--state FILE`, or read it from stdin with `--state -`:

```sh
printf '%s\n' '{"job":{"status":"failed","attempts":3}}' \
  | ev-grep 'Is the job marked failed?' --state - --json
```

State mode accepts one JSON value up to 64 KiB and reads no paths mentioned inside it. Add `--dry-run` to inspect the
input without credentials or requests. The [CLI guide](docs/cli.md#assess-json-state) covers state input and output.

## Results

Example output, showing only stdout:

```text
src/users.py     92% match
src/actions.py   uncertain (model chose uncertain)
```

`match` means the model judges that the file matches the query. `uncertain` means the model chose uncertain or its
confidence fell below `--min-confidence` (default 0.8). Terminal output states which case occurred.
Terminal output omits `no_match` files and writes a count summary to stderr. State mode prints every answer, including
`no_match`. Use `--json` to retain every raw assessment and its routing decision.

Put higher match probabilities first with `--sort score`:

```sh
ev-grep 'Retries failed requests with a delay' src/ --sort score
```

Sorting waits for the search to finish and makes no additional model requests. The default streams results as they arrive.

Search a pipe with `cat src/users.py | ev-grep 'Hides a database failure' --stdin`.
For syntax selections, pass ast-grep's JSON Lines directly:

```sh
ast-grep run --kind function_declaration --lang ts --json=stream src/ \
  | ev-grep 'Retries a failed request' --candidates - --json
```

JSON results contain exact source text and ranges, raw model answers, and separate routing decisions.
Candidate records retain optional ast-grep `metaVariables` as selection metadata, excluded from model input.
The [output guide](docs/output.md) shows score filters and completeness checks.

Each candidate is evaluated on its own with its supplied supporting sources. ev-grep does not follow imports or run
the code. Results can be wrong; use them to decide what to inspect. Request failures are reported as errors, not as
`no_match`.

See the [documentation](docs/README.md) for file filters, provider configuration, exit codes and JSONL output.
The same pages are available at [ev-grep.com/docs](https://ev-grep.com/docs).

## Build from source

Install [Rust](https://rustup.rs/), then run these commands from a source checkout:

```sh
cargo build --release --locked -p ev-grep
cargo install --path crates/ev-grep --locked
```

The toolchain file selects the Rust version. The build produces `target/release/ev-grep`; installation is optional.

[package-boundaries.json](package-boundaries.json) declares the search, provider, and CLI crates and their allowed dependencies.
Check that Cargo policy with the released checker using Cargo, Python 3.14 or later, and uv:

```sh
shasum -a 256 --check .checks/package-boundaries/SHA256SUMS
uv run --locked --script .checks/package-boundaries/check-package-boundaries.py --root . --config package-boundaries.json
```

The bundled guide's npm resolver setup applies when a policy includes JavaScript or TypeScript packages.

Released under the [MIT license](LICENSE).
