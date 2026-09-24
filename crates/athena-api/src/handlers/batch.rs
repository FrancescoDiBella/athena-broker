use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::Value;

use crate::state::AppState;
use athena_model::{BatchOperationResult, Entity, ProblemDetails};

pub async fn batch_create(State(state): State<AppState>, Json(payload): Json<Value>) -> Response {
    let arr = match payload.as_array() {
        Some(a) => a,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ProblemDetails::bad_request_data(
                    "Batch payload must be a JSON array of entities",
                )),
            )
                .into_response();
        }
    };

    let mut entities = Vec::new();
    let mut initial_result = BatchOperationResult::new();

    for item in arr {
        match Entity::from_json(item.clone()) {
            Ok(e) => entities.push(e),
            Err(e) => {
                let id = item.get("id").and_then(Value::as_str).unwrap_or("unknown");
                initial_result.add_error(id, ProblemDetails::bad_request_data(e.to_string()));
            }
        }
    }

    let mut result = match state.entity_repo.batch_create(&entities).await {
        Ok(res) => res,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ProblemDetails::internal_error(e.to_string())),
            )
                .into_response();
        }
    };

    // Combine any upfront parse errors
    result.errors.extend(initial_result.errors);

    // Notify subscriptions for created entities
    for entity in entities {
        if result.success.contains(&entity.id) {
            let attrs = entity.attributes.keys().cloned().collect();
            state
                .subscription_engine
                .notify_mutation(entity, attrs)
                .await;
        }
    }

    if result.is_all_success() {
        (StatusCode::CREATED, Json(result.success)).into_response()
    } else {
        (StatusCode::MULTI_STATUS, Json(result)).into_response()
    }
}

pub async fn batch_upsert(State(state): State<AppState>, Json(payload): Json<Value>) -> Response {
    let arr = match payload.as_array() {
        Some(a) => a,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ProblemDetails::bad_request_data(
                    "Batch payload must be a JSON array of entities",
                )),
            )
                .into_response();
        }
    };

    let mut entities = Vec::new();
    let mut initial_result = BatchOperationResult::new();

    for item in arr {
        match Entity::from_json(item.clone()) {
            Ok(e) => entities.push(e),
            Err(e) => {
                let id = item.get("id").and_then(Value::as_str).unwrap_or("unknown");
                initial_result.add_error(id, ProblemDetails::bad_request_data(e.to_string()));
            }
        }
    }

    let mut result = match state.entity_repo.batch_upsert(&entities).await {
        Ok(res) => res,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ProblemDetails::internal_error(e.to_string())),
            )
                .into_response();
        }
    };

    result.errors.extend(initial_result.errors);

    for entity in entities {
        if result.success.contains(&entity.id) {
            let attrs = entity.attributes.keys().cloned().collect();
            state
                .subscription_engine
                .notify_mutation(entity, attrs)
                .await;
        }
    }

    if result.is_all_success() {
        (StatusCode::NO_CONTENT, ()).into_response()
    } else {
        (StatusCode::MULTI_STATUS, Json(result)).into_response()
    }
}

pub async fn batch_update(State(state): State<AppState>, Json(payload): Json<Value>) -> Response {
    let arr = match payload.as_array() {
        Some(a) => a,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ProblemDetails::bad_request_data(
                    "Batch payload must be a JSON array of entities",
                )),
            )
                .into_response();
        }
    };

    let mut entities = Vec::new();
    let mut initial_result = BatchOperationResult::new();

    for item in arr {
        match Entity::from_json(item.clone()) {
            Ok(e) => entities.push(e),
            Err(e) => {
                let id = item.get("id").and_then(Value::as_str).unwrap_or("unknown");
                initial_result.add_error(id, ProblemDetails::bad_request_data(e.to_string()));
            }
        }
    }

    let mut result = match state.entity_repo.batch_update(&entities).await {
        Ok(res) => res,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ProblemDetails::internal_error(e.to_string())),
            )
                .into_response();
        }
    };

    result.errors.extend(initial_result.errors);

    for entity in entities {
        if result.success.contains(&entity.id) {
            let attrs = entity.attributes.keys().cloned().collect();
            state
                .subscription_engine
                .notify_mutation(entity, attrs)
                .await;
        }
    }

    if result.is_all_success() {
        StatusCode::NO_CONTENT.into_response()
    } else {
        (StatusCode::MULTI_STATUS, Json(result)).into_response()
    }
}

pub async fn batch_delete(State(state): State<AppState>, Json(payload): Json<Value>) -> Response {
    let ids: Vec<String> = match payload.as_array() {
        Some(arr) => arr
            .iter()
            .filter_map(Value::as_str)
            .map(String::from)
            .collect(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ProblemDetails::bad_request_data(
                    "Payload must be an array of entity IDs",
                )),
            )
                .into_response();
        }
    };

    let result = match state.entity_repo.batch_delete(&ids).await {
        Ok(res) => res,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ProblemDetails::internal_error(e.to_string())),
            )
                .into_response();
        }
    };

    if result.is_all_success() {
        StatusCode::NO_CONTENT.into_response()
    } else {
        (StatusCode::MULTI_STATUS, Json(result)).into_response()
    }
}
