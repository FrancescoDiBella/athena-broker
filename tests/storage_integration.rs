use athena_model::Entity;
use athena_storage::{EntityRepository, PgEntityStore};
use serde_json::json;
use sqlx::Row;

/// Never runs against DATABASE_URL or the normal broker database.
#[tokio::test]
#[ignore = "requires the isolated database from scripts/test-integration.sh"]
async fn storage_transactions_history_and_sql() {
    let url = std::env::var("ATHENA_TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres@127.0.0.1:55432/athena_test".into());
    assert!(
        url.ends_with("/athena_test"),
        "Only the isolated athena_test database is permitted"
    );
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await
        .unwrap();
    athena_storage::run_migrations(&pool).await.unwrap();
    athena_storage::run_migrations(&pool).await.unwrap();
    let repo = PgEntityStore::new(pool.clone());
    let prefix = format!("urn:ngsi-ld:Test:{}", uuid::Uuid::new_v4());
    let entity = |suffix: &str, speed: serde_json::Value| {
        Entity::from_json(json!({
            "id": format!("{prefix}:{suffix}"), "type": "TestSensor",
            "speed": {"type":"Property", "value":speed, "observedAt":"2026-09-22T00:00:00Z"}
        }))
        .unwrap()
    };
    let a = entity("a", json!(42));
    let b = entity("b", json!(12));
    let c = entity("c", json!("not-a-number"));
    repo.create_entity(&a).await.unwrap();
    let result = repo
        .bulk_write_atomic(&[b.clone(), a.clone(), c.clone()], false)
        .await
        .unwrap();
    assert_eq!(result.success.len(), 2);
    assert_eq!(result.errors.len(), 1);
    assert!(repo.get_entity_by_id(&b.id, None).await.unwrap().is_some());
    assert!(repo.get_entity_by_id(&c.id, None).await.unwrap().is_some());
    let count: i64 =
        sqlx::query("SELECT count(*) AS n FROM entity_events WHERE entity_id = ANY($1)")
            .bind(vec![a.id.clone(), b.id.clone(), c.id.clone()])
            .fetch_one(&pool)
            .await
            .unwrap()
            .try_get("n")
            .unwrap();
    assert_eq!(
        count, 3,
        "Rolled-back batch entries must not produce events"
    );
    let count: i64 =
        sqlx::query("SELECT count(*) AS n FROM entity_temporal WHERE entity_id = ANY($1)")
            .bind(vec![a.id.clone(), b.id.clone(), c.id.clone()])
            .fetch_one(&pool)
            .await
            .unwrap()
            .try_get("n")
            .unwrap();
    assert_eq!(
        count, 3,
        "All ordinary writes must record history atomically"
    );
    let params = athena_storage::EntityQueryParams {
        id_pattern: Some(format!("^{prefix}:")),
        q: Some(athena_query::Parser::parse_str("speed>20").unwrap()),
        ..Default::default()
    };
    let values = repo.query_entities(&params).await.unwrap();
    assert_eq!(
        values.len(),
        1,
        "Numeric queries must not fail on heterogeneous values"
    );
    assert_eq!(values[0].id, a.id);
    let geo = athena_query::GeoQueryParser::parse(
        Some("within"),
        Some("Point"),
        Some("[0,0]"),
        Some("x'); SELECT pg_sleep(60); --"),
    )
    .unwrap()
    .unwrap();
    let params = athena_storage::EntityQueryParams {
        geo_q: Some(geo),
        ..Default::default()
    };
    assert!(repo.query_entities(&params).await.unwrap().is_empty());
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("DELETE FROM entities WHERE id = $1")
        .bind(&a.id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    assert!(repo.get_entity_by_id(&a.id, None).await.unwrap().is_some());
    let after: i64 = sqlx::query_scalar("SELECT count(*) FROM entity_events WHERE entity_id = $1")
        .bind(&a.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(after, 1, "Rollback must also roll back delete events");
    let multi = entity("multi", json!(1));
    repo.create_entity(&multi).await.unwrap();
    repo.append_entity_attrs(
        &multi.id,
        &json!({"speed":{"type":"Property","value":2,"datasetId":"urn:dataset:other"}}),
        true,
    )
    .await
    .unwrap();
    repo.update_entity_attrs(
        &multi.id,
        &json!({"speed":{"type":"Property","value":3},"@context":{}}),
    )
    .await
    .unwrap();
    let stored = repo
        .get_entity_by_id(&multi.id, None)
        .await
        .unwrap()
        .unwrap();
    let instances = stored.attributes["speed"].as_array().unwrap();
    assert_eq!(instances.len(), 2);
    assert!(instances
        .iter()
        .any(|v| v["datasetId"] == "urn:dataset:other" && v["value"] == 2));
    assert!(!stored.attributes.contains_key("@context"));
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM entity_temporal WHERE entity_id=$1")
        .bind(&multi.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        rows, 3,
        "Unchanged datasets must not multiply history writes"
    );
    let mut malformed = entity("invalid", json!(9));
    malformed.attributes.get_mut("speed").unwrap()["observedAt"] = json!("invalid date");
    let good = entity("valid-before-error", json!(4));
    let good2 = entity("valid-after-error", json!(5));
    let result = repo
        .batch_create(&[good.clone(), malformed, good2.clone()])
        .await
        .unwrap();
    assert_eq!(result.success.len(), 2);
    assert_eq!(result.errors.len(), 1);
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM entity_events WHERE entity_id=ANY($1)")
            .bind(vec![good.id, good2.id])
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        count, 2,
        "Bulk fallback/savepoints must neither lose nor duplicate events"
    );
    let matches = repo
        .query_entities(&athena_storage::EntityQueryParams {
            id: Some(multi.id.clone()),
            q: Some(athena_query::Parser::parse_str("speed==2").unwrap()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(matches.len(), 1, "q must inspect every dataset instance");
    repo.delete_entity_attr_instance(&multi.id, "speed", None, false)
        .await
        .unwrap();
    let remaining = repo
        .get_entity_by_id(&multi.id, None)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        remaining.attributes["speed"]["datasetId"],
        "urn:dataset:other"
    );
    assert!(repo
        .delete_entity_attr_instance(&multi.id, "speed", None, false)
        .await
        .is_err());
    temporal_regressions(&pool, &prefix).await;
    durable_delivery(&pool, &repo, &prefix).await;
    api_contract(&pool, &prefix).await;
    pool.close().await;
}

async fn temporal_regressions(pool: &sqlx::PgPool, prefix: &str) {
    use athena_model::{AggrMethod, TemporalQuery, TimeProperty, TimeRel};
    use athena_storage::TemporalRepository;
    let temporal = athena_storage::PgTemporalStore::new(pool.clone());
    let id = format!("{prefix}:history-only");
    temporal.create_temporal_entity(&id, "HistoricalSensor", &json!({
        "temperature": [
            {"type":"Property","value":10,"datasetId":"urn:dataset:a","observedAt":"2026-09-21T00:00:00Z"},
            {"type":"Property","value":20,"datasetId":"urn:dataset:a","observedAt":"2026-09-22T00:00:00Z"},
            {"type":"Property","value":30,"datasetId":"urn:dataset:b","observedAt":"2026-09-22T00:00:00Z"}
        ],
        "owner": {"type":"Relationship","object":"urn:person:one","observedAt":"2026-09-22T00:00:00Z"}
    })).await.unwrap();
    let mut query = TemporalQuery {
        ids: None,
        id_pattern: None,
        q: None,
        geo_q: None,
        timerel: TimeRel::After,
        time_at: "2020-01-01T00:00:00Z".parse().unwrap(),
        end_time_at: None,
        timeproperty: TimeProperty::ObservedAt,
        aggr_method: None,
        aggr_methods: vec![],
        aggr_period_duration: None,
        last_n: Some(1),
    };
    let result = temporal.query_temporal(&id, None, &query).await.unwrap();
    assert_eq!(
        result["temperature"].as_array().unwrap().len(),
        2,
        "lastN applies per dataset"
    );
    assert_eq!(result["owner"][0]["object"], "urn:person:one");
    assert_eq!(result["type"], "HistoricalSensor");
    for property in [TimeProperty::CreatedAt, TimeProperty::ModifiedAt] {
        query.timeproperty = property;
        assert_eq!(
            temporal.query_temporal(&id, None, &query).await.unwrap()["temperature"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }
    query.timeproperty = TimeProperty::ObservedAt;
    query.last_n = None;
    query.aggr_method = Some(AggrMethod::TotalCount);
    let result = temporal
        .query_temporal(&id, Some(&["temperature".into()]), &query)
        .await
        .unwrap();
    let values = result["temperature"].as_array().unwrap();
    assert!(
        values.iter().any(|v| v["totalCount"][0][0] == 2),
        "Counts must decode as JSON integers: {result}"
    );
    query.aggr_method = None;
    let entities = temporal
        .query_temporal_entities(Some("HistoricalSensor"), None, &query, 1000, 0)
        .await
        .unwrap();
    assert!(
        entities.iter().any(|v| v["id"] == id),
        "History-only entities must be discoverable"
    );
    query.aggr_methods = vec![AggrMethod::TotalCount, AggrMethod::Sum];
    let aggregated = temporal
        .query_temporal(&id, Some(&["temperature".into()]), &query)
        .await
        .unwrap();
    assert!(
        aggregated["temperature"]
            .as_array()
            .unwrap()
            .iter()
            .all(|v| v.get("totalCount").is_some() && v.get("sum").is_some()),
        "{aggregated}"
    );
    query.aggr_methods.clear();
    let result = temporal.query_temporal(&id, None, &query).await.unwrap();
    let iid = result["temperature"][0]["instanceId"]
        .as_str()
        .unwrap()
        .to_owned();
    temporal
        .update_temporal_instance(&id, "temperature", &iid, &json!({"value":99}))
        .await
        .unwrap();
    let result = temporal.query_temporal(&id, None, &query).await.unwrap();
    assert!(result["temperature"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["value"] == 99));
    temporal
        .delete_temporal(&id, Some("temperature"), None, Some(&iid), true)
        .await
        .unwrap();
    assert_eq!(
        temporal.query_temporal(&id, None, &query).await.unwrap()["temperature"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    temporal
        .delete_temporal(&id, None, None, None, true)
        .await
        .unwrap();
    assert!(temporal.query_temporal(&id, None, &query).await.is_err());
}

async fn durable_delivery(pool: &sqlx::PgPool, entities: &PgEntityStore, prefix: &str) {
    use athena_storage::SubscriptionRepository;
    use athena_subscription::engine::{deliver_one, materialize_events};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    let attempts = Arc::new(AtomicUsize::new(0));
    let received = Arc::new(tokio::sync::Mutex::new(Vec::<serde_json::Value>::new()));
    let a = attempts.clone();
    let r = received.clone();
    let app = axum::Router::new().route(
        "/notify",
        axum::routing::post(move |axum::Json(value): axum::Json<serde_json::Value>| {
            let a = a.clone();
            let r = r.clone();
            async move {
                r.lock().await.push(value);
                if a.fetch_add(1, Ordering::SeqCst) == 0 {
                    axum::http::StatusCode::SERVICE_UNAVAILABLE
                } else {
                    axum::http::StatusCode::NO_CONTENT
                }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/notify", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let repo: Arc<dyn SubscriptionRepository> =
        Arc::new(athena_storage::PgSubscriptionStore::new(pool.clone()));
    let id = format!("{prefix}:subscription");
    let entity_id = format!("{prefix}:delivery");
    let sub=serde_json::from_value(json!({"id":id,"type":"Subscription","entities":[{"id":entity_id,"type":"DeliverySensor"}],"notification":{"endpoint":{"uri":endpoint}}})).unwrap();
    repo.create_subscription(&sub).await.unwrap();
    let entity = Entity::from_json(
        json!({"id":entity_id,"type":"DeliverySensor","value":{"type":"Property","value":5}}),
    )
    .unwrap();
    entities.create_entity(&entity).await.unwrap();
    // No in-memory event is submitted: materialization recovers committed work.
    for _ in 0..100 {
        if materialize_events(pool, &repo).await.unwrap() == 0 {
            break;
        }
    }
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM notification_jobs WHERE subscription_id=$1")
            .bind(&id)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(count, 1);
    materialize_events(pool, &repo).await.unwrap();
    let blocked = athena_http::OutboundClient::new(athena_http::OutboundPolicy::default()).unwrap();
    let hostname = endpoint.replace("127.0.0.1", "localhost");
    assert!(
        blocked
            .request(reqwest::Method::POST, &hostname)
            .unwrap()
            .send()
            .await
            .is_err(),
        "The connection resolver must reject private DNS answers"
    );
    let client = athena_http_for_test();
    assert!(deliver_one(pool, &repo, &client).await.unwrap());
    let row = sqlx::query("SELECT status,attempts FROM notification_jobs WHERE subscription_id=$1")
        .bind(&id)
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(row.get::<String, _>("status"), "pending");
    assert_eq!(row.get::<i32, _>("attempts"), 1);
    // Simulate an expired claim left by a killed worker, without sleeping 30 seconds.
    sqlx::query("UPDATE notification_jobs SET available_at=now()-interval '1 second',lease_token=gen_random_uuid(),lease_until=now()-interval '1 second' WHERE subscription_id=$1").bind(&id).execute(pool).await.unwrap();
    assert!(deliver_one(pool, &repo, &client).await.unwrap());
    let status: String =
        sqlx::query_scalar("SELECT status FROM notification_jobs WHERE subscription_id=$1")
            .bind(&id)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(status, "delivered");
    let payloads = received.lock().await;
    assert_eq!(payloads.len(), 2);
    assert_eq!(
        payloads[0]["id"], payloads[1]["id"],
        "Retry must retain notification ID for receiver deduplication"
    );
    assert_eq!(payloads[1]["data"][0]["value"]["value"], 5);
    let poisoned:i64=sqlx::query_scalar("INSERT INTO entity_events(entity_id,revision,operation,entity,changed_attrs) VALUES($1,1,'insert','{}'::jsonb,ARRAY[]::text[]) RETURNING id")
        .bind(format!("{prefix}:poisoned-event")).fetch_one(pool).await.unwrap();
    for _ in 0..12 {
        sqlx::query("UPDATE entity_events SET lease_until=now()-interval '1 second' WHERE id=$1")
            .bind(poisoned)
            .execute(pool)
            .await
            .unwrap();
        materialize_events(pool, &repo).await.unwrap();
    }
    let dead:bool=sqlx::query_scalar("SELECT processed_at IS NOT NULL AND last_error IS NOT NULL AND attempts=12 FROM entity_events WHERE id=$1").bind(poisoned).fetch_one(pool).await.unwrap();
    assert!(
        dead,
        "An invalid event must be retained for diagnosis without blocking the queue forever"
    );
    repo.delete_subscription(&id).await.unwrap();
    server.abort();
}

fn athena_http_for_test() -> athena_http::OutboundClient {
    athena_http::OutboundClient::new(athena_http::OutboundPolicy {
        allow_private: true,
    })
    .unwrap()
}

async fn api_contract(pool: &sqlx::PgPool, prefix: &str) {
    use axum::{
        body::{to_bytes, Body},
        http::Request,
    };
    use std::sync::Arc;
    use tower::ServiceExt;
    let subscriptions: Arc<dyn athena_storage::SubscriptionRepository> =
        Arc::new(athena_storage::PgSubscriptionStore::new(pool.clone()));
    let engine = Arc::new(athena_subscription::SubscriptionEngine::new(
        subscriptions.clone(),
        pool.clone(),
        1,
    ));
    let state = athena_api::AppState::new(
        pool.clone(),
        Arc::new(PgEntityStore::new(pool.clone())),
        subscriptions,
        Arc::new(athena_storage::PgTemporalStore::new(pool.clone())),
        Arc::new(athena_storage::PgCsourceStore::new(pool.clone())),
        engine,
        Arc::new(athena_jsonld::ContextResolver::default()),
    );
    let app = athena_api::create_router(state);
    let id = format!("{prefix}:api");
    let value = json!({"id":id,"type":"ApiSensor","temperature":{"type":"Property","value":21}});
    let request = Request::builder()
        .method("POST")
        .uri("/ngsi-ld/v1/entities")
        .header("content-type", "application/json")
        .body(Body::from(value.to_string()))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(
        response.status(),
        201,
        "{}",
        String::from_utf8_lossy(&to_bytes(response.into_body(), 100000).await.unwrap())
    );
    let request = Request::builder()
        .uri(format!("/ngsi-ld/v1/entities/{id}"))
        .header("accept", "application/ld+json")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["content-type"], "application/ld+json");
    assert!(!response.headers().contains_key("link"));
    let result: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 100000).await.unwrap()).unwrap();
    assert_eq!(result["temperature"]["value"], 21, "{result}");
    assert_eq!(result["type"], "ApiSensor");
    assert!(result.get("@context").is_some());
    let uri = format!(
        "/ngsi-ld/v1/entities?local=true&type=ApiSensor&q=temperature%3E20&id={id}&count=true"
    );
    let response = app
        .clone()
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["NGSILD-Results-Count"], "1");
    let result: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 100000).await.unwrap()).unwrap();
    assert_eq!(
        result.as_array().unwrap().len(),
        1,
        "Context expansion must also apply to q and type"
    );
    for (content, accept, status) in [
        ("application/ld+json", "application/json", 400),
        ("text/plain", "application/json", 415),
        ("application/json", "text/html", 406),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/ngsi-ld/v1/entities")
                    .header("content-type", content)
                    .header("accept", accept)
                    .body(Body::from(value.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), status);
    }
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
    assert_eq!(response.status(), 200);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let text = String::from_utf8(
        to_bytes(response.into_body(), 100000)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(text.contains("athena_db_up 1"));
    assert!(text.contains("athena_http_requests_total"));
}
