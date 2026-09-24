# Contributing

Athena is under active development. Changes are especially useful when they improve
NGSI-LD behavior with a reproducible protocol case, preserve durable history and
notification semantics, or quantify performance under a representative workload.

Before changing behavior, read the [implementation status](IMPLEMENTATION.md),
[API guide](docs/api.md) and [architecture decisions](docs/architecture/decisioni-2026-09-22.md).
The dated capability matrix is an initial audit, not a current source of truth.

## Development workflow

1. Make a focused change and document any API, migration or delivery-contract impact.
2. Add a meaningful regression test for a bug or behavior change. Integration tests
   must use an isolated `athena_test` database; never use a live broker volume.
3. Run `cargo fmt --all --check`, `cargo test --locked --workspace` and
   `cargo clippy --locked --workspace --all-targets`.
4. For storage, scheduler or MQTT changes, run the relevant isolated integration
   suite described in [development](docs/development.md).
5. Include the command, environment and observed result in the change description.
   For performance claims, report latency distribution, durability and queue drain;
   distinguish burst ingestion from sustained end-to-end throughput.

Do not edit an applied SQL migration: SQLx checksums protect deployed databases.
Add a new migration and describe upgrade/rollback constraints. Do not commit `.env`,
database dumps, logs, generated certificates, benchmark fixtures or secrets.

If you discover a security issue, avoid posting exploit details or credentials in a
public issue. Contact the repository owner privately first.
