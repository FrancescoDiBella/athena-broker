use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};
use serde::Deserialize;
use serde_json::Value;

use crate::state::AppState;
use athena_model::{ProblemDetails, Subscription};
use athena_storage::StorageError;

#[derive(Debug, Deserialize)]
pub struct ListSubscriptionOptions {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

pub async fn create_subscription(
    State(state): State<AppState>,
    Extension(ctx): Extension<crate::middleware::RequestContext>,
    Json(payload): Json<Value>,
) -> Response {
    let mut payload = payload;
    if let Err(error) = prepare_input(&state, &ctx, &mut payload, true).await {
        return bad(error);
    }
    let mut sub: Subscription = match serde_json::from_value(payload) {
        Ok(sub) => sub,
        Err(error) => return bad(format!("Invalid subscription: {error}")),
    };
    if sub.id.is_empty() {
        sub.id = format!("urn:ngsi-ld:Subscription:{}", uuid::Uuid::new_v4());
    }
    if let Err(error) = sub.validate() {
        return bad(error);
    }

    let id = sub.id.clone();
    match state.subscription_repo.create_subscription(&sub).await {
        Ok(_) => {
            let mut headers = HeaderMap::new();
            if let Ok(loc) = HeaderValue::from_str(&format!("/ngsi-ld/v1/subscriptions/{id}")) {
                headers.insert(header::LOCATION, loc);
            }
            (StatusCode::CREATED, headers, ()).into_response()
        }
        Err(err) if err.is_unique_violation() => (
            StatusCode::CONFLICT,
            Json(ProblemDetails::already_exists(format!(
                "Subscription '{id}' already exists"
            ))),
        )
            .into_response(),
        Err(StorageError::Problem(error)) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ProblemDetails::internal_error(e.to_string())),
        )
            .into_response(),
    }
}

pub async fn get_subscription(
    State(state): State<AppState>,
    Path(subscription_id): Path<String>,
) -> Response {
    match state
        .subscription_repo
        .get_subscription_by_id(&subscription_id)
        .await
    {
        Ok(Some(sub)) => (StatusCode::OK, Json(sub)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ProblemDetails::not_found(format!(
                "Subscription '{subscription_id}' not found"
            ))),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ProblemDetails::internal_error(e.to_string())),
        )
            .into_response(),
    }
}

pub async fn list_subscriptions(
    State(state): State<AppState>,
    Query(opts): Query<ListSubscriptionOptions>,
) -> Response {
    match state
        .subscription_repo
        .list_subscriptions(opts.limit, opts.offset)
        .await
    {
        Ok(subs) => (StatusCode::OK, Json(subs)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ProblemDetails::internal_error(e.to_string())),
        )
            .into_response(),
    }
}

pub async fn update_subscription(
    State(state): State<AppState>,
    Path(subscription_id): Path<String>,
    Extension(ctx): Extension<crate::middleware::RequestContext>,
    Json(mut payload): Json<Value>,
) -> Response {
    if !athena_model::attributes::valid_uri(&subscription_id) {
        return bad("Invalid subscription id");
    }
    if let Err(error) = prepare_input(&state, &ctx, &mut payload, false).await {
        return bad(error);
    }
    match state
        .subscription_repo
        .update_subscription(&subscription_id, &payload)
        .await
    {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(StorageError::SubscriptionNotFound(_)) => (
            StatusCode::NOT_FOUND,
            Json(ProblemDetails::not_found(format!(
                "Subscription '{subscription_id}' not found"
            ))),
        )
            .into_response(),
        Err(StorageError::Problem(error)) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ProblemDetails::internal_error(error.to_string())),
        )
            .into_response(),
    }
}

fn bad(message: impl Into<String>) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(ProblemDetails::bad_request_data(message)),
    )
        .into_response()
}
async fn prepare_input(
    state: &AppState,
    ctx: &crate::middleware::RequestContext,
    payload: &mut Value,
    create: bool,
) -> Result<(), String> {
    athena_model::subscription::sanitize_subscription_input(payload, create)?;
    let context = payload.get("@context").cloned().unwrap_or_else(|| {
        serde_json::json!(ctx
            .context_uri
            .as_deref()
            .unwrap_or(athena_model::ETSI_CORE_CONTEXT_URL))
    });
    if let Some(targets) = payload.get_mut("entities").and_then(Value::as_array_mut) {
        for target in targets {
            let object = target.as_object().ok_or("Invalid entity selector")?;
            if object
                .keys()
                .any(|k| !matches!(k.as_str(), "id" | "idPattern" | "type"))
            {
                return Err("Unsupported entity selector field".into());
            }
            let kind = target
                .get("type")
                .and_then(Value::as_str)
                .ok_or("Entity selector requires type")?;
            target["type"] =
                serde_json::json!(state.processor.expand_term(kind, context.clone()).await?);
        }
    }
    for pointer in ["/watchedAttributes", "/notification/attributes"] {
        if let Some(attributes) = payload.pointer_mut(pointer) {
            if attributes == "urn:ngsi-ld:null" {
                continue;
            }
            for attr in attributes
                .as_array_mut()
                .ok_or("Attribute list must be an array")?
            {
                let term = attr.as_str().ok_or("Attribute names must be strings")?;
                *attr =
                    serde_json::json!(state.processor.expand_term(term, context.clone()).await?);
            }
        }
    }
    if let Some(query) = payload.get("q").filter(|v| *v != "urn:ngsi-ld:null") {
        let query = query.as_str().ok_or("q must be a string")?;
        let expanded = crate::middleware::expand_q(state, &context, query).await?;
        athena_query::Parser::parse_str(&expanded).map_err(|e| e.to_string())?;
        payload["q"] = serde_json::json!(expanded);
    }
    if let Some(geo) = payload.get_mut("geoQ").filter(|v| *v != "urn:ngsi-ld:null") {
        if let Some(property) = geo.get("geoproperty").and_then(Value::as_str) {
            geo["geoproperty"] = serde_json::json!(
                state
                    .processor
                    .expand_term(property, context.clone())
                    .await?
            );
        }
        athena_subscription::engine::parse_subscription_geo(geo)?;
    }
    if let Some(notification) = payload.get("notification") {
        let params: athena_model::NotificationParams =
            serde_json::from_value(notification.clone()).map_err(|e| e.to_string())?;
        athena_subscription::dispatcher::validate_endpoint(
            state.outbound_policy,
            &params.endpoint,
        )?;
        athena_subscription::dispatcher::receiver_headers(params.endpoint.receiver_info.as_ref())?;
    }
    if create || payload.get("@context").is_some() || ctx.context_uri.is_some() {
        payload["@context"] = context;
    }
    Ok(())
}

pub async fn delete_subscription(
    State(state): State<AppState>,
    Path(subscription_id): Path<String>,
) -> Response {
    match state
        .subscription_repo
        .delete_subscription(&subscription_id)
        .await
    {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(StorageError::SubscriptionNotFound(_)) => (
            StatusCode::NOT_FOUND,
            Json(ProblemDetails::not_found(format!(
                "Subscription '{subscription_id}' not found"
            ))),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ProblemDetails::internal_error(e.to_string())),
        )
            .into_response(),
    }
}
