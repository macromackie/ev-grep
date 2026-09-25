# Working in ev-grep

Read [.contracts](.contracts/index.md) before changing code. Keep contract changes explicit and consistent with behavior.
Run `mise run check` from this workspace; each crate also supports `mise run test`, `lint`, `typecheck`, and `fmt`.
The pinned Rust toolchain supplies Cargo, rustfmt, and Clippy. Checks run offline after dependencies are downloaded.
Live model evals are separate and require explicit credentials; see [evaluation](docs/evaluation.md).

Keep one implementation path across the CLI, tests, and live evaluations. Use one agent unless delegation is requested.
