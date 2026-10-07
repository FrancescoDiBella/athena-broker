use axum::{
    extract::{Path, Query, RawQuery, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::Value;

use crate::state::AppState;
use athena_model::{Entity, ProblemDetails};
use athena_query::{GeoQueryParser, Parser};
use athena_storage::{EntityQueryParams, StorageError};

#[derive(Debug, Deserialize)]
pub struct EntityQueryRequest {
    #[serde(rename = "splitEntities")]
    pub split_entities: Option<bool>,
    pub local: Option<bool>,
    pub id: Option<String>,
    #[serde(rename = "idPattern")]
    pub id_pattern: Option<String>,
    pub r#type: Option<String>,
    pub q: Option<String>,
    pub georel: Option<String>,
    pub geometry: Option<String>,
    pub coordinates: Option<String>,
    pub geoproperty: Option<String>,
    pub attrs: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub options: Option<String>,
    pub count: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct GetEntityOptions {
    pub options: Option<String>,
    pub attrs: Option<String>,
}

pub async fn create_entity(State(state): State<AppState>, Json(payload): Json<Value>) -> Response {
    let entity = match Entity::from_json(payload) {
        Ok(e) => e,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ProblemDetails::bad_request_data(e.to_string())),
            )
                .into_response();
        }
    };

    let id = entity.id.clone();
    let mutated_attrs: Vec<String> = entity.attributes.keys().cloned().collect();

    match state.entity_repo.create_entity(&entity).await {
        Ok(_) => {
            // Notify subscription engine asynchronously
            state
                .subscription_engine
                .notify_mutation(entity, mutated_attrs)
                .await;

            let mut headers = HeaderMap::new();
            if let Ok(loc) = HeaderValue::from_str(&format!("/ngsi-ld/v1/entities/{id}")) {
                headers.insert(header::LOCATION, loc);
            }
            (StatusCode::CREATED, headers, ()).into_response()
        }
        Err(StorageError::EntityAlreadyExists(_)) => (
            StatusCode::CONFLICT,
            Json(ProblemDetails::already_exists(format!(
                "Entity '{id}' already exists"
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

pub async fn get_entity_by_id(
    State(state): State<AppState>,
    Path(entity_id): Path<String>,
    Query(opts): Query<GetEntityOptions>,
) -> Response {
    let filter_attrs: Option<Vec<String>> = opts
        .attrs
        .map(|s| s.split(',').map(str::trim).map(String::from).collect());

    match state
        .entity_repo
        .get_entity_by_id(&entity_id, filter_attrs.as_deref())
        .await
    {
        Ok(Some(entity)) => {
            let is_key_values = opts
                .options
                .as_deref()
                .map(|o| o.contains("keyValues"))
                .unwrap_or(false);
            let include_sys = opts
                .options
                .as_deref()
                .map(|o| o.contains("sysAttrs"))
                .unwrap_or(false);

            let res_json = if is_key_values {
                entity.to_key_values()
            } else {
                entity.to_normalized(include_sys)
            };

            (StatusCode::OK, Json(res_json)).into_response()
        }
        Ok(None) => (
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

pub async fn query_entities(
    State(state): State<AppState>,
    RawQuery(raw_query): RawQuery,
    Query(query): Query<EntityQueryRequest>,
) -> Response {
    if let Some(pattern) = &query.id_pattern {
        if let Err(error) = Parser::validate_pattern(pattern) {
            return (
                StatusCode::BAD_REQUEST,
                Json(ProblemDetails::bad_request_data(error.to_string())),
            )
                .into_response();
        }
    }
    if query.limit.is_some_and(|v| !(0..=1000).contains(&v))
        || query.offset.is_some_and(|v| v < 0)
        || (query.limit == Some(0) && query.count != Some(true))
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(ProblemDetails::bad_request_data(
                "limit must be 0..1000 and offset must be nonnegative",
            )),
        )
            .into_response();
    }
    let q_expr = match &query.q {
        Some(q_str) => match Parser::parse_str(q_str) {
            Ok(expr) => Some(expr),
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(ProblemDetails::bad_request_data(format!(
                        "Invalid 'q' parameter: {e}"
                    ))),
                )
                    .into_response();
            }
        },
        None => None,
    };

    let geo_q = match GeoQueryParser::parse(
        query.georel.as_deref(),
        query.geometry.as_deref(),
        query.coordinates.as_deref(),
        query.geoproperty.as_deref(),
    ) {
        Ok(g) => g,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ProblemDetails::bad_request_data(format!(
                    "Invalid geoQ parameter: {e}"
                ))),
            )
                .into_response();
        }
    };

    let filter_attrs: Option<Vec<String>> = query
        .attrs
        .map(|s| s.split(',').map(str::trim).map(String::from).collect());

    let params = EntityQueryParams {
        id: query.id,
        id_pattern: query.id_pattern,
        type_: query.r#type,
        q: q_expr,
        geo_q,
        attrs: filter_attrs,
        limit: query.limit,
        offset: query.offset,
    };

    let mut headers = HeaderMap::new();
    let result = if query.local.unwrap_or(false) {
        match state.entity_repo.count_entities(&params).await {
            Ok(count) => state
                .entity_repo
                .query_entities(&params)
                .await
                .map(|entities| crate::federation::FederatedResult { entities, count })
                .map_err(|e| e.to_string()),
            Err(error) => Err(error.to_string()),
        }
    } else {
        state
            .federation_service
            .query_federated(
                &state.entity_repo,
                &state.csource_repo,
                &params,
                raw_query.as_deref(),
                query.split_entities.unwrap_or(false),
            )
            .await
    };
    match result {
        Ok(result) => {
            crate::handlers::pagination::links(
                &mut headers,
                "/ngsi-ld/v1/entities",
                raw_query.as_deref(),
                params.limit.unwrap_or(20),
                params.offset.unwrap_or(0),
                result.count,
            );
            if query.count.unwrap_or(false) {
                headers.insert(
                    "NGSILD-Results-Count",
                    HeaderValue::from_str(&result.count.to_string()).unwrap(),
                );
            }
            let entities = result.entities;
            let is_key_values = query
                .options
                .as_deref()
                .map(|o| o.contains("keyValues"))
                .unwrap_or(false);
            let include_sys = query
                .options
                .as_deref()
                .map(|o| o.contains("sysAttrs"))
                .unwrap_or(false);

            let list: Vec<Value> = entities
                .into_iter()
                .map(|e| {
                    if is_key_values {
                        e.to_key_values()
                    } else {
                        e.to_normalized(include_sys)
                    }
                })
                .collect();

            (StatusCode::OK, headers, Json(list)).into_response()
        }
        Err(e) => {
            let status = if query.local.unwrap_or(false) {
                StatusCode::INTERNAL_SERVER_ERROR
            } else {
                StatusCode::BAD_GATEWAY
            };
            let mut problem = ProblemDetails::internal_error(e);
            problem.status = Some(status.as_u16());
            (status, Json(problem)).into_response()
        }
    }
}

pub async fn update_entity(
    State(state): State<AppState>,
    Path(entity_id): Path<String>,
    Json(payload): Json<Value>,
) -> Response {
    if !payload.is_object() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ProblemDetails::bad_request_data(
                "Payload must be a JSON object",
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
            if let Ok(Some(updated_entity)) =
                state.entity_repo.get_entity_by_id(&entity_id, None).await
            {
                state
                    .subscription_engine
                    .notify_mutation(updated_entity, mutated_attrs)
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

pub async fn replace_entity(
    State(state): State<AppState>,
    Path(entity_id): Path<String>,
    Json(payload): Json<Value>,
) -> Response {
    crate::handlers::attrs::mutation_response(
        state.entity_repo.replace_entity(&entity_id, &payload).await,
    )
}

pub async fn delete_entity(
    State(state): State<AppState>,
    Path(entity_id): Path<String>,
) -> Response {
    match state.entity_repo.delete_entity(&entity_id).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
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
