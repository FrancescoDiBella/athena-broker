# Scheduling, MQTT and instance upgrade — 2026-09-22

This increment adds persistent periodic notifications and the MQTT notification
transport to the existing PostgreSQL outbox. It does not establish full NGSI-LD
conformance or production sizing. Usage, configuration and explicit limits are in
[notifications](../notifications.md).

## Implemented

- `timeInterval`, including fractional seconds, with mutual exclusion against
  watched attributes/throttling, creation/PATCH/deletion lifecycle and persistent deadlines.
- Transactional snapshot and queue insertion; concurrent schedulers use row locks,
  skip locked schedules and retain deadlines on failure. Complete current local
  snapshots include unchanged entities and apply selectors, `q`, `geoQ` and projection.
- One pending periodic snapshot per subscription, missed-tick coalescing, configurable
  entity/payload/backlog limits, recorded errors and bounded retry backoff.
- MQTT 3.1.1 and 5.0, QoS 0/1/2, `notifierInfo`, NGSI-LD metadata/body envelope,
  URI credentials/topic decoding, strict settings and bounded protocol frames.
- Broker acknowledgements precede outbox completion for QoS 1/2. Rejected/mismatched
  acknowledgements fail delivery. Durable retries preserve the notification ID.
- MQTT TLS with hostname verification, bundled trust roots and optional additional
  private CA file; TCP uses the actual checked DNS answers. Existing private-network
  restrictions apply to MQTT too.
- Migration 10 adds schedules and a common ordering key for mutation/periodic jobs,
  without creating fake entity history. New schedule metrics and CI integration fixtures.

## Verification

`cargo fmt --all --check`, the workspace tests (32 ordinary tests), and four explicit
PostGIS/Mosquitto integration suites passed locally. The suites cover:

- existing entity/history/outbox atomicity, lifecycle, pause ordering and shutdown;
- concurrent schedulers, overdue recovery, overlapping-selector deduplication,
  `q`/`geoQ` intersection, unchanged entities, empty results, caps and schedule deletion;
- both MQTT versions and all three QoS levels against Mosquitto;
- rejected/missing/mismatched ACK fixtures; stable outbox retries and success counters;
- private CA success, untrusted certificate rejection and hostname mismatch rejection;
- HTTP subscription creation through the background scheduler to repeated MQTT delivery.

Clippy completes with existing warnings in other components; the new MQTT/scheduler
modules introduce no remaining warnings. SQLx 0.7.4 still emits its existing future
compatibility warning. CI was updated but no hosted CI execution is claimed.

## Burst queue performance

The first schema-10 benchmark exposed a stale-statistics plan regression: 5,000 HTTP
notifications took 20.897 seconds. A controlled 5,000-job transaction showed two
anti-join scans visiting all 5,000 pending rows per claim when PostgreSQL still
estimated an empty queue. Updating statistics reduced a sampled claim from 6.832 ms
to 0.154 ms, identifying planner sensitivity rather than a network delivery failure.

The claim query now retains correlated predecessor probes using `OFFSET 0`. This
preserves ordering/lease/pause semantics and lets each existence check stop on its
first match. The diagnostic transaction was rolled back. Autovacuum/statistics and
backlog monitoring still matter; this is not a guarantee of constant latency for
unlimited queues. The final run after cleaning up diagnostic dead tuples delivered all 5,000
notifications in **7.032 s**: 1,000 mutations, 1,000 history instances, 2,219.74
mutations/s during HTTP ingestion, and batch-request p95 190.67 ms. An intervening run
before cleanup took 44.520 s with much slower ingestion too; it is retained in the
[measurement archive](scheduling-mqtt-benchmarks.json), not discarded. These are local
single-run measurements on an evolving fixture database, not proof of production
throughput or a statistically controlled speedup. The final result is close to the
prior schema-9 run (6.537 s); performance qualification at sustained load remains open.

## Deployment procedure and rollback material

The original instance used an unjournaled legacy schema. Its custom-format backup was
restored into a fresh `template0` database in a temporary PostGIS container. Using a
fresh database avoids collisions with PostGIS image initialization schemas. The built
Linux ARM64 image successfully adopted migrations 1–9, then the feature image applied
migration 10. Entity content hashes and counts were checked after migration.

The update is an explicit maintenance operation: stop the broker, take a final
consistent database backup, replace only the broker container, and verify readiness,
schema, metrics and data counts. The PostgreSQL container and persistent volume are
retained. Source changes do not silently deploy themselves.

Local rollback material is under `artifacts/deploy-2026-09-22` (directory mode 0700;
database backups created with umask 077). The legacy image was retained as
`athena-broker:rollback-20260922`; the tested schema-9 baseline is
`athena-broker:roadmap-20260922`. Image identities and backup hashes are recorded in
the deployment manifest. The pre-development source archive is
`/tmp/athena-before-scheduling-mqtt-2026-09-22.tar.gz`.

Do not launch an older binary against schema 10: its integer `time_interval` decoder
is incompatible. To roll back, stop writers, restore the matching whole-database
backup into a fresh database using `createdb -T template0` and
`pg_restore --exit-on-error --no-owner`, verify it, then point the matching old image
at that restored database. Keep the current database for investigation. Writes after
the backup would require separate reconciliation; restoring a snapshot cannot retain
them automatically. No destructive rollback was performed during this update.

## Remaining roadmap

Initial notifications, custom JSON-LD rendering/`jsonldContext`, additional subscription
filters and triggers, distributed periodic selection, MQTT connection pooling/mTLS,
tenant isolation and independent ETSI conformance tests remain open. MQTT delivery
uses one clean connection per attempt; QoS 2 does not make database completion and
external broker acceptance an atomic transaction. No competitor superiority claim or
production MQTT throughput claim is made.

## Completed deployment

Completed at 2026-09-22T22:34:49.025149+00:00 (2026-09-23 in Europe/Rome).
The active image is `athena-broker:scheduling-mqtt-20260922`, identity
`sha256:392a7a46e2089fa92ecb30fed295bbe9ca9ca0882063a7b4086a1670b624cd6e`. Broker and PostgreSQL reported healthy, `/ready` returned 200,
and local entity/subscription query probes returned 200. Schema is now 10.

The final backup `before-scheduling-mqtt.dump` was restored into a new temporary
database and matched the complete pre-upgrade fingerprint. After deployment, both
entity and historical-record fingerprints matched: **1,084 entities, 36 history
records, one context-source registration and zero subscriptions**. No pending jobs
remained. The existing PostgreSQL container/image/volume were retained. Temporary
staging and Mosquitto containers were stopped after verification.

The final backup SHA-256 is
`e7c88cc54fd2060d81662cf4be2e6826065fb71926dd4fba681274a6ffdba414`.
Local evidence and restore material: `artifacts/deploy-2026-09-22/deployment-manifest.json`.
Code/config/test hashes: [source manifest](scheduling-mqtt-source-sha256.json).
