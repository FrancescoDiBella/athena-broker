# Implementation progress

Reference: [architecture plan](docs/architecture/piano-evoluzione-2026-09-22.md).
Priority: high-volume IoT ingestion, history, reliable notifications.

This is an implemented and tested foundation, not completion of the entire roadmap.

| Package | Implemented | Still open |
| --- | --- | --- |
| W00 | Lockfile/toolchain, production-crate tests, isolated PostGIS, CI configuration, real benchmark | Representative production qualification |
| W01 | Bound SQL, heterogeneous comparisons, batch savepoints, checked egress, temporal failure fixes | Broader fuzzing and negative protocol tests |
| W02 | Attribute validation, dataset identity, standard JSON-LD processor, canonical entity IRIs, negotiation | Tenant isolation, complete protocol/model edge cases and legacy migration |
| W03 | Atomic history/outbox for all entity mutation SQL, dataset-aware changes, partial attribute PATCH, attribute/entity PUT, null deletion, partial append reporting, type/scope fragments | Remaining batch modes, merge and distributed update semantics |
| W04 | Durable jobs, bounded workers, retry, leases, fencing, endpoint ordering, dead jobs, fractional throttle, geoQ, type-indexed/prepared matching, recoverable pause and bounded shutdown, persistent periodic snapshots | Cross-batch cache/invalidation, complete scheduling and lifecycle semantics |
| W05 | History catalog, instance IDs, no N+1 entity loop, count types, multiple methods, fixed periods, instance update/deletion, historical q/geo filters, ID/type lists, count/navigation, deletedAt and correct interval bounds | Calendar periods, instance partial-content pagination, remaining filters and temporal append semantics |
| W06 | Bulk create/upsert, processed-context cache, admission limits, measured local components and real HTTP/history/fan-out harness | Long-duration soak/fault injection, bulk update/delete optimization, deployment sizing |
| W07 | List/range parsing, multi-instance q, CSV id/type, attrs selection, all GeoProperty datasets, overlaps, SQL/matcher regex parity | Full query language, discovery, projection and joins |
| W08 | Reliable HTTP delivery, atomic PATCH for supported fields, isActive/expiry lifecycle, KeyValuePair receiver headers, bounded persistent timeInterval, MQTT 3.1.1/5.0 QoS 0/1/2 and TLS/private CA | Initial notifications, trigger selection, jsonldContext/rendering and remaining subscription options |
| W09 | All-page registration discovery, bounded multi-page queries, global count/pagination, dataset merge, split-entity filtering and context normalization | Complete registration modes/conflict semantics, temporal federation, distributed writes and snapshots |
| W10 | Database readiness, HTTP/queue metrics, validated TOML/environment configuration, worker-aware readiness, bounded retention and shutdown | Partitioning, full telemetry, representative restore/failover drills |
| W11 | Versioned source reference and explicit feature gaps | Independent conformance dossier, extended NGSI-LD capabilities |

See [implementation and validation report](docs/architecture/implementazione-verifiche-2026-09-22.md)
for verified guarantees, benchmark scope, operations and remaining limitations.

Pre-change source archives and database backups mentioned in the dated reports were
local deployment artifacts and are not distributed in this repository. Integration
tests use a separately created `athena_test` database on localhost:55432 (see the
[development guide](docs/development.md)); no test database is part of the regular
Compose stack. A local broker instance was updated on 2026-09-22 after a staging
restore and entity content-hash verification. This is a deployment record, not a
claim that a fresh clone has been production-qualified.

The next implemented increment is documented in [operations and performance](docs/architecture/roadmap-operations-2026-09-22.md).
Its source snapshot is `docs/architecture/roadmap-operations-sha256-2026-09-22.json`.
That increment ended at schema 9; the scheduling/MQTT increment adds schema 10.

The scheduling/MQTT release, verification and deployment are described in
[the scheduling report](docs/architecture/scheduling-mqtt-2026-09-22.md).

The [4 October query/mutation increment](docs/architecture/query-mutation-2026-10-04.md)
adds tested behavior without a schema migration. The full roadmap remains open in
[issue #1](https://github.com/FrancescoDiBella/athena-broker/issues/1); local regression
success does not establish completion of W00–W11 or independent ETSI conformance.
