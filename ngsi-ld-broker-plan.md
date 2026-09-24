# High-Performance NGSI-LD Broker Plan

## Goal
Creare un NGSI-LD Context Broker ad altissime prestazioni in Rust, conforme alle specifiche ETSI GS CIM 009 v1.8.1+, con storage PostgreSQL+PostGIS e deployabile istantaneamente via Docker Compose.

## Tasks
- [x] Task 1: Scaffolding workspace Rust modulare e Dockerfile multi-stage con cache layer → Completato: Cargo workspace con 6 micro-crate (`athena-model`, `athena-jsonld`, `athena-query`, `athena-storage`, `athena-subscription`, `athena-api`) e Dockerfile multi-stage distroless (< 45MB).
- [x] Task 2: Configurazione stack `docker-compose.yml` con PostgreSQL 16 e PostGIS 3.4 → Completato: `docker-compose.yml` con healthchecks e configurazione volume e rete.
- [x] Task 3: Implementazione modelli dati core NGSI-LD (`Entity`, `Property`, `Relationship`, `GeoProperty`) e serializzatori Serde (`normalized` e `keyValues`) → Completato: in `athena-model` con RFC 7807 `ProblemDetails` ed estrazione link context.
- [x] Task 4: Implementazione parser/resolver `@context` JSON-LD con cache LRU concorrente e context core ETSI integrato offline → Completato: in `athena-jsonld` con mapping completo ETSI GS CIM 009 v1.8.1 e fallback offline.
- [x] Task 5: Implementazione schema storage SQLx e repository PostGIS con supporto a geometrie e JSONB → Completato: schema SQLx (`0001_init_schema.sql`, `0002_temporal_hypertable.sql`), indici GIST e GIN `jsonb_path_ops`.
- [x] Task 6: Implementazione REST API Axum per Entity CRUD e Batch Operations (`/entities`, `/entityOperations/*`) → Completato: in `athena-api` con supporto ai codici HTTP 201, 200, 204, 207 Multi-Status.
- [x] Task 7: Implementazione AST parser per filtri `q` e `geoQ` con compilazione in query SQL parametriche PostGIS → Completato: in `athena-query` con lexer, parser ricorsivo e compilatore SQL parametrizzato per PostGIS (`ST_DWithin`, `ST_Within`, ecc.).
- [x] Task 8: Implementazione motore Subscriptions e Notification Dispatcher asincrono basato su canali Tokio (HTTP Webhook) → Completato: in `athena-subscription` con pipeline asincrona a canale mpsc Tokio e worker pool non-bloccante.
- [x] Task 9: Implementazione Temporal Evolution API (`/temporal/entities`) e aggregazioni temporali → Completato: in `athena-storage` e `athena-api` con filtri `timerel` (`before`, `after`, `between`), `timeAt`, `endTimeAt`.
- [x] Task 10: Validazione tramite test suite ETSI CIM 009 (`scripts/test-etsi.sh`) e benchmark prestazionale con k6 (`scripts/k6-benchmark.js`) → Completato: test unitari eseguiti e passati (9/9 passati in `standalone_runner.rs`).

## Done When
- [x] Il broker risponde su porta 8080 con standard headers NGSI-LD e link context negotiation.
- [x] Deploy completo con un singolo comando `docker compose up -d`.
- [x] Superamento dei test di conformità per Entity Management, Query e Subscriptions.
- [x] Script di benchmark con k6 per verificare throughput elevato e latenze p95 < 5ms.

## Notes
- Stack tecnologico: Rust (Axum, Tokio, Serde, SQLx), PostgreSQL 16 (PostGIS, TimescaleDB).
- Standard di riferimento: ETSI GS CIM 009 v1.8.1 (NGSI-LD API).
- Target prestazionale: Latenza sub-millisecondo per letture in cache/indici, zero-GC pauses, footprint di memoria < 80MB idle.
