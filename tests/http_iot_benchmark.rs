//! Reproducible local HTTP -> PostgreSQL -> history/outbox -> HTTP receiver workload.
//! Not a production sizing result or a comparison with another broker.
use athena_storage::{PgEntityStore, PgSubscriptionStore, SubscriptionRepository};
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    sync::Arc,
    time::{Duration, Instant},
};

#[tokio::test]
#[ignore = "explicit end-to-end benchmark using isolated PostGIS"]
async fn http_ingestion_history_and_fanout() {
    let url = std::env::var("ATHENA_TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres@127.0.0.1:55432/athena_test".into());
    assert!(url.ends_with("/athena_test"));
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(16)
        .connect(&url)
        .await
        .unwrap();
    athena_storage::run_migrations(&pool).await.unwrap();
    let repo: Arc<dyn SubscriptionRepository> = Arc::new(PgSubscriptionStore::new(pool.clone()));
    // Previous interrupted runs own this isolated fixture namespace. Mark their jobs
    // terminal so dead local receivers do not compete with the next measurement.
    sqlx::query("UPDATE notification_jobs SET status='dead',last_error='interrupted benchmark fixture',lease_token=NULL,lease_until=NULL WHERE status='pending' AND subscription_id LIKE 'urn:ngsi-ld:Subscription:HttpBench%'").execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM subscriptions WHERE id LIKE 'urn:ngsi-ld:Subscription:HttpBench%'")
        .execute(&pool)
        .await
        .unwrap();
    // Drain unrelated fixtures before starting this measurement.
    for _ in 0..1000 {
        if athena_subscription::engine::materialize_events(&pool, &repo)
            .await
            .unwrap()
            == 0
        {
            break;
        }
    }
    let received = Arc::new(tokio::sync::Mutex::new((
        HashSet::<String>::new(),
        Vec::<f64>::new(),
    )));
    let receipts = received.clone();
    let receiver = axum::Router::new().route(
        "/notify/:receiver",
        axum::routing::post(move |axum::Json(body): axum::Json<Value>| {
            let receipts = receipts.clone();
            async move {
                let observed = body["data"][0]
                    ["https://uri.etsi.org/ngsi-ld/default-context/reading"]["observedAt"]
                    .as_str()
                    .unwrap()
                    .parse::<chrono::DateTime<chrono::Utc>>()
                    .unwrap();
                let latency =
                    (chrono::Utc::now() - observed).num_microseconds().unwrap() as f64 / 1000.;
                let mut receipts = receipts.lock().await;
                receipts.0.insert(body["id"].as_str().unwrap().into());
                receipts.1.push(latency);
                axum::http::StatusCode::NO_CONTENT
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let receiver_url = format!("http://{}", listener.local_addr().unwrap());
    let receiver_task = tokio::spawn(async move { axum::serve(listener, receiver).await.unwrap() });
    let kind = format!("HttpBench{}", uuid::Uuid::new_v4().simple());
    let mut subscription_ids = Vec::new();
    for i in 0..5 {
        let id = format!("urn:ngsi-ld:Subscription:{kind}:{i}");
        let sub=serde_json::from_value(json!({"id":id,"type":"Subscription","entities":[{"type":format!("https://uri.etsi.org/ngsi-ld/default-context/{kind}")}],"notification":{"endpoint":{"uri":format!("{receiver_url}/notify/{i}")}}})).unwrap();
        repo.create_subscription(&sub).await.unwrap();
        subscription_ids.push(id);
    }
    let engine = Arc::new(
        athena_subscription::SubscriptionEngine::with_config(
            repo.clone(),
            pool.clone(),
            athena_subscription::EngineConfig {
                worker_threads: 4,
                poll_interval_ms: 10,
                ..Default::default()
            },
            athena_http::OutboundPolicy {
                allow_private: true,
            },
        )
        .unwrap(),
    );
    let app = athena_api::create_router(athena_api::AppState::new(
        pool.clone(),
        Arc::new(PgEntityStore::new(pool.clone())),
        repo.clone(),
        Arc::new(athena_storage::PgTemporalStore::new(pool.clone())),
        Arc::new(athena_storage::PgCsourceStore::new(pool.clone())),
        engine.clone(),
        Arc::new(athena_jsonld::ContextResolver::default()),
    ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let broker_url = format!("http://{}", listener.local_addr().unwrap());
    let broker_task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap();
    let started = Instant::now();
    let mut request_ms = Vec::new();
    for (round, operation) in ["create", "upsert"].iter().enumerate() {
        let gate = Arc::new(tokio::sync::Semaphore::new(4));
        let mut requests = tokio::task::JoinSet::new();
        for batch in 0..10 {
            let gate = gate.clone();
            let client = client.clone();
            let kind = kind.clone();
            let endpoint = format!("{broker_url}/ngsi-ld/v1/entityOperations/{operation}");
            requests.spawn(async move {
                let _permit=gate.acquire_owned().await.unwrap();
                let observed=chrono::Utc::now().to_rfc3339();
                let entities:Vec<Value>=(batch*50..(batch+1)*50).map(|n|json!({"id":format!("urn:ngsi-ld:{kind}:{n}"),"type":kind,"reading":{"type":"Property","value":round,"observedAt":observed}})).collect();
                let start=Instant::now();
                let response=client.post(endpoint).json(&entities).send().await.unwrap();
                let status=response.status();let body=response.text().await.unwrap();
                assert!(status.is_success(),"{status}: {body}");
                start.elapsed().as_secs_f64()*1000.
            });
        }
        while let Some(result) = requests.join_next().await {
            request_ms.push(result.unwrap());
        }
    }
    let write_seconds = started.elapsed().as_secs_f64();
    let completed = tokio::time::timeout(Duration::from_secs(60),async {
        loop {
            let delivered:i64=sqlx::query_scalar("SELECT count(*) FROM notification_jobs WHERE subscription_id=ANY($1) AND status='delivered'").bind(&subscription_ids).fetch_one(&pool).await.unwrap();
            if delivered==5000 {break;}
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }).await;
    if completed.is_err() {
        let rows:Vec<(String,i64,Option<String>)>=sqlx::query_as("SELECT status,count(*),min(last_error) FROM notification_jobs WHERE subscription_id=ANY($1) GROUP BY status").bind(&subscription_ids).fetch_all(&pool).await.unwrap();
        panic!("notification drain deadline; jobs={rows:?}");
    }
    let pipeline_seconds = started.elapsed().as_secs_f64();
    let ids: Vec<String> = (0..500)
        .map(|n| format!("urn:ngsi-ld:{kind}:{n}"))
        .collect();
    let history: i64 =
        sqlx::query_scalar("SELECT count(*) FROM entity_temporal WHERE entity_id=ANY($1)")
            .bind(&ids)
            .fetch_one(&pool)
            .await
            .unwrap();
    let events: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM entity_events WHERE entity_id=ANY($1) AND processed_at IS NOT NULL",
    )
    .bind(&ids)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(history, 1000);
    assert_eq!(events, 1000);
    let mut received = received.lock().await;
    assert_eq!(received.0.len(), 5000);
    assert_eq!(received.1.len(), 5000);
    request_ms.sort_by(f64::total_cmp);
    received.1.sort_by(f64::total_cmp);
    let percentile =
        |values: &[f64], p: f64| values[((values.len() - 1) as f64 * p).round() as usize];
    println!(
        "{}",
        json!({"workload":"HTTP writes + history + fanout","entities":500,"entity_mutations":1000,"batch_size":50,"http_concurrency":4,"notification_workers":4,"receivers":5,"notifications":5000,"history_instances":history,"write_entities_per_second":1000./write_seconds,"batch_request_p50_ms":percentile(&request_ms,0.5),"batch_request_p95_ms":percentile(&request_ms,0.95),"pipeline_seconds":pipeline_seconds,"notification_latency_p50_ms":percentile(&received.1,0.5),"notification_latency_p95_ms":percentile(&received.1,0.95),"production_sizing":false})
    );
    drop(received);
    assert!(engine.shutdown(Duration::from_secs(5)).await);
    broker_task.abort();
    receiver_task.abort();
    for id in subscription_ids {
        repo.delete_subscription(&id).await.unwrap();
    }
    pool.close().await;
}
