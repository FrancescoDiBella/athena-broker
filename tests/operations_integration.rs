use athena_model::{Entity, Subscription, SubscriptionStatus};
use athena_storage::{
    EntityRepository, PgEntityStore, PgSubscriptionStore, SubscriptionRepository,
};
use athena_subscription::engine::{deliver_one, materialize_events};
use serde_json::{json, Value};
use sqlx::Row;
use std::{sync::Arc, time::Duration};

#[tokio::test]
#[ignore = "isolated PostGIS database and local notification receiver"]
async fn production_operations_lifecycle_and_retention() {
    let url = std::env::var("ATHENA_TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres@127.0.0.1:55432/athena_test".into());
    assert!(url.ends_with("/athena_test"));
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(12)
        .connect(&url)
        .await
        .unwrap();
    athena_storage::run_migrations(&pool).await.unwrap();
    let prefix = format!("urn:ngsi-ld:Operations:{}", uuid::Uuid::new_v4());
    subscription_patch(&pool, &prefix).await;
    pause_and_drain(&pool, &prefix).await;
    retention(&pool, &prefix).await;
    configured_pool(&url).await;
    broker_process(&url).await;
    pool.close().await;
}
fn sub(id: &str, entity_id: &str, endpoint: &str) -> Subscription {
    serde_json::from_value(json!({"id":id,"type":"Subscription","entities":[{"id":entity_id,"type":"OperationsSensor"}],"notification":{"endpoint":{"uri":endpoint}},"@context":{"temperature":"https://example.org/temperature"}})).unwrap()
}
async fn subscription_patch(pool: &sqlx::PgPool, prefix: &str) {
    let store = PgSubscriptionStore::new(pool.clone());
    let id = format!("{prefix}:patch");
    let mut subscription = sub(
        &id,
        &format!("{prefix}:unused"),
        "https://example.org/notify",
    );
    subscription.notification.times_sent = Some(500);
    store.create_subscription(&subscription).await.unwrap();
    let saved = store.get_subscription_by_id(&id).await.unwrap().unwrap();
    assert_eq!(saved.context, subscription.context);
    assert_eq!(
        saved.notification.times_sent, None,
        "client counters must be ignored"
    );
    store.record_notification_success(&id).await.unwrap();
    let patch_a = json!({"description":"concurrent"});
    let patch_b = json!({"throttling":0.25,"q":"temperature>20"});
    let patch_c = json!({"subscriptionName":"meter","notification":{"endpoint":{"uri":"https://example.org/updated"},"timesSent":900}});
    let (a, b, c) = tokio::join!(
        store.update_subscription(&id, &patch_a),
        store.update_subscription(&id, &patch_b),
        store.update_subscription(&id, &patch_c)
    );
    a.unwrap();
    b.unwrap();
    c.unwrap();
    let saved = store.get_subscription_by_id(&id).await.unwrap().unwrap();
    assert_eq!(saved.description.as_deref(), Some("concurrent"));
    assert_eq!(saved.subscription_name.as_deref(), Some("meter"));
    assert_eq!(saved.throttling, Some(0.25));
    assert_eq!(saved.notification.times_sent, Some(1));
    assert_eq!(
        saved.notification.endpoint.uri,
        "https://example.org/updated"
    );
    assert!(saved.notification.last_success.is_some());
    for patch in [
        json!({"id":"urn:changed"}),
        json!({"throttling":-1}),
        json!({"q":"("}),
        json!({"notification":{"format":"keyValues"}}),
        json!({"watchedAttributes":[]}),
        json!({"timeInterval":5}),
        json!({"typo":true}),
        json!({"isActive":"false"}),
        json!({"entities":[{"type":"Sensor","idPattern":"["}]}),
    ] {
        assert!(
            store.update_subscription(&id, &patch).await.is_err(),
            "{patch}"
        );
    }
    assert_eq!(
        store.get_subscription_by_id(&id).await.unwrap().unwrap(),
        saved,
        "invalid patches must roll back entirely"
    );
    store.update_subscription(&id,&json!({"description":"urn:ngsi-ld:null","throttling":"urn:ngsi-ld:null","status":"paused"})).await.unwrap();
    let saved = store.get_subscription_by_id(&id).await.unwrap().unwrap();
    assert!(saved.description.is_none());
    assert!(saved.throttling.is_none());
    assert_eq!(
        saved.status,
        SubscriptionStatus::Active,
        "output-only status is ignored"
    );
    assert!(matches!(
        store
            .update_subscription("urn:nonexistent", &json!({"description":"x"}))
            .await,
        Err(athena_storage::StorageError::SubscriptionNotFound(_))
    ));
    sqlx::query(
        "UPDATE subscriptions SET expires_at=clock_timestamp()-interval '1 second' WHERE id=$1",
    )
    .bind(&id)
    .execute(pool)
    .await
    .unwrap();
    store
        .update_subscription(&id, &json!({"isActive":true}))
        .await
        .unwrap();
    assert_eq!(
        store
            .get_subscription_by_id(&id)
            .await
            .unwrap()
            .unwrap()
            .status,
        SubscriptionStatus::Expired
    );
    store
        .update_subscription(
            &id,
            &json!({"expiresAt":(chrono::Utc::now()+chrono::Duration::days(1)).to_rfc3339()}),
        )
        .await
        .unwrap();
    assert_eq!(
        store
            .get_subscription_by_id(&id)
            .await
            .unwrap()
            .unwrap()
            .status,
        SubscriptionStatus::Active
    );
    api_subscription_contract(pool, prefix).await;
    store.delete_subscription(&id).await.unwrap();
}

async fn api_subscription_contract(pool: &sqlx::PgPool, prefix: &str) {
    use axum::{
        body::{to_bytes, Body},
        http::Request,
    };
    use tower::ServiceExt;
    let repo: Arc<dyn SubscriptionRepository> = Arc::new(PgSubscriptionStore::new(pool.clone()));
    let engine = Arc::new(athena_subscription::SubscriptionEngine::new(
        repo.clone(),
        pool.clone(),
        1,
    ));
    assert!(engine.shutdown(Duration::from_secs(2)).await);
    let mut state = athena_api::AppState::new(
        pool.clone(),
        Arc::new(PgEntityStore::new(pool.clone())),
        repo.clone(),
        Arc::new(athena_storage::PgTemporalStore::new(pool.clone())),
        Arc::new(athena_storage::PgCsourceStore::new(pool.clone())),
        engine,
        Arc::new(athena_jsonld::ContextResolver::default()),
    );
    state.write_slots = Arc::new(tokio::sync::Semaphore::new(1));
    let _saturated = state.write_slots.clone().acquire_owned().await.unwrap();
    state.limits.max_body_bytes = 2048;
    let app = athena_api::create_router(state);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/ngsi-ld/v1/entities")
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 503);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/ready")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        503,
        "stopped workers must make the broker unready"
    );

    let id = format!("{prefix}:api-sub");
    let input = json!({"id":id,"type":"Subscription","isActive":false,"watchedAttributes":["temperature"],"notification":{"endpoint":{"uri":"https://example.org/notify","receiverInfo":[{"key":"X-Sensor","value":"test"}]},"timesSent":999}});
    let req = |method: &str, uri: &str, value: &Value| {
        Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(value.to_string()))
            .unwrap()
    };
    let response = app
        .clone()
        .oneshot(req("POST", "/ngsi-ld/v1/subscriptions", &input))
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 100000).await.unwrap();
    assert_eq!(status, 201, "{}", String::from_utf8_lossy(&body));
    let saved = repo.get_subscription_by_id(&id).await.unwrap().unwrap();
    assert_eq!(saved.status, SubscriptionStatus::Paused);
    assert_eq!(
        saved.watched_attributes.unwrap()[0],
        "https://uri.etsi.org/ngsi-ld/default-context/temperature"
    );
    let uri = format!("/ngsi-ld/v1/subscriptions/{id}");
    let response=app.clone().oneshot(req("PATCH",&uri,&json!({"isActive":true,"throttling":0.05,"q":"temperature>18","notification":{"endpoint":{"uri":"https://example.org/new"},"attributes":["temperature"],"format":"simplified"}}))).await.unwrap();
    assert_eq!(response.status(), 204);
    let saved = repo.get_subscription_by_id(&id).await.unwrap().unwrap();
    assert_eq!(saved.status, SubscriptionStatus::Active);
    assert!(saved
        .q
        .unwrap()
        .contains("https://uri.etsi.org/ngsi-ld/default-context/temperature"));
    for patch in [
        json!({"notification":{"endpoint":{"uri":"http://127.0.0.1/notify"}}}),
        json!({"notification":{"endpoint":{"uri":"https://example.org","receiverInfo":[{"key":"Content-Length","value":"1"}]}}}),
        json!({"scopeQ":"/unsupported"}),
    ] {
        assert_eq!(
            app.clone()
                .oneshot(req("PATCH", &uri, &patch))
                .await
                .unwrap()
                .status(),
            400
        );
    }
    assert_eq!(
        app.oneshot(
            Request::builder()
                .uri(&uri)
                .header("accept", "application/ld+json")
                .body(Body::empty())
                .unwrap()
        )
        .await
        .unwrap()
        .status(),
        200
    );
    let oversized = json!({"description":"x".repeat(3000)});
    // Body caps apply to control operations even though they bypass ingestion admission.
    let state = athena_api::AppState::new(
        pool.clone(),
        Arc::new(PgEntityStore::new(pool.clone())),
        repo.clone(),
        Arc::new(athena_storage::PgTemporalStore::new(pool.clone())),
        Arc::new(athena_storage::PgCsourceStore::new(pool.clone())),
        Arc::new(athena_subscription::SubscriptionEngine::new(
            repo.clone(),
            pool.clone(),
            1,
        )),
        Arc::new(athena_jsonld::ContextResolver::default()),
    );
    let mut state = state;
    state.limits.max_body_bytes = 2048;
    assert_eq!(
        athena_api::create_router(state)
            .oneshot(req("PATCH", &uri, &oversized))
            .await
            .unwrap()
            .status(),
        413
    );
    repo.delete_subscription(&id).await.unwrap();
}

async fn pause_and_drain(pool: &sqlx::PgPool, prefix: &str) {
    let received = Arc::new(tokio::sync::Mutex::new(Vec::<Value>::new()));
    let started = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let r = received.clone();
    let s = started.clone();
    let gate = release.clone();
    let app = axum::Router::new().route(
        "/notify",
        axum::routing::post(move |axum::Json(body): axum::Json<Value>| {
            let r = r.clone();
            let s = s.clone();
            let gate = gate.clone();
            async move {
                let gated = body["data"][0]["id"].as_str().unwrap().ends_with(":drain");
                r.lock().await.push(body);
                if gated {
                    s.notify_one();
                    gate.notified().await;
                }
                axum::http::StatusCode::NO_CONTENT
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/notify", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let repo: Arc<dyn SubscriptionRepository> = Arc::new(PgSubscriptionStore::new(pool.clone()));
    let entities = PgEntityStore::new(pool.clone());
    let id = format!("{prefix}:paused");
    let entity_id = format!("{prefix}:queued");
    repo.create_subscription(&sub(&id, &entity_id, &endpoint))
        .await
        .unwrap();
    let make_entity = |id: &str| {
        Entity::from_json(
            json!({"id":id,"type":"OperationsSensor","temperature":{"type":"Property","value":25}}),
        )
        .unwrap()
    };
    entities
        .create_entity(&make_entity(&entity_id))
        .await
        .unwrap();
    for _ in 0..100 {
        if materialize_events(pool, &repo).await.unwrap() == 0 {
            break;
        }
    }
    repo.update_subscription(&id, &json!({"isActive":false}))
        .await
        .unwrap();
    let client = athena_http::OutboundClient::new(athena_http::OutboundPolicy {
        allow_private: true,
    })
    .unwrap();
    for _ in 0..3 {
        deliver_one(pool, &repo, &client).await.unwrap();
    }
    let row = sqlx::query("SELECT status,attempts FROM notification_jobs WHERE subscription_id=$1")
        .bind(&id)
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(row.get::<String, _>("status"), "pending");
    assert_eq!(row.get::<i32, _>("attempts"), 0);
    assert!(received.lock().await.is_empty());
    // A paused subscription must not block another subscriber sharing its endpoint.
    let second_id = format!("{prefix}:second-sub");
    let second_entity = format!("{prefix}:second");
    repo.create_subscription(&sub(&second_id, &second_entity, &endpoint))
        .await
        .unwrap();
    entities
        .create_entity(&make_entity(&second_entity))
        .await
        .unwrap();
    for _ in 0..100 {
        if materialize_events(pool, &repo).await.unwrap() == 0 {
            break;
        }
    }
    deliver_one(pool, &repo, &client).await.unwrap();
    assert_eq!(received.lock().await[0]["subscriptionId"], second_id);
    repo.update_subscription(&id, &json!({"isActive":true}))
        .await
        .unwrap();
    let (a, b, c) = tokio::join!(
        deliver_one(pool, &repo, &client),
        deliver_one(pool, &repo, &client),
        deliver_one(pool, &repo, &client)
    );
    a.unwrap();
    b.unwrap();
    c.unwrap();
    assert_eq!(
        received.lock().await.len(),
        2,
        "one claim per job under concurrent workers"
    );
    assert_eq!(received.lock().await[1]["subscriptionId"], id);
    // Start a real worker and stop it while its receiver is still processing.
    let drain_id = format!("{prefix}:drain-sub");
    let drain_entity = format!("{prefix}:drain");
    repo.create_subscription(&sub(&drain_id, &drain_entity, &endpoint))
        .await
        .unwrap();
    let engine = Arc::new(
        athena_subscription::SubscriptionEngine::with_config(
            repo.clone(),
            pool.clone(),
            athena_subscription::EngineConfig {
                worker_threads: 2,
                poll_interval_ms: 10,
                ..Default::default()
            },
            athena_http::OutboundPolicy {
                allow_private: true,
            },
        )
        .unwrap(),
    );
    entities
        .create_entity(&make_entity(&drain_entity))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), started.notified())
        .await
        .unwrap();
    let stop_engine = engine.clone();
    let shutdown = tokio::spawn(async move { stop_engine.shutdown(Duration::from_secs(5)).await });
    tokio::task::yield_now().await;
    release.notify_one();
    assert!(shutdown.await.unwrap());
    let status: String =
        sqlx::query_scalar("SELECT status FROM notification_jobs WHERE subscription_id=$1")
            .bind(&drain_id)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(status, "delivered");
    assert!(
        engine.shutdown(Duration::from_millis(10)).await,
        "shutdown is idempotent"
    );
    // Corrupt persisted work must reach a dead letter instead of retrying forever.
    let event: i64 = sqlx::query_scalar(
        "SELECT id FROM entity_events WHERE entity_id=$1 ORDER BY id DESC LIMIT 1",
    )
    .bind(&drain_entity)
    .fetch_one(pool)
    .await
    .unwrap();
    let poison: uuid::Uuid = sqlx::query_scalar("INSERT INTO notification_jobs(event_id,subscription_id,entity_id,endpoint,subscription,notification) VALUES($1,$2,$2,'https://example.org/poison','{}','{}') RETURNING id").bind(event).bind(format!("{prefix}:poison")).fetch_one(pool).await.unwrap();
    let config = athena_subscription::EngineConfig {
        max_attempts: 2,
        ..Default::default()
    };
    for _ in 0..2 {
        sqlx::query("UPDATE notification_jobs SET available_at=clock_timestamp()-interval '1 second' WHERE id=$1").bind(poison).execute(pool).await.unwrap();
        athena_subscription::engine::deliver_one_with_config(pool, &repo, &client, &config)
            .await
            .unwrap();
    }
    let row = sqlx::query("SELECT status,attempts,completed_at IS NOT NULL AS completed FROM notification_jobs WHERE id=$1").bind(poison).fetch_one(pool).await.unwrap();
    assert_eq!(row.get::<String, _>("status"), "dead");
    assert_eq!(row.get::<i32, _>("attempts"), 2);
    assert!(row.get::<bool, _>("completed"));
    sqlx::query("DELETE FROM notification_jobs WHERE id=$1")
        .bind(poison)
        .execute(pool)
        .await
        .unwrap();
    for id in [&id, &second_id, &drain_id] {
        repo.delete_subscription(id).await.unwrap();
    }
    server.abort();
}

async fn retention(pool: &sqlx::PgPool, prefix: &str) {
    use athena_storage::maintenance::{cleanup_once, RetentionConfig};
    let mut ids = Vec::new();
    for suffix in ["complete", "protected", "pending", "dead"] {
        let id:i64=sqlx::query_scalar("INSERT INTO entity_events(entity_id,revision,operation,entity,changed_attrs,processed_at,last_error) VALUES($1,1,'insert','{}',ARRAY[]::text[],CASE WHEN $2='pending' THEN NULL ELSE clock_timestamp()-interval '30 days' END,CASE WHEN $2='dead' THEN 'failed' ELSE NULL END) RETURNING id")
            .bind(format!("{prefix}:{suffix}")).bind(suffix).fetch_one(pool).await.unwrap();
        ids.push(id);
    }
    let mut jobs = Vec::new();
    for (event, status) in [(ids[0], "delivered"), (ids[1], "pending"), (ids[3], "dead")] {
        let id:uuid::Uuid=sqlx::query_scalar("INSERT INTO notification_jobs(event_id,subscription_id,entity_id,endpoint,subscription,notification,status,completed_at) VALUES($1,$2,$2,'https://example.org','{}','{}',$3,CASE WHEN $3='pending' THEN NULL ELSE clock_timestamp()-interval '30 days' END) RETURNING id")
            .bind(event).bind(prefix).bind(status).fetch_one(pool).await.unwrap();
        jobs.push(id);
    }
    let entity_id = format!("{prefix}:retained-history");
    PgEntityStore::new(pool.clone()).create_entity(&Entity::from_json(json!({"id":entity_id,"type":"OperationsSensor","reading":{"type":"Property","value":3,"observedAt":"1900-01-01T00:00:00Z"}})).unwrap()).await.unwrap();
    let cfg = RetentionConfig {
        batch_size: 1,
        ..Default::default()
    };
    let result = cleanup_once(pool, &cfg).await.unwrap();
    assert_eq!(result.jobs, 1);
    assert_eq!(result.events, 1);
    assert_eq!(result.history, 0);
    let remaining: Vec<i64> =
        sqlx::query_scalar("SELECT id FROM entity_events WHERE id=ANY($1) ORDER BY id")
            .bind(&ids)
            .fetch_all(pool)
            .await
            .unwrap();
    assert_eq!(remaining, vec![ids[1], ids[2], ids[3]]);
    let remaining_jobs: Vec<uuid::Uuid> =
        sqlx::query_scalar("SELECT id FROM notification_jobs WHERE id=ANY($1)")
            .bind(&jobs)
            .fetch_all(pool)
            .await
            .unwrap();
    assert_eq!(
        remaining_jobs.len(),
        2,
        "pending and dead jobs survive default retention"
    );
    let enabled = RetentionConfig {
        history_seconds: 86400,
        dead_seconds: 86400,
        ..cfg
    };
    let result = cleanup_once(pool, &enabled).await.unwrap();
    assert_eq!(result.jobs, 1);
    assert_eq!(
        result.history, 0,
        "retention must use ingest time, not old device observedAt"
    );
    sqlx::query("UPDATE entity_temporal SET recorded_at=clock_timestamp()-interval '30 days' WHERE entity_id=$1").bind(&entity_id).execute(pool).await.unwrap();
    let result = cleanup_once(pool, &enabled).await.unwrap();
    assert_eq!(result.history, 1);
    assert!(
        PgEntityStore::new(pool.clone())
            .get_entity_by_id(&entity_id, None)
            .await
            .unwrap()
            .is_some(),
        "history retention cannot delete live entities"
    );
    // Remove only this intentionally incomplete fixture, so subsequent delivery tests stay isolated.
    sqlx::query("DELETE FROM notification_jobs WHERE id=ANY($1)")
        .bind(&jobs)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM entity_events WHERE id=ANY($1)")
        .bind(&ids)
        .execute(pool)
        .await
        .unwrap();
}

async fn configured_pool(url: &str) {
    let pool = athena_storage::create_connection_pool(&athena_storage::DatabaseConfig {
        url: url.into(),
        max_connections: 3,
        statement_timeout_ms: 3210,
        lock_timeout_ms: 1230,
        ..Default::default()
    })
    .await
    .unwrap();
    let row=sqlx::query("SELECT current_setting('statement_timeout') AS statement,current_setting('lock_timeout') AS lock").fetch_one(&pool).await.unwrap();
    assert_eq!(row.get::<String, _>("statement"), "3210ms");
    assert_eq!(row.get::<String, _>("lock"), "1230ms");
    pool.close().await;
}

#[cfg(unix)]
async fn broker_process(url: &str) {
    let bin = env!("CARGO_BIN_EXE_athena-broker");
    let port_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = port_listener.local_addr().unwrap().port();
    drop(port_listener);
    let path =
        std::env::temp_dir().join(format!("athena-test-config-{}.toml", uuid::Uuid::new_v4()));
    let settings = json!({"server":{"host":"127.0.0.1","port":port,"shutdown_grace_sec":3},"database":{"url":url,"max_connections":4},"subscriptions":{"worker_threads":1}});
    std::fs::write(&path, toml::to_string(&settings).unwrap()).unwrap();
    let output = tokio::process::Command::new(bin)
        .env_clear()
        .arg("--config")
        .arg(&path)
        .arg("--check-config")
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains(url));
    let invalid = tokio::process::Command::new(bin)
        .env_clear()
        .env("PORT", "wrong")
        .arg("--check-config")
        .output()
        .await
        .unwrap();
    assert!(!invalid.status.success());
    let mut child = tokio::process::Command::new(bin)
        .env_clear()
        .arg("--config")
        .arg(&path)
        .kill_on_drop(true)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if client
                .get(format!("http://127.0.0.1:{port}/ready"))
                .send()
                .await
                .is_ok_and(|r| r.status().is_success())
            {
                break;
            }
            assert!(
                child.try_wait().unwrap().is_none(),
                "broker exited before readiness"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let health = tokio::process::Command::new(bin)
        .env_clear()
        .env("ATHENA_CONFIG", &path)
        .arg("--healthcheck")
        .output()
        .await
        .unwrap();
    assert!(
        health.status.success(),
        "{}",
        String::from_utf8_lossy(&health.stderr)
    );
    let started = std::time::Instant::now();
    assert!(tokio::process::Command::new("kill")
        .args(["-TERM", &child.id().unwrap().to_string()])
        .status()
        .await
        .unwrap()
        .success());
    let output = tokio::time::timeout(Duration::from_secs(5), child.wait_with_output())
        .await
        .unwrap()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(started.elapsed() < Duration::from_secs(5));
    std::fs::remove_file(path).unwrap();
}
#[cfg(not(unix))]
async fn broker_process(_url: &str) {}
