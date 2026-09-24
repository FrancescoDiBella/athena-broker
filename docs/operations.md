# Operating Athena

This runbook describes the implemented development broker. Production qualification
still requires conformance testing, workload sizing, failure drills and deployment
security. It is not a declaration of full NGSI-LD conformance.

## Configuration and startup

Run `athena-broker --config /etc/athena/config.toml --check-config` before deployment.
The same file can be selected with `ATHENA_CONFIG`. Environment overrides file values;
unknown fields and malformed values are errors, not silent fallbacks. Configuration
is immutable for the process lifetime. Restart replicas to change it.

Use [the complete example](../config/default.toml) as a starting point. The old
`channel_buffer`, `default_timeout_sec` and `offline_core_context` placeholders are
removed; an old file containing them fails validation. The core context remains
bundled offline. `ALLOW_INTERNAL_ENDPOINTS` controls notifications, remote contexts,
and federation consistently; allowing it means permitting private network targets.

Choose `database.max_connections` across **all** replicas within the PostgreSQL budget.
The connection acquisition, statement and lock timeouts are separate. The notification
lease must exceed HTTP timeout plus two acquisition/statement budgets; the validator
checks this relationship. The delivery routine also has a cancellation deadline inside
the lease. Configuration validation never prints the connection URL or TOML source.

Keep the broker behind an authenticated TLS ingress and restrict `/metrics` to the
monitoring network. Built-in OAuth/OIDC, per-resource authorization and tenant isolation
are still absent. The Compose file is a development deployment: the password comes
from an ignored `.env`, PostgreSQL's published port binds to loopback, and the HTTP
port remains public on the host. It is not a production security template. For a
fresh install copy `.env.example` to `.env` and set a nonempty password. On an
existing PostgreSQL volume, changing `.env` does not change the role password stored
in the database; coordinate rotation with `ALTER ROLE` and update the broker URL.

## Migrations and upgrade

Migrations use SQLx locking/checksums. Do not edit applied SQL files. Versions 7–9 add
subscription contexts, fractional throttling, completion/recording timestamps and
indexes for delivery/retention. Version 7 can rewrite existing temporal rows because
its `recorded_at` default is volatile; the column/type changes and index builds take
locks. **These are not online, zero-downtime migrations.** Version 10 changes `time_interval`
to double precision, adds the durable schedule table and backfills/reindexes job ordering.
Stop all older broker replicas before applying it; an old integer decoder is incompatible.

Before a production upgrade, restore a backup into staging, measure migration duration,
WAL/disk headroom and lock impact at representative data volume. Stop old broker
replicas for this upgrade: version 7 changes the throttling column from integer to
floating point, which the old decoder cannot read. Rollback requires the tested
backup/restore procedure, not simply launching the old executable over the new schema.
Existing temporal records receive migration time as their retention recording time;
this intentionally avoids immediately expiring legacy data.

A backup should include entity state, temporal catalog/history, subscriptions, events
and notification jobs from a consistent database snapshot. Restore them together.
A receiver can have acknowledged a notification after the restored snapshot, so
notification IDs must remain idempotent at the receiver. A restore of the local instance was tested during the 2026-09-22 update, including
entity content hashes. This does not replace representative restore/failover drills.

## Delivery and lifecycle

Entity/history/outbox writes share a database transaction. Jobs are durable, claimed
with `FOR UPDATE SKIP LOCKED`, and completed with a lease-token fence. Matching is
prepared and indexed by entity type once per batch. It refreshes subscriptions from
the database at each batch; no cross-replica invalidation cache is involved.

Delivery is **at least once** within the retry/dead-letter policy. A crash after the
receiver accepts HTTP but before the database commit can cause redelivery of the
same notification ID. Pending work is not removed by retention. A malformed stored
job or repeated delivery failure reaches `dead` after the configured attempt limit.

`PATCH /ngsi-ld/v1/subscriptions/{id}` supports `subscriptionName`, `description`,
`entities`, `watchedAttributes`, `q`, `geoQ`, `notification`, `throttling`, `timeInterval`, `expiresAt`
and `isActive`. Each PATCH merges under a row lock and validates the resulting
subscription. A `notification` fragment replaces that whole member; include its
endpoint. Server-generated status/counters are ignored on input and retained.
Optional supported fields can be removed with `urn:ngsi-ld:null`; JSON null is rejected.
Selectors can be omitted when `watchedAttributes` is provided. Unsupported options
return errors instead of being silently accepted.

Use `isActive:false` to pause and `isActive:true` to resume. Already queued work survives
a pause without spending retry attempts; an inactive subscriber does not block another
subscriber at the same endpoint. In-flight HTTP calls may already have reached a
receiver when a pause/delete occurs. Paused-period mutations do not generate new
jobs. Expired subscriptions require a future `expiresAt` to become active again.

Queued jobs retain their original notification ID, body, endpoint and headers for
stable retries. An endpoint/filter/format PATCH applies to future materialized jobs;
current active/paused/expired state and throttling are rechecked before delivery.
Delivery ordering is per endpoint and per subscription, including endpoint changes.
If a subscription is paused, other subscriptions may proceed at that endpoint; there
is no total endpoint ordering across the pause/resume boundary. Slow receivers are
serialized, so increasing worker count alone cannot speed up one receiver.

`receiverInfo` accepts standard `{key,value}` arrays and the legacy `headers` object.
Protocol/framing headers cannot be overridden. Fractional positive throttling is
supported. Periodic `timeInterval` and MQTT/TLS are described in [notifications](notifications.md).
Initial notifications, notificationTrigger, showChanges,
jsonldContext negotiation and several advanced selection options remain incomplete.
The subscription's input context is persisted; custom notification rendering remains
an open conformance task.

## Retention and capacity

Defaults remove delivered jobs and unreferenced successful events after seven days;
failed events/jobs and temporal history remain indefinitely. Setting any age to zero
disables that category. Temporal retention uses `recorded_at`, independent of observed,
created or modified times supplied in entity history. It does not delete live entities
or the temporal entity catalog, which may legitimately retain an empty history.

Each cleanup pass deletes at most `batch_size` rows **per category**: delivered jobs,
dead jobs, successful events, failed events, history. Multiple replicas skip locked
rows. Events with any remaining job cannot be removed. The worker waits `interval_sec`
between passes; tune both values to exceed the measured expiration rate. For example,
1000 rows per minute per category is a conservative development default, not suitable
for sustained thousands of expirations per second. Large histories still require
partitioning and autovacuum planning; row deletion does not shrink database files.

Admission samples pending queues once per second and checks a bounded row count. It
is backpressure, not a strict global quota: simultaneous replicas, in-flight requests
and fan-out can overshoot. Subscription controls bypass ingestion admission but still
have payload/media validation and body caps. Monitor disk space independently.

## Health and shutdown

`/health` reports process liveness. `/ready` requires the current migration version,
a responsive database and running materializer/delivery tasks. Shutdown makes workers
unready, stops new claims and uses a shared deadline for HTTP/worker/pool drain.
Unfinished claims recover after lease expiry. The orchestrator's kill grace should
exceed `server.shutdown_grace_sec` (30 seconds by default; Compose grants 40 seconds).

`/metrics` exposes request counts/errors/in-flight/time, pool occupancy, pending jobs,
dead jobs/events, oldest event age, database availability and worker availability
(`athena_notification_workers_up`), due schedules (`athena_schedules_due`) and
schedule generation errors (`athena_schedules_failed`). Queue counts are database queries with a two-second
timeout; they are not a full low-cost telemetry pipeline for arbitrarily large queues.
Investigate rising oldest-event age or pending/dead counts before changing concurrency.
There is currently no authenticated public dead-letter replay API.

## Verification commands

```bash
cargo test --locked --workspace
bash scripts/test-integration.sh
cargo test --locked --release --test subscription_benchmark -- --ignored --nocapture
cargo test --locked --release --test http_iot_benchmark -- --ignored --nocapture
```

Integration/HTTP benchmarks accept only `ATHENA_TEST_DATABASE_URL` ending in
`/athena_test`. The HTTP benchmark retains history fixtures and retires interrupted
runs in its own `HttpBench` subscription namespace. Run benchmarks sequentially,
on an otherwise idle isolated database, and record hardware, database image, mode,
configuration and queue depth. Local emulation measurements cannot size production.
