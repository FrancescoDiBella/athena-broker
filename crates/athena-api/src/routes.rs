use axum::{
    extract::DefaultBodyLimit,
    middleware::{from_fn, from_fn_with_state},
    routing::{delete, get, post},
    Router,
};
use tower_http::cors::{Any, CorsLayer};

use crate::handlers::{attrs, batch, csource, entities, health, subscriptions, temporal, ui};
use crate::middleware::ngsi_ld_headers_middleware;
use crate::security::security_headers_middleware;
use crate::state::AppState;

pub fn create_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        // Web Dashboard & Explorer
        .route("/", get(ui::serve_ui))
        .route("/ui", get(ui::serve_ui))
        // Health & Metrics
        .route("/health", get(health::health_check))
        .route("/ready", get(health::readiness))
        .route("/metrics", get(health::metrics))
        // Entities CRUD & Query (with Federation)
        .route(
            "/ngsi-ld/v1/entities",
            post(entities::create_entity).get(entities::query_entities),
        )
        .route(
            "/ngsi-ld/v1/entities/:entity_id",
            get(entities::get_entity_by_id)
                .patch(entities::update_entity)
                .delete(entities::delete_entity),
        )
        // Entity Attributes
        .route(
            "/ngsi-ld/v1/entities/:entity_id/attrs",
            post(attrs::append_attrs).patch(attrs::update_attrs),
        )
        .route(
            "/ngsi-ld/v1/entities/:entity_id/attrs/:attr_id",
            delete(attrs::delete_attr),
        )
        // Batch Operations
        .route(
            "/ngsi-ld/v1/entityOperations/create",
            post(batch::batch_create),
        )
        .route(
            "/ngsi-ld/v1/entityOperations/upsert",
            post(batch::batch_upsert),
        )
        .route(
            "/ngsi-ld/v1/entityOperations/update",
            post(batch::batch_update),
        )
        .route(
            "/ngsi-ld/v1/entityOperations/delete",
            post(batch::batch_delete),
        )
        // Subscriptions
        .route(
            "/ngsi-ld/v1/subscriptions",
            post(subscriptions::create_subscription).get(subscriptions::list_subscriptions),
        )
        .route(
            "/ngsi-ld/v1/subscriptions/:subscription_id",
            get(subscriptions::get_subscription)
                .patch(subscriptions::update_subscription)
                .delete(subscriptions::delete_subscription),
        )
        // Context Source Registrations (Federated Distribution)
        .route(
            "/ngsi-ld/v1/csourceRegistrations",
            post(csource::create_csource_registration).get(csource::list_csource_registrations),
        )
        .route(
            "/ngsi-ld/v1/csourceRegistrations/:registration_id",
            get(csource::get_csource_registration)
                .patch(csource::update_csource_registration)
                .delete(csource::delete_csource_registration),
        )
        // Temporal Evolution & Aggregations
        .route(
            "/ngsi-ld/v1/temporal/entities",
            post(temporal::create_temporal_entity).get(temporal::query_temporal_entities),
        )
        .route(
            "/ngsi-ld/v1/temporal/entities/:entity_id",
            get(temporal::query_temporal_entity_by_id).delete(temporal::delete_temporal_entity),
        )
        .route(
            "/ngsi-ld/v1/temporal/entities/:entity_id/attrs/:attr_id",
            delete(temporal::delete_temporal_attribute),
        )
        .route(
            "/ngsi-ld/v1/temporal/entities/:entity_id/attrs/:attr_id/:instance_id",
            delete(temporal::delete_temporal_instance).patch(temporal::update_temporal_instance),
        )
        // Middleware layers
        .layer(DefaultBodyLimit::max(state.limits.max_body_bytes))
        .layer(from_fn(security_headers_middleware)) // Hardened security headers
        .layer(from_fn_with_state(
            state.clone(),
            ngsi_ld_headers_middleware,
        )) // NGSI-LD Link context headers
        .layer(from_fn_with_state(
            state.clone(),
            crate::middleware::request_metrics,
        ))
        .layer(cors)
        .with_state(state)
}
