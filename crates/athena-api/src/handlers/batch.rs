use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::Value;

use crate::state::AppState;
use athena_model::{BatchOperationResult, Entity, ProblemDetails};

fn items(payload: &Value) -> Result<&[Value], ProblemDetails> {
    match payload.as_array() {
        Some(items)
            if !items.is_empty() && items.len() <= 1000 && !items.iter().any(Value::is_null) =>
        {
            Ok(items)
        }
        _ => Err(ProblemDetails::bad_request_data(
            "Batch payload requires 1..1000 non-null items",
        )),
    }
}

pub async fn batch_create(State(state): State<AppState>, Json(payload): Json<Value>) -> Response {
    let arr = match items(&payload) {
        Ok(arr) => arr,
        Err(problem) => return (StatusCode::BAD_REQUEST, Json(problem)).into_response(),
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
    let arr = match items(&payload) {
        Ok(arr) => arr,
        Err(problem) => return (StatusCode::BAD_REQUEST, Json(problem)).into_response(),
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
    let arr = match items(&payload) {
        Ok(arr) => arr,
        Err(problem) => return (StatusCode::BAD_REQUEST, Json(problem)).into_response(),
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
    let arr = match items(&payload) {
        Ok(arr) => arr,
        Err(problem) => return (StatusCode::BAD_REQUEST, Json(problem)).into_response(),
    };
    if !arr
        .iter()
        .all(|v| v.as_str().is_some_and(athena_model::attributes::valid_uri))
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(ProblemDetails::bad_request_data(
                "Every batch-delete item must be an absolute entity URI",
            )),
        )
            .into_response();
    }
    let ids: Vec<String> = arr.iter().map(|v| v.as_str().unwrap().to_owned()).collect();

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
