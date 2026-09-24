# Athena NGSI-LD Broker

Athena is an experimental NGSI-LD context broker written in Rust. It uses Axum and
Tokio for HTTP processing and PostgreSQL/PostGIS for entity state, temporal history
and durable notification queues. Development prioritizes high-volume IoT writes,
history and HTTP/MQTT notifications. The specification reference is ETSI GS CIM 009
V1.9.1. **Full NGSI-LD conformance and superiority to other brokers have not been
established.** See [implementation status](IMPLEMENTATION.md) before evaluating it for
production.

## Quick start (local development)

Install Docker with the Compose plugin, then run from the repository root:

```bash
cp .env.example .env
# Set POSTGRES_PASSWORD in .env to a long, unique alphanumeric password.
docker compose up --build -d
curl --fail http://localhost:8080/ready
```

Open the [web explorer](http://localhost:8080/ui) to navigate and try the API. The
root path `/` serves the same page. Compose starts **one broker container** and its
required PostgreSQL/PostGIS container; PostgreSQL is bound to loopback only. Broker
data is kept in the `pgdata` Docker volume. `docker compose down` stops the services;
do not use `down -v` if you want to keep your data.

The Compose file is a development deployment. It publishes the unauthenticated API
on port 8080. Do not expose it to an untrusted network. On an existing database
volume, changing `.env` alone does **not** rotate PostgreSQL's stored password; see
the [operations runbook](docs/operations.md) before updating an existing instance.

## Implemented foundations

- Entity writes, temporal history and mutation events commit atomically in PostgreSQL,
  including batch operations and entity/attribute deletion.
- Bulk create/upsert with individual-error fallback; dataset-aware attribute merging
  and deletion; bound SQL parameters and type-safe comparisons over multiple instances.
- Persistent notification jobs, bounded workers, fenced leases, retries, stable
  notification IDs, endpoint serialization, throttling and retained failed jobs.
  Delivery is **at least once**: receivers should deduplicate notification IDs.
- JSON-LD expansion/compaction using `json-ld`, canonical attribute IRIs, cached
  processed contexts, bundled ETSI context, media negotiation and bounded remote loads.
- Temporal metadata and instance IDs, history-only entities, `lastN`, created/modified
  time queries, multiple `aggrMethods`, fixed-duration aggregation and temporal deletion
  and instance update endpoints.
- Shared outbound HTTP policy checks the actual DNS answers used for connections.
  Notification redirects are disabled; context redirects are checked on every hop.
- Validated TOML/environment configuration, database and worker readiness, request/queue
  metrics, admission limits, bounded shutdown and bounded retention of completed work.
- Atomic subscription PATCH for supported fields, `isActive` pause/resume, fractional
  throttling, standard receiver headers and type-indexed matching prepared per batch.
- Persistent `timeInterval` scheduling with atomic snapshots, bounded backlog and
  configurable payload limits; MQTT 3.1.1/5.0 notifications with QoS 0/1/2 and verified TLS.
- A lockfile, pinned Rust toolchain and CI configuration with real database tests.

The code is organized into `athena-api` (HTTP routes/UI), `athena-model` (NGSI-LD
objects), `athena-jsonld` (contexts), `athena-query` (filter parsing),
`athena-storage` (SQL/migrations), `athena-subscription` (scheduling/delivery), and
`athena-http` (outbound requests). `src/` wires these crates into the executable.
The [API guide](docs/api.md) inventories the available routes and request formats.

## Development and verification

Rust 1.98.1 is pinned by `rust-toolchain.toml`. PostgreSQL 16 with PostGIS is required.

```bash
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets
```

Integration tests deliberately require a database named `athena_test`; they never
use the broker's normal `DATABASE_URL`.

```bash
docker run --rm -d --name athena-integration-db \
  -e POSTGRES_HOST_AUTH_METHOD=trust -e POSTGRES_DB=athena_test \
  -p 127.0.0.1:55432:5432 postgis/postgis:16-3.4
bash scripts/test-integration.sh
docker stop athena-integration-db
```

Run benchmarks only against an isolated `athena_test` database, never the Compose
volume. They are opt-in and can leave fixtures; see [development guide](docs/development.md).

MQTT interoperability (including a temporary private CA and hostname verification):

```bash
bash scripts/start-mqtt-test.sh
ATHENA_TEST_MQTT_CA="$PWD/artifacts/mqtt-test/ca.crt" \
  cargo test --locked --test mqtt_integration -- --ignored --nocapture
docker stop athena-mqtt-integration
```

See [periodic/MQTT examples and limits](docs/notifications.md).

The archived programs in `docs/architecture/legacy-mocks` copied implementations;
they are not conformance tests and are excluded from Cargo test discovery.

## Run

The quick start above is for a **new** database. Existing installations need a
database backup and staging migration test before upgrading: migrations add history
capture to entity mutations and can take locks. Previously discarded user contexts
cannot be reconstructed automatically.

```bash
curl http://localhost:8080/health  # process liveness
curl http://localhost:8080/ready   # database + schema readiness
curl http://localhost:8080/metrics
```

Configuration precedence is **built-in defaults → explicit TOML → environment**.
Use `--config PATH` or `ATHENA_CONFIG`; files are not discovered implicitly.
Unknown TOML keys, invalid environment values and inconsistent timeouts stop startup.
Validate without opening the database or printing credentials:

```bash
cargo run --locked -- --config config/default.toml --check-config
cargo run --locked -- --config config/default.toml
```

[config/default.toml](config/default.toml) lists all settings, including poll interval,
event batch size, retries, body limit, JSON-LD cache/concurrency and retention.

| Environment override | Default | Meaning |
| --- | --- | --- |
| `DATABASE_URL` | password-free local example | PostgreSQL connection; set real credentials for deployment |
| `HOST` / `PORT` | 0.0.0.0 / 8080 | HTTP binding |
| `DB_MAX_CONNECTIONS` / `DB_MIN_CONNECTIONS` | 50 / 1 | Pool bounds; maximum must be at least 3 |
| `DB_CONNECT_TIMEOUT_SECONDS` | 10 | Pool acquisition timeout |
| `DB_STATEMENT_TIMEOUT_MS` / `DB_LOCK_TIMEOUT_MS` | 15000 / 5000 | Per-connection query and lock limits |
| `NOTIFICATION_WORKERS` | 4 | Delivery concurrency, 1–64 |
| `NOTIFICATION_TIMEOUT_SECONDS` / `NOTIFICATION_LEASE_SECONDS` | 10 / 90 | HTTP timeout and durable ownership lease |
| `NOTIFICATION_MAX_ATTEMPTS` | 12 | Retry/dead-letter threshold |
| `MAX_IN_FLIGHT_WRITES` | 128 | Concurrent entity/temporal writes |
| `MAX_PENDING_EVENTS` | 100000 | Event/job admission threshold, sampled once per second |
| `SHUTDOWN_GRACE_SECONDS` | 30 | Shared shutdown deadline |
| `ALLOW_INTERNAL_ENDPOINTS` | false | Permit private HTTP destinations in trusted deployments |
| `RUST_LOG` | info | Log filter |

Request bodies default to 4 MiB; batches are limited to 1000 elements. Ingestion
saturation returns 503 while subscription controls remain available. Queue admission
is a soft threshold and can overshoot during a sampling interval or a large fan-out.

Completed notifications and unreferenced, successfully processed events are retained
for seven days by default. Cleanup removes at most the configured batch per category
per interval. **Dead letters and temporal history are never deleted by default**.
Set their retention ages explicitly to enable deletion; history ages use database
recording time, not device `observedAt`. Backups and disk monitoring remain necessary.

SIGTERM/Ctrl+C stops new worker claims, drains in-flight work within the configured
shared deadline, and leaves unfinished jobs recoverable through lease expiry. The
Compose development setup grants 40 seconds before forcible termination.

Operational guarantees, migration costs and deployment limits are documented in the
[operations runbook](docs/operations.md). Production requires managed secrets, an
authenticated TLS ingress, backups and capacity testing. Native authentication and
tenant isolation are not implemented. `/metrics` and the web explorer also have no
built-in access control.

## Example

```bash
curl -X POST http://localhost:8080/ngsi-ld/v1/entities \
  -H 'Content-Type: application/json' \
  -d '{"id":"urn:ngsi-ld:Sensor:1","type":"Sensor","temperature":{"type":"Property","value":23,"observedAt":"2026-09-22T10:00:00Z"}}'

curl --get http://localhost:8080/ngsi-ld/v1/entities \
  --data-urlencode 'local=true' --data-urlencode 'type=Sensor' \
  --data-urlencode 'q=temperature>20' -H 'Accept: application/ld+json'
```

`application/json` uses a context `Link` header, defaulting to the core context.
`application/ld+json` requires an inline `@context` on each input object. Core terms
are protected. Ranges use `temperature==20..30`; lists use `temperature==20,25,30`.
For temporal aggregation, use `aggrMethods=avg,totalCount`, with an optional
`aggrPeriodDuration=PT1H`. Months and years are explicitly rejected pending calendar
bucket support.

## Documentation and project status

| Document | Purpose |
| --- | --- |
| [API guide](docs/api.md) | Routes, JSON-LD handling, examples and error conventions |
| [Operations runbook](docs/operations.md) | Configuration, migrations, backup, delivery, monitoring and limits |
| [Notifications](docs/notifications.md) | Scheduling, HTTP/MQTT delivery, retry semantics and examples |
| [Development guide](docs/development.md) | Local toolchain, isolated tests, CI and benchmarks |
| [Implementation status](IMPLEMENTATION.md) | Implemented roadmap packages and open work |
| [Architecture decisions](docs/architecture/decisioni-2026-09-22.md) | Design choices and trade-offs |
| [Architecture plan](docs/architecture/piano-evoluzione-2026-09-22.md) | Priorities for IoT-scale development |

Reports under `docs/architecture/` are **dated snapshots**. In particular, the
[initial capability matrix](docs/architecture/matrice-ngsi-ld-2026-09-22.md) predates
later implementation work; it is an audit record, not a current conformance claim.
Archived benchmark JSON and SHA-256 manifests preserve the scope of those reports.

## Remaining work

The complete architectural plan is larger than the implemented foundation. See
[implementation status](IMPLEMENTATION.md) and the [architecture plan](docs/architecture/piano-evoluzione-2026-09-22.md).
Outstanding areas include full update/partial-success semantics, remaining subscription options, distributed scheduling, MQTT connection pooling, tenant isolation, temporal pagination and calendar
periods, temporal partitioning, discovery/projection/join coverage, complete federated
pagination/count/registration semantics and an independent conformance suite.
`NGSILD-Tenant` is rejected instead of silently ignored. Federation
remains experimental; use `local=true` for a bounded local query contract.

Local component measurements and their limitations are in
[initial validation](docs/architecture/implementazione-verifiche-2026-09-22.md) and
[operations/performance follow-up](docs/architecture/roadmap-operations-2026-09-22.md).
The later [scheduling/MQTT report](docs/architecture/scheduling-mqtt-2026-09-22.md)
records a local burst of 1,000 mutations with 5,000 notifications. These isolated
single runs do not establish sustained production throughput.

See [CONTRIBUTING.md](CONTRIBUTING.md) for development and review expectations. The
source code is licensed under [Apache-2.0](LICENSE).
