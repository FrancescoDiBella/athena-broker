use crate::state::AppState;
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use sqlx::Row;

pub async fn health_check() -> Response {
    Json(json!({"status":"alive","version":env!("CARGO_PKG_VERSION")})).into_response()
}

pub async fn readiness(State(state): State<AppState>) -> Response {
    if !state.subscription_engine.is_healthy().await {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"status":"workers_unavailable"})),
        )
            .into_response();
    }
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        sqlx::query("SELECT 1 FROM _sqlx_migrations WHERE version=$1 AND success")
            .bind(athena_storage::SCHEMA_VERSION)
            .fetch_optional(&state.pool),
    )
    .await;
    match result {
        Ok(Ok(Some(_))) => Json(json!({"status":"ready"})).into_response(),
        _ => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"status":"unavailable"})),
        )
            .into_response(),
    }
}

pub async fn metrics(State(state): State<AppState>) -> Response {
    use std::sync::atomic::Ordering::Relaxed;
    let result=tokio::time::timeout(std::time::Duration::from_secs(2),sqlx::query("SELECT (SELECT count(*) FROM subscription_schedules sc JOIN subscriptions s ON s.id=sc.subscription_id WHERE s.status='active' AND (s.expires_at IS NULL OR s.expires_at>now()) AND sc.next_run_at<=now()) AS schedules_due, (SELECT count(*) FROM subscription_schedules WHERE last_error IS NOT NULL) AS schedules_failed, (SELECT count(*) FROM entity_events WHERE processed_at IS NOT NULL AND last_error IS NOT NULL) AS event_dead, (SELECT count(*) FROM entity_events WHERE processed_at IS NULL) AS events, (SELECT count(*) FROM notification_jobs WHERE status='pending') AS pending, (SELECT count(*) FROM notification_jobs WHERE status='dead') AS dead, (SELECT COALESCE(EXTRACT(EPOCH FROM now()-min(created_at)),0)::double precision FROM entity_events WHERE processed_at IS NULL) AS lag").fetch_one(&state.pool)).await;
    let mut output=format!("# TYPE athena_http_requests_total counter\nathena_http_requests_total {}\n# TYPE athena_http_server_errors_total counter\nathena_http_server_errors_total {}\n# TYPE athena_http_in_flight gauge\nathena_http_in_flight {}\n# TYPE athena_http_duration_seconds summary\nathena_http_duration_seconds_sum {}\nathena_http_duration_seconds_count {}\nathena_db_pool_connections {}\nathena_db_pool_idle {}\n",state.metrics.total.load(Relaxed),state.metrics.failures.load(Relaxed),state.metrics.in_flight.load(Relaxed),state.metrics.duration_micros.load(Relaxed) as f64/1_000_000.,state.metrics.total.load(Relaxed),state.pool.size(),state.pool.num_idle());
    output.push_str(&format!(
        "athena_notification_workers_up {}\n",
        u8::from(state.subscription_engine.is_healthy().await)
    ));
    match result {
        Ok(Ok(row)) => {
            for (column, name) in [
                ("events", "outbox_pending"),
                ("event_dead", "outbox_dead"),
                ("pending", "notifications_pending"),
                ("dead", "notifications_dead"),
                ("schedules_due", "schedules_due"),
                ("schedules_failed", "schedules_failed"),
            ] {
                if let Ok(value) = row.try_get::<i64, _>(column) {
                    output.push_str(&format!("athena_{name} {value}\n"));
                }
            }
            if let Ok(lag) = row.try_get::<f64, _>("lag") {
                output.push_str(&format!("athena_outbox_oldest_seconds {lag}\n"));
            }
            output.push_str("athena_db_up 1\n");
        }
        _ => output.push_str("athena_db_up 0\n"),
    }
    ([("content-type", "text/plain; version=0.0.4")], output).into_response()
}
