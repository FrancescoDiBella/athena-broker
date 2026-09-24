use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension, Json,
};
use serde::Deserialize;
use serde_json::Value;

use crate::state::AppState;
use athena_model::ProblemDetails;
use athena_storage::StorageError;

#[derive(Debug, Deserialize)]
pub struct AppendOptions {
    pub options: Option<String>,
}

pub async fn append_attrs(
    State(state): State<AppState>,
    Path(entity_id): Path<String>,
    Query(opts): Query<AppendOptions>,
    Json(payload): Json<Value>,
) -> Response {
    if !payload.is_object() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ProblemDetails::bad_request_data(
                "Payload must be a JSON object of attributes",
            )),
        )
            .into_response();
    }

    let overwrite = !opts
        .options
        .as_deref()
        .map(|o| o.contains("noOverwrite"))
        .unwrap_or(false);

    let mutated_attrs: Vec<String> = payload
        .as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();

    match state
        .entity_repo
        .append_entity_attrs(&entity_id, &payload, overwrite)
        .await
    {
        Ok(_) => {
            if let Ok(Some(entity)) = state.entity_repo.get_entity_by_id(&entity_id, None).await {
                state
                    .subscription_engine
                    .notify_mutation(entity, mutated_attrs)
                    .await;
            }
            StatusCode::NO_CONTENT.into_response()
        }
        Err(StorageError::EntityNotFound(_)) => (
            StatusCode::NOT_FOUND,
            Json(ProblemDetails::not_found(format!(
                "Entity '{entity_id}' not found"
            ))),
        )
            .into_response(),
        Err(athena_storage::StorageError::Problem(problem)) => {
            (StatusCode::BAD_REQUEST, Json(problem)).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ProblemDetails::internal_error(e.to_string())),
        )
            .into_response(),
    }
}

pub async fn update_attrs(
    State(state): State<AppState>,
    Path(entity_id): Path<String>,
    Json(payload): Json<Value>,
) -> Response {
    if !payload.is_object() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ProblemDetails::bad_request_data(
                "Payload must be a JSON object of attributes",
            )),
        )
            .into_response();
    }

    let mutated_attrs: Vec<String> = payload
        .as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();

    match state
        .entity_repo
        .update_entity_attrs(&entity_id, &payload)
        .await
    {
        Ok(_) => {
            if let Ok(Some(entity)) = state.entity_repo.get_entity_by_id(&entity_id, None).await {
                state
                    .subscription_engine
                    .notify_mutation(entity, mutated_attrs)
                    .await;
            }
            StatusCode::NO_CONTENT.into_response()
        }
        Err(StorageError::EntityNotFound(_)) => (
            StatusCode::NOT_FOUND,
            Json(ProblemDetails::not_found(format!(
                "Entity '{entity_id}' not found"
            ))),
        )
            .into_response(),
        Err(athena_storage::StorageError::Problem(problem)) => {
            (StatusCode::BAD_REQUEST, Json(problem)).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ProblemDetails::internal_error(e.to_string())),
        )
            .into_response(),
    }
}

pub async fn delete_attr(
    State(state): State<AppState>,
    Path((entity_id, attr_id)): Path<(String, String)>,
    Extension(ctx): Extension<crate::middleware::RequestContext>,
    Query(options): Query<crate::handlers::temporal::DeleteTemporalOptions>,
) -> Response {
    if options.delete_all && options.dataset.is_some() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ProblemDetails::bad_request_data(
                "datasetId and deleteAll are mutually exclusive",
            )),
        )
            .into_response();
    }
    let attr_id = match state
        .processor
        .expand_term(
            &attr_id,
            serde_json::json!(ctx
                .context_uri
                .unwrap_or_else(|| athena_model::ETSI_CORE_CONTEXT_URL.into())),
        )
        .await
    {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ProblemDetails::bad_request_data(e)),
            )
                .into_response()
        }
    };
    match state
        .entity_repo
        .delete_entity_attr_instance(
            &entity_id,
            &attr_id,
            options.dataset.as_deref(),
            options.delete_all,
        )
        .await
    {
        Ok(_) => {
            if let Ok(Some(entity)) = state.entity_repo.get_entity_by_id(&entity_id, None).await {
                state
                    .subscription_engine
                    .notify_mutation(entity, vec![attr_id])
                    .await;
            }
            StatusCode::NO_CONTENT.into_response()
        }
        Err(StorageError::EntityNotFound(_)) => (
            StatusCode::NOT_FOUND,
            Json(ProblemDetails::not_found(format!(
                "Attribute '{attr_id}' on '{entity_id}' not found"
            ))),
        )
            .into_response(),
        Err(athena_storage::StorageError::Problem(problem)) => {
            (StatusCode::BAD_REQUEST, Json(problem)).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ProblemDetails::internal_error(e.to_string())),
        )
            .into_response(),
    }
}
