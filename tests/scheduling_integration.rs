use athena_model::{Entity, Subscription};
use athena_storage::{
    EntityRepository, PgEntityStore, PgSubscriptionStore, SubscriptionRepository,
};
use athena_subscription::{scheduler::materialize_schedule, EngineConfig};
use serde_json::{json, Value};
use sqlx::{PgPool, Row};

async fn due(pool: &PgPool, id: &str) {
    sqlx::query("UPDATE subscription_schedules SET next_run_at=clock_timestamp()-interval '1 hour',retry_at=clock_timestamp() WHERE subscription_id=$1")
        .bind(id).execute(pool).await.unwrap();
}
async fn jobs(pool: &PgPool, id: &str) -> Vec<sqlx::postgres::PgRow> {
    sqlx::query("SELECT * FROM notification_jobs WHERE subscription_id=$1 ORDER BY ordering_id,id")
        .bind(id)
        .fetch_all(pool)
        .await
        .unwrap()
}
async fn finish(pool: &PgPool, id: &str) {
    sqlx::query("UPDATE notification_jobs SET status='delivered',delivered_at=clock_timestamp() WHERE subscription_id=$1").bind(id).execute(pool).await.unwrap();
}

#[tokio::test]
#[ignore = "isolated PostGIS database"]
async fn periodic_snapshots_are_atomic_complete_and_restart_safe() {
    let url = std::env::var("ATHENA_TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres@127.0.0.1:55432/athena_test".into());
    assert!(url.ends_with("/athena_test"));
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(6)
        .connect(&url)
        .await
        .unwrap();
    athena_storage::run_migrations(&pool).await.unwrap();
    let prefix = format!("urn:ngsi-ld:Schedule:{}", uuid::Uuid::new_v4());
    let id = format!("{prefix}:subscription");
    let entity_store = PgEntityStore::new(pool.clone());
    let store = PgSubscriptionStore::new(pool.clone());
    let mut entities = Vec::new();
    for n in 0..4 {
        let entity=Entity::from_json(json!({"id":format!("{prefix}:{n}"),"type":["ScheduleSensor","SecondType"],"temperature":{"type":"Property","value":n*10},"location":{"type":"GeoProperty","value":{"type":"Point","coordinates":[n,0]}}})).unwrap();
        entity_store.create_entity(&entity).await.unwrap();
        entities.push(entity);
    }
    let sub:Subscription=serde_json::from_value(json!({"id":id,"type":"Subscription","entities":[{"type":"ScheduleSensor","idPattern":prefix},{"type":"SecondType","idPattern":prefix}],"timeInterval":0.25,"q":"temperature>=10","geoQ":{"geometry":"Polygon","coordinates":[[[0,-1],[2.5,-1],[2.5,1],[0,1],[0,-1]]],"georel":"within"},"notification":{"format":"keyValues","attributes":["temperature"],"endpoint":{"uri":"https://example.org/scheduled"}}})).unwrap();
    store.create_subscription(&sub).await.unwrap();
    assert_eq!(
        store
            .get_subscription_by_id(&id)
            .await
            .unwrap()
            .unwrap()
            .time_interval,
        Some(0.25)
    );
    for patch in [
        json!({"timeInterval":0}),
        json!({"timeInterval":-1}),
        json!({"watchedAttributes":["temperature"]}),
        json!({"throttling":1}),
    ] {
        assert!(store.update_subscription(&id, &patch).await.is_err());
    }
    let cfg = EngineConfig::default();
    // Simulate restart/outage: only the persisted overdue clock is needed.
    due(&pool, &id).await;
    let (a, b) = tokio::join!(
        materialize_schedule(&pool, &cfg),
        materialize_schedule(&pool, &cfg)
    );
    assert!(a.unwrap() || b.unwrap());
    let rows = jobs(&pool, &id).await;
    assert_eq!(
        rows.len(),
        1,
        "concurrent schedulers must produce one durable job"
    );
    assert!(rows[0].get::<Option<i64>, _>("event_id").is_none());
    let first: Value = rows[0].get("notification");
    let data = first["data"].as_array().unwrap();
    assert_eq!(
        data.len(),
        2,
        "q AND geoQ must filter and overlapping selectors must deduplicate"
    );
    assert_eq!(data[0]["temperature"], 10);
    assert!(data[0].get("location").is_none());
    let event_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM entity_events WHERE entity_id LIKE $1")
            .bind(format!("{prefix}%"))
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        event_count, 4,
        "scheduling must not fabricate entity mutations"
    );
    // Backpressure keeps one snapshot until delivery; missed ticks are coalesced.
    due(&pool, &id).await;
    materialize_schedule(&pool, &cfg).await.unwrap();
    assert_eq!(jobs(&pool, &id).await.len(), 1);
    finish(&pool, &id).await;
    materialize_schedule(&pool, &cfg).await.unwrap();
    let rows = jobs(&pool, &id).await;
    assert_eq!(rows.len(), 2);
    let second: Value = rows[1].get("notification");
    assert_eq!(
        second["data"], first["data"],
        "unchanged entities must be notified again"
    );
    assert_ne!(second["id"], first["id"]);
    finish(&pool, &id).await;
    // Notification statistics cannot postpone the next periodic run.
    let next: chrono::DateTime<chrono::Utc> = sqlx::query_scalar(
        "SELECT next_run_at FROM subscription_schedules WHERE subscription_id=$1",
    )
    .bind(&id)
    .fetch_one(&pool)
    .await
    .unwrap();
    store.record_notification_success(&id).await.unwrap();
    let after: chrono::DateTime<chrono::Utc> = sqlx::query_scalar(
        "SELECT next_run_at FROM subscription_schedules WHERE subscription_id=$1",
    )
    .bind(&id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(next, after);
    store
        .update_subscription(&id, &json!({"isActive":false}))
        .await
        .unwrap();
    due(&pool, &id).await;
    materialize_schedule(&pool, &cfg).await.unwrap();
    assert_eq!(jobs(&pool, &id).await.len(), 2);
    store.update_subscription(&id,&json!({"isActive":true,"notification":{"endpoint":{"uri":"mqtt://example.org/periodic"}}})).await.unwrap();
    due(&pool, &id).await;
    // Fail visibly rather than silently discarding entities at the configured cap.
    let small = EngineConfig {
        scheduler_max_entities: 1,
        ..Default::default()
    };
    materialize_schedule(&pool, &small).await.unwrap();
    let error: Option<String> = sqlx::query_scalar(
        "SELECT last_error FROM subscription_schedules WHERE subscription_id=$1",
    )
    .bind(&id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(error.unwrap().contains("scheduler_max_entities"));
    assert_eq!(jobs(&pool, &id).await.len(), 2);
    due(&pool, &id).await;
    materialize_schedule(&pool, &cfg).await.unwrap();
    let rows = jobs(&pool, &id).await;
    assert_eq!(rows.len(), 3);
    assert_eq!(
        rows[2].get::<String, _>("endpoint"),
        "mqtt://example.org/periodic"
    );
    finish(&pool, &id).await;
    store
        .update_subscription(&id, &json!({"q":"temperature>1000"}))
        .await
        .unwrap();
    due(&pool, &id).await;
    materialize_schedule(&pool, &cfg).await.unwrap();
    assert_eq!(
        jobs(&pool, &id).await.len(),
        3,
        "empty results produce no notification"
    );
    store
        .update_subscription(&id, &json!({"timeInterval":"urn:ngsi-ld:null"}))
        .await
        .unwrap();
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM subscription_schedules WHERE subscription_id=$1")
            .bind(&id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 0);
    store
        .update_subscription(
            &id,
            &json!({"timeInterval":1,"q":"urn:ngsi-ld:null","geoQ":"urn:ngsi-ld:null"}),
        )
        .await
        .unwrap();
    due(&pool, &id).await;
    sqlx::query(
        "UPDATE subscriptions SET expires_at=clock_timestamp()-interval '1 second' WHERE id=$1",
    )
    .bind(&id)
    .execute(&pool)
    .await
    .unwrap();
    materialize_schedule(&pool, &cfg).await.unwrap();
    assert_eq!(
        jobs(&pool, &id).await.len(),
        3,
        "expired schedule must not materialize"
    );
    store.delete_subscription(&id).await.unwrap();
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM subscription_schedules WHERE subscription_id=$1")
            .bind(&id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 0);
    for entity in entities {
        entity_store.delete_entity(&entity.id).await.unwrap();
    }
    pool.close().await;
}
