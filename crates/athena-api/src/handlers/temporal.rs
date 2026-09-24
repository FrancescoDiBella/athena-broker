use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;

use crate::state::AppState;
use athena_model::{AggrMethod, ProblemDetails, TemporalQuery, TimeProperty, TimeRel};

#[derive(Debug, Deserialize)]
pub struct TemporalQueryRequest {
    pub timerel: Option<String>,
    #[serde(rename = "timeAt")]
    pub time_at: Option<String>,
    #[serde(rename = "endTimeAt")]
    pub end_time_at: Option<String>,
    pub timeproperty: Option<String>,
    pub attrs: Option<String>,
    #[serde(rename = "aggrMethods", alias = "aggrMethod")]
    pub aggr_method: Option<String>,
    #[serde(rename = "aggrPeriodDuration")]
    pub aggr_period_duration: Option<String>,
    #[serde(rename = "lastN")]
    pub last_n: Option<usize>,
    pub id: Option<String>,
    pub r#type: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

pub async fn create_temporal_entity(
    State(state): State<AppState>,
    Json(payload): Json<Value>,
) -> Response {
    let obj = match payload.as_object() {
        Some(o) => o,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ProblemDetails::bad_request_data(
                    "Payload must be a JSON object",
                )),
            )
                .into_response();
        }
    };

    let entity_id = match obj.get("id").and_then(Value::as_str) {
        Some(id) => id,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ProblemDetails::bad_request_data(
                    "Missing 'id' in temporal entity",
                )),
            )
                .into_response();
        }
    };

    let entity_type = match obj.get("type").and_then(Value::as_str) {
        Some(t) => t,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ProblemDetails::bad_request_data(
                    "Missing 'type' in temporal entity",
                )),
            )
                .into_response();
        }
    };

    match state
        .temporal_repo
        .create_temporal_entity(entity_id, entity_type, &payload)
        .await
    {
        Ok(_) => {
            let mut headers = HeaderMap::new();
            if let Ok(loc) =
                HeaderValue::from_str(&format!("/ngsi-ld/v1/temporal/entities/{entity_id}"))
            {
                headers.insert(header::LOCATION, loc);
            }
            (StatusCode::CREATED, headers, ()).into_response()
        }
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

pub async fn query_temporal_entity_by_id(
    State(state): State<AppState>,
    Path(entity_id): Path<String>,
    Query(params): Query<TemporalQueryRequest>,
) -> Response {
    let query = match build_temporal_query(&params) {
        Ok(q) => q,
        Err(err_resp) => return err_resp,
    };

    let filter_attrs: Option<Vec<String>> = params
        .attrs
        .map(|s| s.split(',').map(str::trim).map(String::from).collect());

    match state
        .temporal_repo
        .query_temporal(&entity_id, filter_attrs.as_deref(), &query)
        .await
    {
        Ok(res) => (StatusCode::OK, Json(res)).into_response(),
        Err(athena_storage::StorageError::Problem(problem)) => {
            let status = problem
                .status
                .and_then(|s| StatusCode::from_u16(s).ok())
                .unwrap_or(StatusCode::BAD_REQUEST);
            (status, Json(problem)).into_response()
        }
        Err(athena_storage::StorageError::EntityNotFound(id)) => (
            StatusCode::NOT_FOUND,
            Json(ProblemDetails::not_found(format!("Entity {id} not found"))),
        )
            .into_response(),

        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ProblemDetails::internal_error(e.to_string())),
        )
            .into_response(),
    }
}

pub async fn query_temporal_entities(
    State(state): State<AppState>,
    Query(params): Query<TemporalQueryRequest>,
) -> Response {
    let query = match build_temporal_query(&params) {
        Ok(q) => q,
        Err(err_resp) => return err_resp,
    };

    let filter_attrs: Option<Vec<String>> = params
        .attrs
        .as_ref()
        .map(|s| s.split(',').map(str::trim).map(String::from).collect());

    if let Some(ref entity_id) = params.id {
        match state
            .temporal_repo
            .query_temporal(entity_id, filter_attrs.as_deref(), &query)
            .await
        {
            Ok(res) => (StatusCode::OK, Json(vec![res])).into_response(),
            Err(athena_storage::StorageError::Problem(problem)) => {
                let status = problem
                    .status
                    .and_then(|s| StatusCode::from_u16(s).ok())
                    .unwrap_or(StatusCode::BAD_REQUEST);
                (status, Json(problem)).into_response()
            }
            Err(athena_storage::StorageError::EntityNotFound(id)) => (
                StatusCode::NOT_FOUND,
                Json(ProblemDetails::not_found(format!("Entity {id} not found"))),
            )
                .into_response(),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ProblemDetails::internal_error(e.to_string())),
            )
                .into_response(),
        }
    } else {
        let limit = params.limit.unwrap_or(20);
        let offset = params.offset.unwrap_or(0);
        let entity_type = params.r#type.as_deref();

        match state
            .temporal_repo
            .query_temporal_entities(entity_type, filter_attrs.as_deref(), &query, limit, offset)
            .await
        {
            Ok(res) => (StatusCode::OK, Json(res)).into_response(),
            Err(athena_storage::StorageError::Problem(problem)) => {
                let status = problem
                    .status
                    .and_then(|s| StatusCode::from_u16(s).ok())
                    .unwrap_or(StatusCode::BAD_REQUEST);
                (status, Json(problem)).into_response()
            }
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ProblemDetails::internal_error(e.to_string())),
            )
                .into_response(),
        }
    }
}

fn build_temporal_query(params: &TemporalQueryRequest) -> Result<TemporalQuery, Response> {
    let timerel_str = match &params.timerel {
        Some(t) => t.to_lowercase(),
        None => "after".to_string(),
    };

    let timerel = match timerel_str.as_str() {
        "before" => TimeRel::Before,
        "after" => TimeRel::After,
        "between" => TimeRel::Between,
        other => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ProblemDetails::bad_request_data(format!(
                    "Invalid timerel '{other}'"
                ))),
            )
                .into_response());
        }
    };

    let time_at = match params.time_at.as_deref() {
        Some(s) => match DateTime::parse_from_rfc3339(s) {
            Ok(dt) => dt.with_timezone(&Utc),
            Err(_) => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(ProblemDetails::bad_request_data("Invalid timeAt timestamp")),
                )
                    .into_response());
            }
        },
        None if params.timerel.is_none() => DateTime::UNIX_EPOCH,
        None => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ProblemDetails::bad_request_data(
                    "timeAt is required with timerel",
                )),
            )
                .into_response())
        }
    };

    let end_time_at = match params.end_time_at.as_deref() {
        Some(s) => match DateTime::parse_from_rfc3339(s) {
            Ok(dt) => Some(dt.with_timezone(&Utc)),
            Err(_) => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(ProblemDetails::bad_request_data(
                        "Invalid endTimeAt timestamp",
                    )),
                )
                    .into_response());
            }
        },
        None => None,
    };

    if timerel == TimeRel::Between {
        if let Some(ref end) = end_time_at {
            if time_at > *end {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(ProblemDetails::bad_request_data(
                        "timeAt cannot be after endTimeAt in timerel=between query",
                    )),
                )
                    .into_response());
            }
        }
    }

    let timeproperty = match params.timeproperty.as_deref() {
        Some("createdAt") => TimeProperty::CreatedAt,
        Some("modifiedAt") => TimeProperty::ModifiedAt,
        None | Some("observedAt") => TimeProperty::ObservedAt,
        Some(_) => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ProblemDetails::bad_request_data("Invalid timeproperty")),
            )
                .into_response())
        }
    };

    let mut aggr_methods = Vec::new();
    for item in params
        .aggr_method
        .as_deref()
        .unwrap_or("")
        .split(',')
        .filter(|s| !s.is_empty())
    {
        let aggr_method = match Some(item) {
            Some(m) => match m.to_lowercase().as_str() {
                "avg" => Some(AggrMethod::Avg),
                "min" => Some(AggrMethod::Min),
                "max" => Some(AggrMethod::Max),
                "sum" => Some(AggrMethod::Sum),
                "totalcount" | "count" => Some(AggrMethod::TotalCount),
                "distinctcount" => Some(AggrMethod::DistinctCount),
                "stddev" => Some(AggrMethod::Stddev),
                "sumsq" => Some(AggrMethod::Sumsq),
                other => {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(ProblemDetails::bad_request_data(format!(
                            "Unsupported aggrMethod '{other}'"
                        ))),
                    )
                        .into_response());
                }
            },
            None => None,
        };

        if let Some(method) = aggr_method {
            if !aggr_methods.contains(&method) {
                aggr_methods.push(method);
            }
        }
    }
    Ok(TemporalQuery {
        timerel,
        time_at,
        end_time_at,
        timeproperty,
        aggr_method: aggr_methods.first().copied(),
        aggr_methods,
        aggr_period_duration: params.aggr_period_duration.clone(),
        last_n: params.last_n,
    })
}

#[derive(Deserialize)]
pub struct DeleteTemporalOptions {
    #[serde(rename = "datasetId")]
    pub dataset: Option<String>,
    #[serde(rename = "deleteAll", default)]
    pub delete_all: bool,
}
fn temporal_mutation_response(result: Result<(), athena_storage::StorageError>) -> Response {
    match result {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(athena_storage::StorageError::EntityNotFound(id)) => {
            (StatusCode::NOT_FOUND, Json(ProblemDetails::not_found(id))).into_response()
        }
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
pub async fn delete_temporal_entity(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    temporal_mutation_response(
        state
            .temporal_repo
            .delete_temporal(&id, None, None, None, true)
            .await,
    )
}
pub async fn delete_temporal_attribute(
    State(state): State<AppState>,
    Path((id, attribute)): Path<(String, String)>,
    axum::Extension(ctx): axum::Extension<crate::middleware::RequestContext>,
    Query(options): Query<DeleteTemporalOptions>,
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
    let attribute = match state
        .processor
        .expand_term(
            &attribute,
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
    temporal_mutation_response(
        state
            .temporal_repo
            .delete_temporal(
                &id,
                Some(&attribute),
                options.dataset.as_deref(),
                None,
                options.delete_all,
            )
            .await,
    )
}
pub async fn delete_temporal_instance(
    State(state): State<AppState>,
    Path((id, attribute, instance)): Path<(String, String, String)>,
    axum::Extension(ctx): axum::Extension<crate::middleware::RequestContext>,
) -> Response {
    let attribute = match state
        .processor
        .expand_term(
            &attribute,
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
    temporal_mutation_response(
        state
            .temporal_repo
            .delete_temporal(&id, Some(&attribute), None, Some(&instance), true)
            .await,
    )
}
pub async fn update_temporal_instance(
    State(state): State<AppState>,
    Path((id, attribute, instance)): Path<(String, String, String)>,
    axum::Extension(ctx): axum::Extension<crate::middleware::RequestContext>,
    Json(patch): Json<Value>,
) -> Response {
    let attribute = match state
        .processor
        .expand_term(
            &attribute,
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
    temporal_mutation_response(
        state
            .temporal_repo
            .update_temporal_instance(&id, &attribute, &instance, &patch)
            .await,
    )
}
