# Development and verification

The pinned toolchain is Rust 1.98.1 (`rust-toolchain.toml`); use Cargo with
`--locked` so checks use `Cargo.lock`. PostgreSQL 16 with PostGIS is required for
integration tests. The application is a Cargo workspace of six supporting crates
plus the API crate and executable; package roles are summarized in the
[README](../README.md).

## Routine checks

```bash
cargo fmt --all --check
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets
```

CI runs these checks and then exercises isolated PostgreSQL and MQTT integration
tests. Hosted CI results have not yet been used as production qualification.

## Isolated integration database

Integration tests require `ATHENA_TEST_DATABASE_URL` with database name
`athena_test`. They intentionally do not use the development broker's normal
`DATABASE_URL`. To create an ephemeral local fixture:

```bash
docker run --rm -d --name athena-integration-db \
  -e POSTGRES_HOST_AUTH_METHOD=trust -e POSTGRES_DB=athena_test \
  -p 127.0.0.1:55432:5432 postgis/postgis:16-3.4
bash scripts/test-integration.sh
docker stop athena-integration-db
```

The test database uses trust authentication **only on loopback**. Do not point tests
or benchmarks at a production database. The separate container should be stopped
after testing; the regular Compose deployment needs only `athena-broker` and
`athena-postgres`.

For MQTT interoperability, first start the Mosquitto fixture and temporary CA with
`bash scripts/start-mqtt-test.sh`, then run the ignored `mqtt_integration` suite
with `ATHENA_TEST_MQTT_CA="$PWD/artifacts/mqtt-test/ca.crt"`. Stop the
`athena-mqtt-integration` container when finished. Generated certificates and logs
live under ignored `artifacts/`.

## Performance tests

Benchmarks are opt-in and must run sequentially on the isolated fixture, ideally
with an otherwise idle host and database:

```bash
cargo test --locked --release --test iot_benchmark -- --ignored --nocapture
cargo test --locked --release --test subscription_benchmark -- --ignored --nocapture
cargo test --locked --release --test http_iot_benchmark -- --ignored --nocapture
```

Set `ATHENA_TEST_DATABASE_URL=postgres://postgres@127.0.0.1:55432/athena_test`
if using a different shell/session than `scripts/test-integration.sh`. Record CPU,
memory, database image, configuration, queue depth, p50/p95/p99 and whether the
notification queue fully drained. Burst ingestion is not sustained capacity.
The dated [benchmark report](architecture/scheduling-mqtt-2026-09-22.md) and
[measurement archive](architecture/scheduling-mqtt-benchmarks.json) include both
fast and regressed runs. The old capability matrix is a snapshot preceding later
changes and should not be presented as a current conformance score.
