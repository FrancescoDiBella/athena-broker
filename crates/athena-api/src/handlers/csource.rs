use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};
use serde::Deserialize;
use serde_json::Value;

use crate::security::validate_endpoint;
use crate::state::AppState;
use athena_model::{CsourceRegistration, ProblemDetails};
use athena_storage::StorageError;

#[derive(Debug, Deserialize)]
pub struct ListCsourceOptions {
    pub r#type: Option<String>,
    pub id: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

pub async fn create_csource_registration(
    State(state): State<AppState>,
    Extension(ctx): Extension<crate::middleware::RequestContext>,
    Json(payload): Json<Value>,
) -> Response {
    let context = payload.get("@context").cloned().unwrap_or_else(|| {
        serde_json::json!(ctx
            .context_uri
            .unwrap_or_else(|| athena_model::ETSI_CORE_CONTEXT_URL.into()))
    });
    let mut csource: CsourceRegistration = match serde_json::from_value(payload) {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ProblemDetails::bad_request_data(format!(
                    "Invalid CsourceRegistration payload: {e}"
                ))),
            )
                .into_response();
        }
    };

    if csource.id.is_empty() {
        csource.id = format!(
            "urn:ngsi-ld:ContextSourceRegistration:{}",
            uuid::Uuid::new_v4()
        );
    }

    // SSRF Validation on destination endpoint
    if let Err(ssrf_err) = validate_endpoint(state.outbound_policy, &csource.endpoint) {
        return (
            StatusCode::BAD_REQUEST,
            Json(ProblemDetails::bad_request_data(format!(
                "Invalid or unsafe endpoint URL: {ssrf_err}"
            ))),
        )
            .into_response();
    }

    let prepare = async {
        if csource.r#type != "ContextSourceRegistration"
            || !athena_model::attributes::valid_uri(&csource.id)
        {
            return Err("Invalid registration identity".to_owned());
        }
        for information in &mut csource.information {
            if let Some(entities) = &mut information.entities {
                for entity in entities {
                    entity.r#type = state
                        .processor
                        .expand_term(&entity.r#type, context.clone())
                        .await?;
                }
            }
            for names in [
                &mut information.property_names,
                &mut information.relationship_names,
            ]
            .into_iter()
            .flatten()
            {
                for name in names {
                    *name = state.processor.expand_term(name, context.clone()).await?;
                }
            }
        }
        Ok::<_, String>(())
    }
    .await;
    if let Err(error) = prepare {
        return (
            StatusCode::BAD_REQUEST,
            Json(ProblemDetails::bad_request_data(error)),
        )
            .into_response();
    }
    let id = csource.id.clone();
    match state.csource_repo.create_csource(&csource).await {
        Ok(_) => {
            let mut headers = HeaderMap::new();
            if let Ok(loc) =
                HeaderValue::from_str(&format!("/ngsi-ld/v1/csourceRegistrations/{id}"))
            {
                headers.insert(header::LOCATION, loc);
            }
            (StatusCode::CREATED, headers, ()).into_response()
        }
        Err(err) if err.is_unique_violation() => (
            StatusCode::CONFLICT,
            Json(ProblemDetails::already_exists(format!(
                "ContextSourceRegistration '{id}' already exists"
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

pub async fn get_csource_registration(
    State(state): State<AppState>,
    Path(registration_id): Path<String>,
) -> Response {
    match state.csource_repo.get_csource_by_id(&registration_id).await {
        Ok(Some(csource)) => (StatusCode::OK, Json(csource)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ProblemDetails::not_found(format!(
                "ContextSourceRegistration '{registration_id}' not found"
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

pub async fn list_csource_registrations(
    State(state): State<AppState>,
    Query(opts): Query<ListCsourceOptions>,
) -> Response {
    match state
        .csource_repo
        .list_csources(
            opts.r#type.as_deref(),
            opts.id.as_deref(),
            opts.limit,
            opts.offset,
        )
        .await
    {
        Ok(list) => (StatusCode::OK, Json(list)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ProblemDetails::internal_error(e.to_string())),
        )
            .into_response(),
    }
}

pub async fn update_csource_registration(
    State(state): State<AppState>,
    Path(registration_id): Path<String>,
    Json(payload): Json<Value>,
) -> Response {
    // If endpoint is being updated, validate SSRF
    if let Some(endpoint) = payload.get("endpoint").and_then(Value::as_str) {
        if let Err(ssrf_err) = validate_endpoint(state.outbound_policy, endpoint) {
            return (
                StatusCode::BAD_REQUEST,
                Json(ProblemDetails::bad_request_data(format!(
                    "Invalid or unsafe endpoint URL: {ssrf_err}"
                ))),
            )
                .into_response();
        }
    }

    match state
        .csource_repo
        .update_csource(&registration_id, &payload)
        .await
    {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(StorageError::CsourceNotFound(_)) => (
            StatusCode::NOT_FOUND,
            Json(ProblemDetails::not_found(format!(
                "ContextSourceRegistration '{registration_id}' not found"
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

pub async fn delete_csource_registration(
    State(state): State<AppState>,
    Path(registration_id): Path<String>,
) -> Response {
    match state.csource_repo.delete_csource(&registration_id).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(StorageError::CsourceNotFound(_)) => (
            StatusCode::NOT_FOUND,
            Json(ProblemDetails::not_found(format!(
                "ContextSourceRegistration '{registration_id}' not found"
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
