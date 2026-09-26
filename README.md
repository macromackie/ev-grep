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
mise use -g github:macromackie/ev-grep@0.2.0
```

Archives and checksums are on the [releases page](https://github.com/macromackie/ev-grep/releases).

## Search

Set an OpenRouter API key, then describe what you want to find:

```sh
export OPENROUTER_API_KEY='your-key'
ev-grep 'Catches a database failure and returns an empty result' src/
```

Queries and selected file contents go to the provider. Requests use your account and may incur charges.
Use `--dry-run` to inspect file selection without an API key or network requests:

```sh
ev-grep 'Catches a database failure and returns an empty result' src/ --dry-run
```

Add `:N-M` to a file path to assess only those lines, such as lines a branch changed:
`ev-grep 'Writes to the database' src/users.py:40-72`. The [pipelines guide](docs/pipelines.md) gets paths and ranges
from `rg`, `git diff`, and ast-grep.

## Results

Example output, showing only stdout:

```text
src/users.py     match
src/actions.py   uncertain
```

`match` means the model judges that the file matches the query. `uncertain` means it lacks enough context or confidence.
Terminal output omits `no_match` files and writes a count summary to stderr. Use `--json` to include every assessment.
Use `--sarif` to write a SARIF log for code scanning tools such as GitHub's.

Each file is evaluated on its own. ev-grep does not read related files for context or run the code. Results can be
wrong; use them to decide what to inspect. Request failures are reported as errors, not as `no_match`.

See the [documentation](docs/README.md) for file filters, provider configuration, exit codes, JSONL output, and SARIF output.
The same pages are available at [ev-grep.com/docs](https://ev-grep.com/docs).

## Build from source

Install [Rust](https://rustup.rs/), then run these commands from a source checkout:

```sh
cargo build --release --locked -p ev-grep
cargo install --path crates/ev-grep --locked
```

The toolchain file selects the Rust version. The build produces `target/release/ev-grep`; installation is optional.

Released under the [MIT license](LICENSE).
