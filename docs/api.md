# HTTP API guide

This guide describes routes currently wired in `crates/athena-api/src/routes.rs`.
It is an implementation guide, not a statement of complete ETSI NGSI-LD conformance.
Unless otherwise shown, examples use `http://localhost:8080` and the local Compose
installation described in the [README](../README.md).

## Representation and context

- Send NGSI-LD objects with `Content-Type: application/json` and a JSON-LD context
  `Link` header, or use `Content-Type: application/ld+json` with inline `@context`.
  If no `Link` is supplied for JSON, Athena uses its bundled ETSI core context.
- Request `Accept: application/ld+json` for an inline context in responses, or
  `application/json` for JSON with the context link. Context expansion and
  compaction are implemented, but unsupported NGSI-LD model forms remain; see
  [implementation status](../IMPLEMENTATION.md).
- IDs are URIs such as `urn:ngsi-ld:Sensor:1`. URL-encode IDs and attribute names
  when placing them in path segments.
- `NGSILD-Tenant` is rejected: tenant isolation is not implemented. The development
  deployment has no native authentication or authorization.

## Route inventory

`{id}`, `{attr}` and `{instance}` below are URL-encoded path segments.

| Method | Route | Purpose |
| --- | --- | --- |
| GET | `/`, `/ui` | Web explorer and test page |
| GET | `/health`, `/ready`, `/metrics` | Process, dependency readiness and Prometheus-style metrics |
| POST, GET | `/ngsi-ld/v1/entities` | Create entity, query entities |
| GET, PATCH, DELETE | `/ngsi-ld/v1/entities/{id}` | Read, update, delete entity |
| POST, PATCH | `/ngsi-ld/v1/entities/{id}/attrs` | Append or update attributes |
| DELETE | `/ngsi-ld/v1/entities/{id}/attrs/{attr}` | Delete an attribute |
| POST | `/ngsi-ld/v1/entityOperations/create` | Batch create |
| POST | `/ngsi-ld/v1/entityOperations/upsert` | Batch upsert |
| POST | `/ngsi-ld/v1/entityOperations/update` | Batch update |
| POST | `/ngsi-ld/v1/entityOperations/delete` | Batch delete |
| POST, GET | `/ngsi-ld/v1/subscriptions` | Create or list subscriptions |
| GET, PATCH, DELETE | `/ngsi-ld/v1/subscriptions/{id}` | Read, update, delete subscription |
| POST, GET | `/ngsi-ld/v1/csourceRegistrations` | Create or list context-source registrations |
| GET, PATCH, DELETE | `/ngsi-ld/v1/csourceRegistrations/{id}` | Read, update, delete registration |
| POST, GET | `/ngsi-ld/v1/temporal/entities` | Import/query temporal entities |
| GET, DELETE | `/ngsi-ld/v1/temporal/entities/{id}` | Read or delete temporal entity |
| DELETE | `/ngsi-ld/v1/temporal/entities/{id}/attrs/{attr}` | Delete temporal attribute |
| PATCH, DELETE | `/ngsi-ld/v1/temporal/entities/{id}/attrs/{attr}/{instance}` | Update/delete temporal instance |

The entity query currently accepts `id`, `idPattern`, `type`, `q`, `georel`,
`geometry`, `coordinates`, `geoproperty`, `attrs`, `limit`, `offset`, `options`,
`count` and `local`. `local=true` suppresses federation. Query results with
registrations have experimental merge/count/pagination semantics, so use `local=true`
when a bounded local result is required. The temporal query accepts `id`, `type`,
`timerel`, `timeAt`, `endTimeAt`, `timeproperty`, `attrs`, `lastN`, `aggrMethods`,
`aggrPeriodDuration`, `limit` and `offset`; calendar periods are not supported.

## Minimal entity walkthrough

```bash
curl -i -X POST http://localhost:8080/ngsi-ld/v1/entities \
  -H 'Content-Type: application/json' \
  -d '{"id":"urn:ngsi-ld:Sensor:demo-1","type":"Sensor","temperature":{"type":"Property","value":23}}'

curl --get http://localhost:8080/ngsi-ld/v1/entities \
  --data-urlencode 'local=true' \
  --data-urlencode 'type=Sensor' \
  --data-urlencode 'q=temperature>20' \
  --data-urlencode 'limit=10'

curl -i http://localhost:8080/ngsi-ld/v1/entities/urn:ngsi-ld:Sensor:demo-1

curl -i -X DELETE http://localhost:8080/ngsi-ld/v1/entities/urn:ngsi-ld:Sensor:demo-1
```

Entity creation returns HTTP 201 and a `Location` header. A duplicate ID returns
409. Batch create returns 201 for all-success and 207 with per-item errors for a
mixed result. Batch upsert returns 204 for all-success and 207 for a mixed result.
The request body limit defaults to 4 MiB and batch operations accept at most 1,000
elements. Ingestion backpressure can return 503; clients should retry with bounds.

## Subscriptions and notifications

Subscriptions are managed over HTTP and can deliver notifications over HTTP(S) or
MQTT. A subscription creation returns 201 with `Location`. Mutation notifications
are durable and retried, with **at-least-once** delivery. `timeInterval` produces
periodic snapshots of matching *local* entities. MQTT is currently a notification
transport, not an entity-ingestion API. See [notifications](notifications.md) for a
complete subscription example, QoS semantics, scheduler limits and retry behavior.

## Errors and operational endpoints

The handlers return JSON problem details for many validation, not-found, conflict
and server errors. Common statuses include 400 for invalid input, 404 for a missing
resource, 409 for duplicate creation and 503 for ingestion admission. Middleware
and extractor errors may differ in shape; clients should handle HTTP status and
content type first rather than assume every error body has one schema.

`/health` is process liveness. `/ready` checks database, schema and worker state.
`/metrics` exposes request, queue, scheduler and database gauges; it has no built-in
access control. Keep it behind an authenticated ingress outside local development.
For migration and production limits see the [operations runbook](operations.md).
