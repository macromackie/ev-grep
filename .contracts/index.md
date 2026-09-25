# Product contracts

These contracts govern the exported Rust workspace. Read in numbered order; conflicts require an explicit decision.

- [Execution outcomes](001-execution-outcomes/CONTRACT.md): Keep semantic uncertainty distinct from failed execution.

- [Crate boundaries](002-crate-boundaries/CONTRACT.md): Keep the search engine independent of providers and the terminal.

- [Provider boundaries](003-provider-boundaries/CONTRACT.md): Send bounded requests to the selected service without hidden routing.

- [Rust and checks](004-rust-and-checks/CONTRACT.md): Keep the Rust workspace readable and locally verifiable.
