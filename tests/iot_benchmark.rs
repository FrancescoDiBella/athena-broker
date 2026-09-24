//! Repeatable local component benchmark, not a claim about end-to-end broker capacity.
use athena_model::Entity;
use athena_storage::{EntityRepository, PgEntityStore};
use serde_json::json;
use std::time::Instant;

#[tokio::test]
#[ignore = "explicit benchmark on isolated PostgreSQL only"]
async fn iot_write_baseline() {
    let url = std::env::var("ATHENA_TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres@127.0.0.1:55432/athena_test".into());
    assert!(url.ends_with("/athena_test"));
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .connect(&url)
        .await
        .unwrap();
    athena_storage::run_migrations(&pool).await.unwrap();
    let store = PgEntityStore::new(pool.clone());
    let prefix = format!("urn:ngsi-ld:Benchmark:{}", uuid::Uuid::new_v4());
    let count = 500;
    let make = |family: &str, i: usize, value: usize| {
        Entity::from_json(json!({"id":format!("{prefix}:{family}:{i}"),"type":"BenchmarkSensor","reading":{"type":"Property","value":value,"observedAt":"2026-09-22T00:00:00Z"}})).unwrap()
    };
    // Warm connection, trigger, index and statement caches.
    store.create_entity(&make("warm", 0, 0)).await.unwrap();
    let single: Vec<_> = (0..count).map(|i| make("single", i, i)).collect();
    let start = Instant::now();
    let mut latencies = Vec::new();
    for entity in &single {
        let t = Instant::now();
        store.create_entity(entity).await.unwrap();
        latencies.push(t.elapsed().as_secs_f64() * 1000.);
    }
    let single_seconds = start.elapsed().as_secs_f64();
    latencies.sort_by(f64::total_cmp);
    let bulk: Vec<_> = (0..count).map(|i| make("bulk", i, i)).collect();
    let start = Instant::now();
    for chunk in bulk.chunks(100) {
        let result = store.bulk_write_atomic(chunk, false).await.unwrap();
        assert!(result.errors.is_empty());
    }
    let bulk_seconds = start.elapsed().as_secs_f64();
    let upserts: Vec<_> = (0..count).map(|i| make("bulk", i, i + 1)).collect();
    let start = Instant::now();
    for chunk in upserts.chunks(100) {
        let result = store.bulk_write_atomic(chunk, true).await.unwrap();
        assert!(result.errors.is_empty());
    }
    let upsert_seconds = start.elapsed().as_secs_f64();
    let event_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM entity_events WHERE entity_id LIKE $1")
            .bind(format!("{prefix}%"))
            .fetch_one(&pool)
            .await
            .unwrap();
    let history_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM entity_temporal WHERE entity_id LIKE $1")
            .bind(format!("{prefix}%"))
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(event_count, 1501);
    assert_eq!(history_count, 1501);
    let payload = json!({"id":"urn:sensor:benchmark","type":"Sensor","temperature":{"type":"Property","value":23}});
    let context = json!(athena_jsonld::processor::CORE);
    let processor = athena_jsonld::Processor::default();
    processor
        .normalize(payload.clone(), context.clone())
        .await
        .unwrap();
    let start = Instant::now();
    for _ in 0..100 {
        processor
            .normalize(payload.clone(), context.clone())
            .await
            .unwrap();
    }
    let cached = start.elapsed().as_secs_f64();
    let start = Instant::now();
    for _ in 0..100 {
        athena_jsonld::Processor::default()
            .normalize(payload.clone(), context.clone())
            .await
            .unwrap();
    }
    let uncached = start.elapsed().as_secs_f64();
    println!("{}",serde_json::to_string_pretty(&json!({"profile":if cfg!(debug_assertions){"debug"}else{"release"},"entities_per_phase":count,"batch_size":100,"single_create_entities_per_second":count as f64/single_seconds,"single_create_p50_ms":latencies[count/2],"single_create_p95_ms":latencies[count*95/100],"bulk_create_entities_per_second":count as f64/bulk_seconds,"bulk_upsert_entities_per_second":count as f64/upsert_seconds,"history_rows_verified":history_count,"events_verified":event_count,"jsonld_cached_per_second":100./cached,"jsonld_uncached_per_second":100./uncached,"database":"isolated PostgreSQL/PostGIS, local Docker; not production sizing"})).unwrap());
    pool.close().await;
}
