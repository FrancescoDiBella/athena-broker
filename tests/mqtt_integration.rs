//! Run against scripts/start-mqtt-test.sh plus the isolated PostGIS database.
use athena_http::{OutboundClient, OutboundPolicy};
use athena_model::{Entity, Notification, Subscription};
use athena_storage::{
    EntityRepository, PgEntityStore, PgSubscriptionStore, SubscriptionRepository,
};
use athena_subscription::{
    dispatcher::NotificationDispatcher,
    engine::{deliver_one, materialize_events},
    mqtt::MqttClient,
};
use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS};
use serde_json::{json, Value};
use sqlx::Row;
use std::{sync::Arc, time::Duration};

#[tokio::test]
#[ignore = "isolated PostGIS and local Mosquitto test service"]
async fn mqtt_versions_qos_tls_and_durable_retry() {
    let port = std::env::var("ATHENA_TEST_MQTT_PORT")
        .unwrap_or_else(|_| "18884".into())
        .parse::<u16>()
        .unwrap();
    let topic = format!("athena-test/{}", uuid::Uuid::new_v4());
    let options = MqttOptions::new(format!("test-{}", uuid::Uuid::new_v4()), "127.0.0.1", port);
    let (subscriber, mut events) = AsyncClient::new(options, 16);
    subscriber
        .subscribe(format!("{topic}/#"), QoS::ExactlyOnce)
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !matches!(
            events.poll().await.unwrap(),
            Event::Incoming(Packet::SubAck(_))
        ) {}
    })
    .await
    .unwrap();
    let (sent, mut received) = tokio::sync::mpsc::channel::<Value>(32);
    let reader = tokio::spawn(async move {
        loop {
            match events.poll().await {
                Ok(Event::Incoming(Packet::Publish(p))) => {
                    sent.send(serde_json::from_slice(&p.payload).unwrap())
                        .await
                        .unwrap();
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });
    let client = OutboundClient::with_timeout(
        OutboundPolicy {
            allow_private: true,
        },
        Duration::from_secs(3),
    )
    .unwrap();
    let notification = serde_json::to_value(Notification::new(
        "urn:ngsi-ld:Subscription:Mqtt",
        vec![json!({"id":"urn:ngsi-ld:Sensor:Mqtt","type":"Sensor"})],
    ))
    .unwrap();
    let mut sub:Subscription=serde_json::from_value(json!({"id":"urn:ngsi-ld:Subscription:Mqtt","type":"Subscription","entities":[{"type":"Sensor"}],"notification":{"endpoint":{"uri":format!("mqtt://127.0.0.1:{port}/{topic}/events"),"receiverInfo":[{"key":"X-Trace","value":"mqtt-test"}]}}})).unwrap();
    for version in ["mqtt3.1.1", "mqtt5.0"] {
        for qos in 0..=2 {
            sub.notification.endpoint.notifier_info = Some(
                json!([{"key":"MQTT-Version","value":version},{"key":"MQTT-QoS","value":qos.to_string()}]),
            );
            sub.notification.endpoint.accept = Some(
                if qos == 2 {
                    "application/ld+json"
                } else {
                    "application/json"
                }
                .into(),
            );
            sub.validate().unwrap();
            NotificationDispatcher::send(&sub, &notification, &client)
                .await
                .unwrap();
            let message = tokio::time::timeout(Duration::from_secs(3), received.recv())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(message["body"]["id"], notification["id"]);
            assert_eq!(message["metadata"]["X-Trace"], "mqtt-test");
            assert_eq!(
                message["metadata"]["Content-Type"],
                sub.notification.endpoint.accept.as_deref().unwrap()
            );
            if qos == 2 {
                assert!(message["body"]["@context"].is_string());
            } else {
                assert!(message["metadata"]["Link"]
                    .as_str()
                    .unwrap()
                    .contains("json-ld#context"));
            }
        }
    }
    // Secure transport uses additional private trust roots, never insecure TLS.
    if let Ok(ca) = std::env::var("ATHENA_TEST_MQTT_CA") {
        let tls_port = std::env::var("ATHENA_TEST_MQTTS_PORT").unwrap_or_else(|_| "18885".into());
        let trusted = MqttClient::from_ca_file(Some(&ca)).unwrap();
        sub.notification.endpoint.uri = format!("mqtts://localhost:{tls_port}/{topic}/secure");
        assert!(
            NotificationDispatcher::send(&sub, &notification, &client)
                .await
                .is_err(),
            "private CA is not trusted by default"
        );
        trusted.send(&sub, &notification, &client).await.unwrap();
        let msg = tokio::time::timeout(Duration::from_secs(3), received.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(msg["body"]["id"], notification["id"]);
        // Certificate deliberately has only DNS:localhost, not an IP SAN.
        sub.notification.endpoint.uri = format!("mqtts://127.0.0.1:{tls_port}/{topic}/secure");
        assert!(
            trusted.send(&sub, &notification, &client).await.is_err(),
            "hostname mismatch must fail even with a trusted CA"
        );
    }
    sub.notification.endpoint.uri = format!("mqtt://127.0.0.1:{port}/{topic}/durable");
    let blocked = OutboundClient::new(OutboundPolicy::default()).unwrap();
    assert!(NotificationDispatcher::send(&sub, &notification, &blocked)
        .await
        .is_err());
    let db = std::env::var("ATHENA_TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres@127.0.0.1:55432/athena_test".into());
    assert!(db.ends_with("/athena_test"));
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(6)
        .connect(&db)
        .await
        .unwrap();
    athena_storage::run_migrations(&pool).await.unwrap();
    let repo: Arc<dyn SubscriptionRepository> = Arc::new(PgSubscriptionStore::new(pool.clone()));
    let entity_store = PgEntityStore::new(pool.clone());
    let entity_id = format!("urn:ngsi-ld:Mqtt:{}", uuid::Uuid::new_v4());
    sub.id = format!("{entity_id}:subscription");
    sub.entities[0].id = Some(entity_id.clone());
    repo.create_subscription(&sub).await.unwrap();
    entity_store
        .create_entity(
            &Entity::from_json(
                json!({"id":entity_id,"type":"Sensor","reading":{"type":"Property","value":1}}),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    for _ in 0..100 {
        if materialize_events(&pool, &repo).await.unwrap() == 0 {
            break;
        }
    }
    let row = sqlx::query("SELECT id,notification FROM notification_jobs WHERE subscription_id=$1")
        .bind(&sub.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    let job: uuid::Uuid = row.get("id");
    let original: Value = row.get("notification");
    // Blocked transport must remain pending, with the exact same persisted body.
    for _ in 0..100 {
        deliver_one(&pool, &repo, &blocked).await.unwrap();
        let attempts: i32 =
            sqlx::query_scalar("SELECT attempts FROM notification_jobs WHERE id=$1")
                .bind(job)
                .fetch_one(&pool)
                .await
                .unwrap();
        if attempts > 0 {
            break;
        }
    }
    let row = sqlx::query("SELECT status,notification,attempts FROM notification_jobs WHERE id=$1")
        .bind(job)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(row.get::<String, _>("status"), "pending");
    assert_eq!(row.get::<Value, _>("notification"), original);
    assert_eq!(row.get::<i32, _>("attempts"), 1);
    sqlx::query("UPDATE notification_jobs SET available_at=clock_timestamp() WHERE id=$1")
        .bind(job)
        .execute(&pool)
        .await
        .unwrap();
    for _ in 0..100 {
        deliver_one(&pool, &repo, &client).await.unwrap();
        let status: String = sqlx::query_scalar("SELECT status FROM notification_jobs WHERE id=$1")
            .bind(job)
            .fetch_one(&pool)
            .await
            .unwrap();
        if status == "delivered" {
            break;
        }
    }
    let message = tokio::time::timeout(Duration::from_secs(3), received.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(message["body"]["id"], original["id"]);
    assert_eq!(
        repo.get_subscription_by_id(&sub.id)
            .await
            .unwrap()
            .unwrap()
            .notification
            .times_sent,
        Some(1)
    );
    repo.delete_subscription(&sub.id).await.unwrap();
    // HTTP subscription -> background persistent scheduler -> durable MQTT delivery.
    let engine = Arc::new(
        athena_subscription::SubscriptionEngine::with_config(
            repo.clone(),
            pool.clone(),
            athena_subscription::EngineConfig {
                poll_interval_ms: 25,
                worker_threads: 2,
                ..Default::default()
            },
            OutboundPolicy {
                allow_private: true,
            },
        )
        .unwrap(),
    );
    let state = athena_api::AppState::new(
        pool.clone(),
        Arc::new(PgEntityStore::new(pool.clone())),
        repo.clone(),
        Arc::new(athena_storage::PgTemporalStore::new(pool.clone())),
        Arc::new(athena_storage::PgCsourceStore::new(pool.clone())),
        engine.clone(),
        Arc::new(athena_jsonld::ContextResolver::default()),
    )
    .configure(
        Default::default(),
        OutboundPolicy {
            allow_private: true,
        },
        Arc::new(athena_jsonld::Processor::default()),
    );
    let app = athena_api::create_router(state);
    let scheduled_id = format!("{entity_id}:periodic");
    let body = json!({"id":scheduled_id,"type":"Subscription","entities":[{"id":entity_id,"type":"Sensor"}],"timeInterval":0.1,"notification":{"endpoint":{"uri":sub.notification.endpoint.uri,"notifierInfo":[{"key":"MQTT-QoS","value":"1"}]}}});
    use tower::ServiceExt;
    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/ngsi-ld/v1/subscriptions")
                .header("Content-Type", "application/json")
                .body(axum::body::Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::CREATED);
    let first = tokio::time::timeout(Duration::from_secs(5), received.recv())
        .await
        .unwrap()
        .unwrap();
    let second = tokio::time::timeout(Duration::from_secs(5), received.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first["body"]["subscriptionId"], scheduled_id);
    assert_eq!(second["body"]["subscriptionId"], scheduled_id);
    assert_eq!(first["body"]["data"], second["body"]["data"]);
    assert_ne!(first["body"]["id"], second["body"]["id"]);
    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("PATCH")
                .uri(format!("/ngsi-ld/v1/subscriptions/{scheduled_id}"))
                .header("Content-Type", "application/json")
                .body(axum::body::Body::from(
                    json!({"watchedAttributes":["reading"]}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
    repo.delete_subscription(&scheduled_id).await.unwrap();
    assert!(engine.shutdown(Duration::from_secs(5)).await);
    entity_store.delete_entity(&entity_id).await.unwrap();
    reader.abort();
    pool.close().await;
}
