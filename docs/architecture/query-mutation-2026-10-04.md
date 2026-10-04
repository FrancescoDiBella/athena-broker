# Entity mutations, temporal geoqueries and federation — 4 October 2026

This increment closes specific correctness gaps in W01/W03/W05/W07/W09. It does
**not** complete every package in the architecture roadmap. The remaining work is
tracked in [issue #1](https://github.com/FrancescoDiBella/athena-broker/issues/1).

## Changed behavior

- `PATCH /entities/{id}/attrs/{attr}` patches one dataset instance under a database
  row lock. Other subattributes and datasets survive. NGSI-LD null can remove
  optional subattributes; deleting required members or changing the attribute
  type is rejected. Inline JSON-LD contexts also apply to the attribute path.
- `PUT /entities/{id}` replaces an existing entity; `PUT .../attrs/{attr}` replaces
  an existing dataset instance. Missing targets return 404; mismatched entity IDs
  and invalid representations return 400. Database triggers capture the final
  mutation, history and outbox event in the same transaction.
- Attribute updates support Property/Relationship/LanguageProperty null deletion.
  Append with `noOverwrite` returns 207 and `updated`/`notUpdated` when existing
  instances are preserved. Updates may add attributes, as allowed in version 1.9.1.
  Entity type additions and scope update/append behavior are handled under the same
  lock. Invalid scope values are rejected. No-op attribute changes do not increment
  the entity revision or create events.
- Empty/null-containing batch containers and invalid batch-delete IDs are rejected
  before any deletion. An invalid item is no longer silently dropped from a delete
  request. This does not complete all batch update/upsert modes.
- Temporal collection queries combine ID lists, ID patterns, type lists, `q`, geo
  filters, attributes, count and pagination. Value and geo filters use historical
  instances within the selected interval, independently of current entity state.
  Output projection and `lastN` follow entity selection. Historical-only and deleted
  entities can match. The upper `between` bound is exclusive; `after` includes its
  lower bound. `timeproperty=deletedAt` is supported.
- Temporal responses retain attribute arrays even for one selected instance.
  Missing IDs in a collection produce an empty result, and pagination applies to
  ID-filtered queries. `limit=0` requires `count=true`. Navigation links coexist with
  the JSON-LD context Link header.
- Spatial matching considers all GeoProperty dataset instances, including custom
  attributes. Non-spatial values cannot enter PostGIS geometry conversion.
  `overlaps` is supported; inappropriate or duplicate distance modifiers are
  rejected. Meter-based `near` uses geography, including across the antimeridian.
- Subscription positive/negative regex matching agrees with SQL for multiple
  datasets, missing attributes and non-string values. Query regexes and ID patterns
  are validated against the Rust regex syntax supported by the matcher; malformed
  patterns and unsupported constructs such as look-around return 400.

The normative reference is [ETSI GS CIM 009 V1.9.1](https://www.etsi.org/deliver/etsi_gs/CIM/001_099/009/01.09.01_60/gs_CIM009v010901p.pdf),
particularly clauses 4.10, 4.11, 5.5.8, 5.6.2–5.6.4 and 5.7.4. Tests below establish
only the listed implementation cases, not independent conformance certification.

## Federation contract

Current-entity queries discover all registration pages, honor expiry/status and
match CSV IDs/types. They collect bounded local and remote results before merging,
counting and applying the global page. Remote pages can be smaller than the requested
page size. Malformed/partial responses, changing counts, repeated IDs, upstream failures
and budget exhaustion fail explicitly with HTTP 502; they do not produce a success
containing incomplete data. `local=true` bypasses federation.

Remote queries use `local=true` to stop cascades and request normalized system
attributes. Responses are normalized with their inline or Link context, including
combined context/navigation Link fields. Legacy local
short keys are normalized with the default vocabulary before merging. Local dataset
instances take precedence, then registration IDs in lexical order; independent
remote datasets are retained. This deterministic policy is not complete normative
registration-mode/conflict resolution.

`splitEntities=true` discovers sources independently of the output projection and
retrieves candidates without value/geo/projection filters,
merges attributes, then evaluates `q`, geo and attribute selection through the same
PostgreSQL query compiler used by local queries. The default is `splitEntities=false`,
where each source applies its filters. Sorting is modified time descending, then
entity ID. Counts are deduplicated entity counts; `limit=0&count=true` still performs
the bounded collection needed to deduplicate.

Budgets: 256 matching registrations, 8 concurrent remote requests, a shared 30-second
remote deadline, 10,000 entities per source and in the merged result, 8 MiB per remote
response, and 32 MiB of combined normalized snapshots. Cancelling the incoming request
aborts outstanding remote tasks. Live data can change between offset pages; there is
no distributed snapshot or stable cursor guarantee. A changing/repeated remote page
fails explicitly. Temporal federation, distributed writes and complete registration
modes remain open. Temporal queries are local by default; explicit `local=false` or
`splitEntities=true` returns 501 instead of silently returning incomplete local data.

## Verification

The production crates, Axum middleware/router, PostgreSQL 16/PostGIS 3.4 and temporary
local HTTP sources are used by `tests/roadmap_integration.rs`. Cases include concurrent
patches, durable event counts, null deletion, replacement, malformed delete batches,
metadata, history-only entities, count-only pages, excluded upper timestamps, named
geospatial datasets, polygon holes/boundaries, overlaps, antimeridian distances,
deleted spatial history, SQL/subscription regex parity, discovery beyond 100
registrations, duplicate entities, small upstream pages, split-entity filters and
failed upstreams. CI and `scripts/test-integration.sh` include this suite.

Checks run locally:

```sh
cargo fmt --all --check
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets
ATHENA_TEST_MQTT_CA="$PWD/artifacts/mqtt-test/ca.crt" \
  cargo test --locked --test roadmap_integration --test storage_integration \
  --test operations_integration --test scheduling_integration \
  --test mqtt_integration -- --ignored --nocapture
```

All five integration suites passed against the isolated `athena_test` database.
No application database or production deployment was modified. Clippy retains
pre-existing warnings; SQLx 0.7.4 retains a future compatibility warning.

No schema migration is required. Historical filtered queries may aggregate many
instances, and dataset-aware spatial matching now reads JSON instances rather than
relying solely on the default-location projection. Large-history query plans and
sustained ingestion capacity need dedicated measurement. The ordinary temporal path
without `q`/geo retains an index-friendly existence query. These changes carry no
new throughput or production qualification claim.
