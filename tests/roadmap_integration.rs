use athena_model::{CsourceRegistration, Entity, EntityInfo, RegistrationInfo};
use athena_storage::{
    CsourceRepository, EntityRepository, PgCsourceStore, PgEntityStore, PgSubscriptionStore,
    PgTemporalStore, SubscriptionRepository,
};
use axum::{
    body::{to_bytes, Body},
    extract::Query,
    http::{HeaderMap, Request, StatusCode},
    routing::get,
    Json, Router,
};
use serde_json::{json, Value};
use std::{collections::HashMap, sync::Arc};
use tower::ServiceExt;

fn app(pool: &sqlx::PgPool) -> Router {
    let subscriptions: Arc<dyn SubscriptionRepository> =
        Arc::new(PgSubscriptionStore::new(pool.clone()));
    let engine = Arc::new(athena_subscription::SubscriptionEngine::new(
        subscriptions.clone(),
        pool.clone(),
        1,
    ));
    let mut state = athena_api::AppState::new(
        pool.clone(),
        Arc::new(PgEntityStore::new(pool.clone())),
        subscriptions,
        Arc::new(PgTemporalStore::new(pool.clone())),
        Arc::new(PgCsourceStore::new(pool.clone())),
        engine,
        Arc::new(athena_jsonld::ContextResolver::default()),
    );
    state.outbound_policy = athena_http::OutboundPolicy {
        allow_private: true,
    };
    state.federation_service = Arc::new(athena_api::federation::FederationService::with_policy(
        state.outbound_policy,
        state.processor.clone(),
    ));
    athena_api::create_router(state)
}
async fn request(
    app: &Router,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> (StatusCode, HeaderMap, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("content-type", "application/json")
                .body(
                    body.map(|v| Body::from(v.to_string()))
                        .unwrap_or_else(Body::empty),
                )
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), 32 * 1024 * 1024)
        .await
        .unwrap();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, headers, value)
}
fn uri(path: &str, pairs: &[(&str, &str)]) -> String {
    let query = pairs
        .iter()
        .fold(String::new(), |mut result, (key, value)| {
            if !result.is_empty() {
                result.push('&');
            }
            result.push_str(&format!("{key}={}", percent(value)));
            result
        });
    format!("{path}?{query}")
}
fn percent(value: &str) -> String {
    value
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

#[tokio::test]
#[ignore = "isolated athena_test PostGIS database and local HTTP sources"]
async fn roadmap_protocol_regressions() {
    let url = std::env::var("ATHENA_TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres@127.0.0.1:55432/athena_test".into());
    assert!(url.ends_with("/athena_test"));
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(8)
        .connect(&url)
        .await
        .unwrap();
    athena_storage::run_migrations(&pool).await.unwrap();
    let app = app(&pool);
    let prefix = format!("urn:ngsi-ld:Roadmap:{}", uuid::Uuid::new_v4());
    mutations(&app, &pool, &prefix).await;
    historical_geo(&app, &pool, &prefix).await;
    spatial_edges(&app, &prefix).await;
    query_matcher_parity(&pool, &prefix).await;
    federation(&app, &pool, &prefix).await;
    pool.close().await;
}

async fn mutations(app: &Router, pool: &sqlx::PgPool, prefix: &str) {
    let id = format!("{prefix}:mutations");
    let path = format!("/ngsi-ld/v1/entities/{id}");
    let (status,_,body)=request(app,"POST","/ngsi-ld/v1/entities",Some(json!({"id":id,"type":"RoadmapSensor","temperature":[{"type":"Property","value":10,"unitCode":"CEL"},{"type":"Property","value":30,"datasetId":"urn:dataset:other"}]}))).await;
    assert_eq!(status, 201, "{body}");
    let (status, _, body) = request(
        app,
        "PATCH",
        &format!("{path}/attrs/temperature"),
        Some(json!({"value":20})),
    )
    .await;
    assert_eq!(status, 204, "{body}");
    let (_, _, body) = request(app, "GET", &path, None).await;
    let values = body["temperature"].as_array().unwrap();
    assert!(
        values
            .iter()
            .any(|v| v.get("datasetId").is_none() && v["value"] == 20 && v["unitCode"] == "CEL"),
        "{body}"
    );
    let (status, _, body) = request(
        app,
        "PATCH",
        &format!("{path}/attrs/temperature"),
        Some(json!({"unitCode":"urn:ngsi-ld:null"})),
    )
    .await;
    assert_eq!(status, 204, "{body}");
    let (status, _, _) = request(
        app,
        "PATCH",
        &format!("{path}/attrs/temperature"),
        Some(json!({"type":"Relationship","object":"urn:target"})),
    )
    .await;
    assert_eq!(status, 400);
    let (status,_,body)=request(app,"POST",&format!("{path}/attrs?options=noOverwrite"),Some(json!({"temperature":{"type":"Property","value":99},"humidity":{"type":"Property","value":50}}))).await;
    assert_eq!(status, 207, "{body}");
    assert_eq!(body["updated"].as_array().unwrap().len(), 1);
    assert_eq!(body["notUpdated"].as_array().unwrap().len(), 1);
    let (status, _, body) = request(
        app,
        "PATCH",
        &format!("{path}/attrs"),
        Some(json!({"temperature":{"type":"Property","value":"urn:ngsi-ld:null"}})),
    )
    .await;
    assert_eq!(status, 204, "{body}");
    let (_, _, body) = request(app, "GET", &path, None).await;
    assert_eq!(
        body["temperature"]["datasetId"], "urn:dataset:other",
        "{body}"
    );
    let (status, _, body) = request(
        app,
        "PUT",
        &format!("{path}/attrs/temperature"),
        Some(json!({"type":"Property","value":40,"datasetId":"urn:dataset:other"})),
    )
    .await;
    assert_eq!(status, 204, "{body}");
    let (status, _, body) = request(
        app,
        "PUT",
        &path,
        Some(json!({"type":"Replacement","pressure":{"type":"Property","value":1013}})),
    )
    .await;
    assert_eq!(status, 204, "{body}");
    let (_, _, body) = request(app, "GET", &path, None).await;
    assert!(body.get("temperature").is_none());
    assert!(body.get("humidity").is_none());
    assert_eq!(body["pressure"]["value"], 1013);
    for malformed in [
        json!([]),
        json!([id, 123]),
        json!([id, "not-an-absolute-uri"]),
    ] {
        assert_eq!(
            request(
                app,
                "POST",
                "/ngsi-ld/v1/entityOperations/delete",
                Some(malformed)
            )
            .await
            .0,
            400
        );
        assert_eq!(
            request(app, "GET", &path, None).await.0,
            200,
            "invalid delete batches make no writes"
        );
    }
    for operation in ["create", "upsert", "update"] {
        assert_eq!(
            request(
                app,
                "POST",
                &format!("/ngsi-ld/v1/entityOperations/{operation}"),
                Some(json!([]))
            )
            .await
            .0,
            400
        );
    }

    let repo = PgEntityStore::new(pool.clone());
    let a = json!({"pressure":{"value":1000}});
    let b = json!({"pressure":{"unitCode":"BAR"}});
    // Concurrent patches on one instance must preserve independently changed members.
    let (a, b) = tokio::join!(
        repo.mutate_attribute(
            &id,
            "https://uri.etsi.org/ngsi-ld/default-context/pressure",
            &a["pressure"],
            false
        ),
        repo.mutate_attribute(
            &id,
            "https://uri.etsi.org/ngsi-ld/default-context/pressure",
            &b["pressure"],
            false
        )
    );
    a.unwrap();
    b.unwrap();
    let saved = repo.get_entity_by_id(&id, None).await.unwrap().unwrap();
    let pressure = &saved.attributes["https://uri.etsi.org/ngsi-ld/default-context/pressure"];
    assert_eq!(pressure["value"], 1000);
    assert_eq!(pressure["unitCode"], "BAR");
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM entity_events WHERE entity_id=$1")
        .bind(&id)
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(count, 9, "only successful mutations produce durable events");
    let (status, _, body) = request(
        app,
        "POST",
        &format!("{path}/attrs"),
        Some(json!({"type":"AdditionalType","scope":"/site/a"})),
    )
    .await;
    assert_eq!(status, 204, "{body}");
    let (status, _, body) = request(
        app,
        "POST",
        &format!("{path}/attrs?options=noOverwrite"),
        Some(json!({"scope":["/site/a","/site/b"]})),
    )
    .await;
    assert_eq!(status, 204, "{body}");
    let (_, _, body) = request(app, "GET", &path, None).await;
    assert_eq!(body["scope"], json!(["/site/a", "/site/b"]));
    assert_eq!(body["type"].as_array().unwrap().len(), 2);
    let alias_id = format!("{prefix}:alias");
    assert_eq!(request(app,"POST","/ngsi-ld/v1/entities",Some(json!({"id":alias_id,"type":"AliasSensor","urn:example:temperature":{"type":"Property","value":1}}))).await.0,201);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/ngsi-ld/v1/entities/{alias_id}/attrs/t"))
                .header("content-type", "application/ld+json")
                .body(Body::from(
                    json!({"@context":{"t":"urn:example:temperature"},"value":7}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        204,
        "inline context expands attribute paths"
    );
    let (_, _, body) = request(
        app,
        "GET",
        &format!("/ngsi-ld/v1/entities/{alias_id}"),
        None,
    )
    .await;
    assert_eq!(body["urn:example:temperature"]["value"], 7);
}

async fn historical_geo(app: &Router, _pool: &sqlx::PgPool, prefix: &str) {
    for path in ["/ngsi-ld/v1/entities", "/ngsi-ld/v1/temporal/entities"] {
        for parameter in [("idPattern", "["), ("q", "temperature~='['")] {
            let (status, _, body) = request(app, "GET", &uri(path, &[parameter]), None).await;
            assert_eq!(status, 400, "invalid regex must not reach SQL: {body}");
        }
    }
    for option in ["local=false", "splitEntities=true"] {
        let (status, _, body) = request(
            app,
            "GET",
            &format!("/ngsi-ld/v1/temporal/entities?{option}"),
            None,
        )
        .await;
        assert_eq!(
            status, 501,
            "unsupported temporal federation must be explicit: {body}"
        );
        assert_eq!(body["status"], 501);
    }
    let id = format!("{prefix}:history");
    let other = format!("{prefix}:history-far");
    for (id, coordinates) in [(&id, json!([0, 0])), (&other, json!([20, 20]))] {
        let (status,_,body)=request(app,"POST","/ngsi-ld/v1/temporal/entities",Some(json!({"id":id,"type":"HistoricGeoSensor","temperature":[{"type":"Property","value":10,"observedAt":"2026-01-01T00:00:00Z"},{"type":"Property","value":20,"observedAt":"2026-01-02T00:00:00Z"}],"location":[{"type":"GeoProperty","value":{"type":"Point","coordinates":coordinates},"observedAt":"2026-01-01T00:00:00Z","datasetId":"urn:dataset:gps"},{"type":"GeoProperty","value":{"type":"Point","coordinates":[40,40]},"observedAt":"2026-01-02T00:00:00Z"}]}))).await;
        assert_eq!(status, 201, "{body}");
    }
    let url = uri(
        "/ngsi-ld/v1/temporal/entities",
        &[
            ("id", &format!("{id},{other}")),
            ("type", "HistoricGeoSensor,Missing"),
            ("timerel", "between"),
            ("timeAt", "2026-01-01T00:00:00Z"),
            ("endTimeAt", "2026-01-02T00:00:00Z"),
            ("georel", "near;maxDistance==1000"),
            ("geometry", "Point"),
            ("coordinates", "[0,0]"),
            ("q", "temperature>=10"),
            ("attrs", "temperature"),
            ("count", "true"),
            ("limit", "1"),
        ],
    );
    let (status, headers, body) = request(app, "GET", &url, None).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(headers["NGSILD-Results-Count"], "1");
    assert_eq!(body.as_array().unwrap().len(), 1);
    assert_eq!(body[0]["id"], id);
    assert!(body[0].get("location").is_none());
    assert_eq!(
        body[0]["temperature"].as_array().unwrap().len(),
        1,
        "upper bound is exclusive: {body}"
    );
    assert_eq!(body[0]["temperature"][0]["value"], 10);
    let (status, headers, body) =
        request(app, "GET", &url.replace("limit=1", "limit=0"), None).await;
    assert_eq!(status, 200);
    assert_eq!(headers["NGSILD-Results-Count"], "1");
    assert_eq!(body, json!([]));
    let (status, _, body) = request(app, "GET", &format!("{url}&offset=1"), None).await;
    assert_eq!(status, 200);
    assert_eq!(body, json!([]));
    let url = uri(
        "/ngsi-ld/v1/temporal/entities",
        &[
            ("id", &id),
            ("timerel", "after"),
            ("timeAt", "2026-01-02T00:00:00Z"),
            ("attrs", "temperature"),
        ],
    );
    let (status, _, body) = request(app, "GET", &url, None).await;
    assert_eq!(status, 200);
    assert_eq!(
        body[0]["temperature"][0]["value"], 20,
        "after includes its lower bound"
    );
    let (status, _, body) = request(
        app,
        "GET",
        &uri(
            "/ngsi-ld/v1/temporal/entities",
            &[
                ("id", &format!("{prefix}:unknown")),
                ("timerel", "after"),
                ("timeAt", "2026-01-01T00:00:00Z"),
            ],
        ),
        None,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body, json!([]), "missing collection IDs are an empty match");
    // Multiple GeoProperty datasets and unrelated non-spatial values must be safe.
    let current = format!("{prefix}:current-geo");
    let (status,_,body)=request(app,"POST","/ngsi-ld/v1/entities",Some(json!({"id":current,"type":"RoadmapGeo","position":[{"type":"GeoProperty","value":{"type":"Point","coordinates":[50,50]}},{"type":"GeoProperty","value":{"type":"Point","coordinates":[0,0]},"datasetId":"urn:gps"}],"unrelated":{"type":"Property","value":"not geometry"}}))).await;
    assert_eq!(status, 201, "{body}");
    for (property, expected) in [("position", 1), ("unrelated", 0), ("missing", 0)] {
        let url = uri(
            "/ngsi-ld/v1/entities",
            &[
                ("id", &current),
                ("local", "true"),
                ("geoproperty", property),
                ("georel", "equals"),
                ("geometry", "Point"),
                ("coordinates", "[0,0]"),
            ],
        );
        let (status, _, body) = request(app, "GET", &url, None).await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body.as_array().unwrap().len(), expected);
    }
    for relation in [
        "within;maxDistance==1",
        "near;maxDistance==NaN",
        "near;minDistance==2;maxDistance==1",
    ] {
        let url = uri(
            "/ngsi-ld/v1/entities",
            &[
                ("local", "true"),
                ("georel", relation),
                ("geometry", "Point"),
                ("coordinates", "[0,0]"),
            ],
        );
        assert_eq!(request(app, "GET", &url, None).await.0, 400);
    }
}

async fn query_matcher_parity(pool: &sqlx::PgPool, prefix: &str) {
    let repo = PgEntityStore::new(pool.clone());
    let entity=Entity::from_json(json!({"id":format!("{prefix}:patterns"),"type":"PatternSensor","label":[{"type":"Property","value":"alpha"},{"type":"Property","value":"beta","datasetId":"urn:second"}],"number":{"type":"Property","value":1}})).unwrap();
    repo.create_entity(&entity).await.unwrap();
    for (query, expected) in [
        ("label~='^alpha'", true),
        ("label!~='^alpha'", true),
        ("missing!~='.*'", false),
        ("number!~='.*'", false),
    ] {
        let sub:athena_model::Subscription=serde_json::from_value(json!({"id":format!("{prefix}:pattern-sub"),"type":"Subscription","entities":[{"type":"PatternSensor"}],"q":query,"notification":{"endpoint":{"uri":"https://example.org/notify"}}})).unwrap();
        let matched = athena_subscription::SubscriptionMatcher::matches(&sub, &entity, &[]);
        assert_eq!(matched, expected, "{query}");
        let rows = repo
            .query_entities(&athena_storage::EntityQueryParams {
                id: Some(entity.id.clone()),
                q: Some(athena_query::Parser::parse_str(query).unwrap()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(
            !rows.is_empty(),
            matched,
            "SQL/subscription parity for {query}"
        );
    }
}

async fn spatial_edges(app: &Router, prefix: &str) {
    let polygon = format!("{prefix}:polygon");
    let (status,_,body)=request(app,"POST","/ngsi-ld/v1/entities",Some(json!({"id":polygon,"type":"SpatialEdge","location":{"type":"GeoProperty","value":{"type":"Polygon","coordinates":[[[0,0],[2,0],[2,2],[0,2],[0,0]],[[0.5,0.5],[0.5,1.5],[1.5,1.5],[1.5,0.5],[0.5,0.5]]]}}}))).await;
    assert_eq!(status, 201, "{body}");
    for (relation, geometry, coordinates, expected) in [
        ("contains", "Point", "[1,1]", 0),
        ("contains", "Point", "[0.25,0.25]", 1),
        ("intersects", "Point", "[0,0]", 1),
        ("disjoint", "Point", "[1,1]", 1),
        (
            "within",
            "Polygon",
            "[[[-1,-1],[3,-1],[3,3],[-1,3],[-1,-1]]]",
            1,
        ),
        (
            "overlaps",
            "Polygon",
            "[[[1,1],[3,1],[3,3],[1,3],[1,1]]]",
            1,
        ),
    ] {
        let url = uri(
            "/ngsi-ld/v1/entities",
            &[
                ("id", &polygon),
                ("local", "true"),
                ("georel", relation),
                ("geometry", geometry),
                ("coordinates", coordinates),
            ],
        );
        let (status, _, body) = request(app, "GET", &url, None).await;
        assert_eq!(status, 200, "{relation}: {body}");
        assert_eq!(body.as_array().unwrap().len(), expected, "{relation}");
    }
    let id = format!("{prefix}:antimeridian");
    assert_eq!(request(app,"POST","/ngsi-ld/v1/entities",Some(json!({"id":id,"type":"SpatialEdge","location":{"type":"GeoProperty","value":{"type":"Point","coordinates":[179.9,0]}}}))).await.0,201);
    for (relation, expected) in [
        ("near;maxDistance==30000", 1),
        ("near;minDistance==30000", 0),
        ("near;maxDistance==10000", 0),
    ] {
        let url = uri(
            "/ngsi-ld/v1/entities",
            &[
                ("id", &id),
                ("local", "true"),
                ("georel", relation),
                ("geometry", "Point"),
                ("coordinates", "[-179.9,0]"),
            ],
        );
        let (status, _, body) = request(app, "GET", &url, None).await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(
            body.as_array().unwrap().len(),
            expected,
            "distance must cross the antimeridian in meters"
        );
    }
    assert_eq!(
        request(app, "DELETE", &format!("/ngsi-ld/v1/entities/{id}"), None)
            .await
            .0,
        204
    );
    let url = uri(
        "/ngsi-ld/v1/temporal/entities",
        &[
            ("id", &id),
            ("timeproperty", "deletedAt"),
            ("timerel", "after"),
            ("timeAt", "2020-01-01T00:00:00Z"),
            ("georel", "near;maxDistance==30000"),
            ("geometry", "Point"),
            ("coordinates", "[-179.9,0]"),
        ],
    );
    let (status, _, body) = request(app, "GET", &url, None).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body[0]["id"], id, "deletion history retains spatial data");
    assert_eq!(body[0]["location"].as_array().unwrap().len(), 1);
}

async fn federation(app: &Router, pool: &sqlx::PgPool, prefix: &str) {
    let kind = format!("{prefix}:FederationType");
    let shared = format!("{prefix}:shared");
    let remote = format!("{prefix}:remote");
    let repo = PgEntityStore::new(pool.clone());
    repo.create_entity(
        &Entity::from_json(
            json!({"id":shared,"type":kind,"temperature":{"type":"Property","value":1}}),
        )
        .unwrap(),
    )
    .await
    .unwrap();
    let data = Arc::new(vec![
        json!({"id":remote,"type":kind,"temperature":{"type":"Property","value":20}}),
        json!({"id":shared,"type":kind,"temperature":{"type":"Property","value":2,"datasetId":"urn:remote"},"humidity":{"type":"Property","value":55}}),
    ]);
    let calls = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let seen = calls.clone();
    let failing = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let fail = failing.clone();
    let source = Router::new().route(
        "/ngsi-ld/v1/entities",
        get(move |Query(query): Query<HashMap<String, String>>| {
            let data = data.clone();
            let fail = fail.clone();
            let seen = seen.clone();
            async move {
                seen.lock().await.push(query.clone());
                let mode = fail.load(std::sync::atomic::Ordering::Relaxed);
                if mode == 1 {
                    return (
                        StatusCode::SERVICE_UNAVAILABLE,
                        [("NGSILD-Results-Count", "0")],
                        Json(Vec::<Value>::new()),
                    );
                }
                let offset = if mode == 2 {
                    0
                } else {
                    query.get("offset").unwrap().parse::<usize>().unwrap()
                };
                // Deliberately cap pages at one, despite the requested page size.
                let page = data
                    .iter()
                    .skip(offset)
                    .take(1)
                    .cloned()
                    .collect::<Vec<_>>();
                if mode == 3 {
                    return (
                        StatusCode::OK,
                        [("NGSILD-Results-Count", "2")],
                        Json(vec![json!({"unexpected":true})]),
                    );
                }
                (
                    if mode == 5 {
                        StatusCode::PARTIAL_CONTENT
                    } else {
                        StatusCode::OK
                    },
                    [(
                        "NGSILD-Results-Count",
                        if mode == 4 {
                            "0"
                        } else if mode == 6 && offset > 0 {
                            "3"
                        } else {
                            "2"
                        },
                    )],
                    Json(page),
                )
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, source).await.unwrap() });
    let store = PgCsourceStore::new(pool.clone());
    let registration = CsourceRegistration::new(
        format!("{prefix}:source"),
        endpoint,
        vec![RegistrationInfo {
            entities: Some(vec![EntityInfo {
                id: None,
                id_pattern: None,
                r#type: kind.clone(),
            }]),
            property_names: Some(vec![
                "https://uri.etsi.org/ngsi-ld/default-context/humidity".into(),
            ]),
            relationship_names: None,
        }],
    );
    store.create_csource(&registration).await.unwrap();
    // This matching registration must be discovered past the old first-100 limit.
    for n in 0..101 {
        store
            .create_csource(&CsourceRegistration::new(
                format!("{prefix}:unrelated:{n}"),
                "http://127.0.0.1:1",
                vec![RegistrationInfo {
                    entities: Some(vec![EntityInfo {
                        id: None,
                        id_pattern: None,
                        r#type: format!("{prefix}:Unrelated"),
                    }]),
                    property_names: None,
                    relationship_names: None,
                }],
            ))
            .await
            .unwrap();
    }
    let url = uri(
        "/ngsi-ld/v1/entities",
        &[
            ("type", &kind),
            ("count", "true"),
            ("limit", "1"),
            ("offset", "1"),
        ],
    );
    let (status, headers, body) = request(app, "GET", &url, None).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(headers["NGSILD-Results-Count"], "2");
    let links = headers
        .get_all("link")
        .iter()
        .map(|v| v.to_str().unwrap())
        .collect::<Vec<_>>();
    assert!(links.iter().any(|link| link.contains("rel=\"prev\"")));
    assert!(links
        .iter()
        .any(|link| link.contains("http://www.w3.org/ns/json-ld#context")));
    assert_eq!(body.as_array().unwrap().len(), 1, "global page size");
    let (status, _, body) = request(
        app,
        "GET",
        &url.replace("limit=1", "limit=10")
            .replace("offset=1", "offset=0"),
        None,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let merged = body
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["id"] == shared)
        .unwrap();
    assert_eq!(
        merged["temperature"].as_array().unwrap().len(),
        2,
        "merge preserves independent datasets: {body}"
    );
    for call in calls.lock().await.iter() {
        assert_eq!(call["local"], "true");
        assert_eq!(call["limit"], "1000");
        assert_eq!(call["options"], "sysAttrs");
    }

    let split = uri(
        "/ngsi-ld/v1/entities",
        &[
            ("type", &kind),
            ("splitEntities", "true"),
            ("q", "temperature==1;humidity==55"),
            ("attrs", "temperature"),
            ("count", "true"),
        ],
    );
    let (status, headers, body) = request(app, "GET", &split, None).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(headers["NGSILD-Results-Count"], "1");
    assert_eq!(
        body[0]["id"], shared,
        "cross-source predicates apply after merge: {body}"
    );
    assert!(
        body[0].get("humidity").is_none(),
        "projection follows filtering"
    );
    let seen = calls.lock().await;
    assert!(!seen.last().unwrap().contains_key("q"));
    assert!(!seen.last().unwrap().contains_key("attrs"));
    drop(seen);
    for mode in [2, 3, 4, 5, 6] {
        failing.store(mode, std::sync::atomic::Ordering::Relaxed);
        let (status, _, body) = request(app, "GET", &url, None).await;
        assert_eq!(status, 502, "reject invalid upstream mode {mode}: {body}");
        assert_eq!(body["status"], 502);
    }
    failing.store(1, std::sync::atomic::Ordering::Relaxed);
    let (status, _, body) = request(app, "GET", &url, None).await;
    assert_eq!(
        status, 502,
        "failed sources must not produce incomplete success: {body}"
    );
    let (status, headers, body) = request(app, "GET", &format!("{url}&local=true"), None).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(headers["NGSILD-Results-Count"], "1");
    server.abort();
    let _ = server.await;
    sqlx::query("DELETE FROM csource_registrations WHERE id LIKE $1")
        .bind(format!("{prefix}%"))
        .execute(pool)
        .await
        .unwrap();
}
