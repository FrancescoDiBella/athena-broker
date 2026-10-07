use crate::state::AppState;
use athena_model::{LinkHeader, ProblemDetails, ETSI_CORE_CONTEXT_URL};
use axum::{
    body::{to_bytes, Body},
    extract::{Request, State},
    http::{header, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};

#[derive(Clone, Debug, Default)]
pub struct RequestContext {
    pub context_uri: Option<String>,
    pub body_context: Option<Value>,
}
fn problem(status: StatusCode, message: impl Into<String>) -> Response {
    let mut detail = ProblemDetails::bad_request_data(message);
    detail.status = Some(status.as_u16());
    let mut response = (status, Json(detail)).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/problem+json"),
    );
    if status == StatusCode::SERVICE_UNAVAILABLE {
        response
            .headers_mut()
            .insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
    }
    response
}
fn ld_accepted(value: &str) -> Option<bool> {
    let mut best: Option<(f32, usize, bool)> = None;
    for item in value.split(',') {
        let mut parts = item.trim().split(';');
        let mime = parts.next().unwrap_or("").trim();
        let ld = match mime {
            "application/ld+json" => true,
            "application/json" | "application/*" | "*/*" => false,
            _ => continue,
        };
        let q = parts
            .find_map(|p| {
                p.trim()
                    .strip_prefix("q=")
                    .and_then(|v| v.parse::<f32>().ok())
            })
            .unwrap_or(1.);
        let specificity = if mime.contains('*') { 0 } else { 1 };
        if q > 0. && q <= 1. && best.is_none_or(|b| (q, specificity) > (b.0, b.1)) {
            best = Some((q, specificity, ld));
        }
    }
    best.map(|b| b.2)
}

pub async fn ngsi_ld_headers_middleware(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    if !request.uri().path().starts_with("/ngsi-ld/") {
        return next.run(request).await;
    }
    // Single-tenant installations must not silently mix requests for different tenants.
    if request.headers().contains_key("NGSILD-Tenant") {
        return problem(
            StatusCode::NOT_IMPLEMENTED,
            "This installation has no tenant isolation; NGSILD-Tenant is not supported",
        );
    }
    let accept = request
        .headers()
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/json");
    let Some(ld) = ld_accepted(accept) else {
        return problem(
            StatusCode::NOT_ACCEPTABLE,
            "Supported representations: application/json, application/ld+json",
        );
    };
    let mut links = Vec::new();
    for value in request.headers().get_all(header::LINK) {
        if let Ok(text) = value.to_str() {
            for link in LinkHeader::parse_list(text) {
                if link.rel == athena_model::JSON_LD_CONTEXT_REL {
                    links.push(link.uri);
                }
            }
        }
    }
    if links.len() > 1 {
        return problem(
            StatusCode::BAD_REQUEST,
            "Only one JSON-LD context Link is allowed",
        );
    }
    let context_uri = links.pop();
    let output_context = json!(context_uri.as_deref().unwrap_or(ETSI_CORE_CONTEXT_URL));
    let mut body_context = None;
    let path = request.uri().path().to_owned();
    let entity_document = path.contains("/entities") || path.contains("/entityOperations/");
    let is_delete = path.ends_with("entityOperations/delete");
    let has_body = matches!(
        *request.method(),
        axum::http::Method::POST | axum::http::Method::PATCH | axum::http::Method::PUT
    );
    // Keep subscription controls available when the ingestion queue is saturated.
    let _write_permit = if has_body && entity_document {
        let Ok(permit) = state.write_slots.clone().try_acquire_owned() else {
            return problem(
                StatusCode::SERVICE_UNAVAILABLE,
                "Write concurrency limit reached; retry later",
            );
        };
        let mut admission = state.admission.lock().await;
        if admission.0.elapsed() >= std::time::Duration::from_secs(1) {
            let limit = state.limits.max_pending_events;
            let check=sqlx::query_scalar::<_,bool>("SELECT (SELECT count(*) FROM (SELECT id FROM entity_events WHERE processed_at IS NULL LIMIT $1) e)<$1 AND (SELECT count(*) FROM (SELECT id FROM notification_jobs WHERE status='pending' LIMIT $1) j)<$1").bind(limit).fetch_one(&state.pool);
            admission.1 = matches!(
                tokio::time::timeout(std::time::Duration::from_secs(2), check).await,
                Ok(Ok(true))
            );
            admission.0 = std::time::Instant::now();
        }
        if !admission.1 {
            return problem(
                StatusCode::SERVICE_UNAVAILABLE,
                "Durable queue capacity reached or database unavailable; retry later",
            );
        }
        Some(permit)
    } else {
        None
    };
    if has_body {
        let content_type = request
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("");
        let input_ld = content_type == "application/ld+json";
        if !input_ld && content_type != "application/json" {
            return problem(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "Use application/json or application/ld+json",
            );
        }
        if input_ld && context_uri.is_some() {
            return problem(
                StatusCode::BAD_REQUEST,
                "application/ld+json uses @context in the body, not a context Link",
            );
        }
        let (mut parts, body) = request.into_parts();
        let bytes = match to_bytes(body, state.limits.max_body_bytes).await {
            Ok(b) => b,
            Err(_) => {
                return problem(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "Payload exceeds the configured body limit",
                )
            }
        };
        let mut value: Value = match serde_json::from_slice(&bytes) {
            Ok(v) => v,
            Err(_) => return problem(StatusCode::BAD_REQUEST, "Invalid JSON payload"),
        };
        let items = if let Value::Array(items) = &mut value {
            items.iter_mut().collect::<Vec<_>>()
        } else {
            vec![&mut value]
        };
        if items.len() > 1000 {
            return problem(StatusCode::PAYLOAD_TOO_LARGE, "Batch exceeds 1000 elements");
        }
        for item in items {
            if is_delete {
                continue;
            }
            let context = item.get("@context").cloned();
            body_context = context.clone();
            if input_ld && context.is_none() {
                return problem(
                    StatusCode::BAD_REQUEST,
                    "application/ld+json requires @context",
                );
            }
            if !input_ld && context.is_some() {
                return problem(
                    StatusCode::BAD_REQUEST,
                    "application/json must supply context through Link",
                );
            }
            if entity_document {
                match state
                    .processor
                    .normalize(
                        item.clone(),
                        context.unwrap_or_else(|| output_context.clone()),
                    )
                    .await
                {
                    Ok(normalized) => *item = normalized,
                    Err(e) => {
                        return problem(StatusCode::BAD_REQUEST, format!("Invalid JSON-LD: {e}"))
                    }
                }
            }
        }
        parts.headers.remove(header::CONTENT_LENGTH);
        parts.headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        request = Request::from_parts(parts, Body::from(value.to_string()));
    }
    if let Some(query) = request.uri().query().map(str::to_owned) {
        let mut pairs = url::form_urlencoded::parse(query.as_bytes())
            .into_owned()
            .collect::<Vec<_>>();
        for (key, value) in &mut pairs {
            if matches!(key.as_str(), "type" | "attrs" | "geoproperty") {
                let mut terms = Vec::new();
                for term in value.split(',') {
                    match state
                        .processor
                        .expand_term(term, output_context.clone())
                        .await
                    {
                        Ok(v) => terms.push(v),
                        Err(e) => return problem(StatusCode::BAD_REQUEST, e),
                    }
                }
                *value = terms.join(",");
            } else if key == "q" {
                match expand_q(&state, &output_context, value).await {
                    Ok(v) => *value = v,
                    Err(e) => return problem(StatusCode::BAD_REQUEST, e),
                }
            }
        }
        let query = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(pairs)
            .finish();
        if let Ok(uri) = format!("{}?{}", request.uri().path(), query).parse() {
            *request.uri_mut() = uri;
        }
    }
    request.extensions_mut().insert(RequestContext {
        context_uri: context_uri.clone(),
        body_context,
    });
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(header::VARY, HeaderValue::from_static("Accept, Link"));
    if response.status().is_client_error() || response.status().is_server_error() {
        let json = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.contains("json"));
        if !json {
            let status = response.status();
            let bytes = to_bytes(response.into_body(), 64 * 1024)
                .await
                .unwrap_or_default();
            return problem(status, String::from_utf8_lossy(&bytes).into_owned());
        }
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/problem+json"),
        );
        return response;
    }
    let is_json = response
        .headers()
        .get(header::CONTENT_TYPE)
        .is_some_and(|v| v.to_str().unwrap_or("").starts_with("application/json"));
    if is_json {
        let (mut parts, body) = response.into_parts();
        let bytes = match to_bytes(body, 32 * 1024 * 1024).await {
            Ok(v) => v,
            Err(_) => return problem(StatusCode::INTERNAL_SERVER_ERROR, "Response too large"),
        };
        let mut value: Value = match serde_json::from_slice(&bytes) {
            Ok(v) => v,
            Err(_) => {
                return problem(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Invalid response serialization",
                )
            }
        };
        let items = if let Value::Array(items) = &mut value {
            items.iter_mut().collect::<Vec<_>>()
        } else {
            vec![&mut value]
        };
        for item in items {
            if entity_document && item.get("id").is_some() && item.get("type").is_some() {
                match state
                    .processor
                    .compact(item.clone(), output_context.clone())
                    .await
                {
                    Ok(mut compact) => {
                        // JSON-LD compaction collapses singleton sets. Temporal attribute
                        // histories are arrays even when only one instance is selected.
                        if path.starts_with("/ngsi-ld/v1/temporal/entities") {
                            if let Some(object) = compact.as_object_mut() {
                                for (key, value) in object {
                                    if !matches!(
                                        key.as_str(),
                                        "id" | "type"
                                            | "@context"
                                            | "scope"
                                            | "createdAt"
                                            | "modifiedAt"
                                    ) && !value.is_array()
                                    {
                                        *value = Value::Array(vec![value.take()]);
                                    }
                                }
                            }
                        }
                        *item = compact;
                    }
                    Err(e) => {
                        return problem(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            format!("JSON-LD response: {e}"),
                        )
                    }
                }
            }
            if let Some(map) = item.as_object_mut() {
                map.remove("@context");
                if ld {
                    map.insert("@context".into(), output_context.clone());
                }
            }
        }
        parts.headers.remove(header::CONTENT_LENGTH);
        parts.headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static(if ld {
                "application/ld+json"
            } else {
                "application/json"
            }),
        );
        response = Response::from_parts(parts, Body::from(value.to_string()));
    }
    if !ld {
        if let Ok(value) = HeaderValue::from_str(
            &LinkHeader::new_context(context_uri.unwrap_or_else(|| ETSI_CORE_CONTEXT_URL.into()))
                .to_header_value(),
        ) {
            response.headers_mut().append(header::LINK, value);
        }
    }
    response
}

#[derive(Default)]
pub struct RequestMetrics {
    pub total: std::sync::atomic::AtomicU64,
    pub failures: std::sync::atomic::AtomicU64,
    pub in_flight: std::sync::atomic::AtomicU64,
    pub duration_micros: std::sync::atomic::AtomicU64,
}

pub async fn request_metrics(
    axum::extract::State(state): axum::extract::State<crate::state::AppState>,
    request: Request,
    next: Next,
) -> Response {
    use std::sync::atomic::Ordering::Relaxed;
    struct Guard(std::sync::Arc<RequestMetrics>);
    impl Drop for Guard {
        fn drop(&mut self) {
            self.0.in_flight.fetch_sub(1, Relaxed);
        }
    }
    state.metrics.in_flight.fetch_add(1, Relaxed);
    let _guard = Guard(state.metrics.clone());
    let start = std::time::Instant::now();
    let response = next.run(request).await;
    state.metrics.total.fetch_add(1, Relaxed);
    if response.status().is_server_error() {
        state.metrics.failures.fetch_add(1, Relaxed);
    }
    state.metrics.duration_micros.fetch_add(
        start.elapsed().as_micros().min(u64::MAX as u128) as u64,
        Relaxed,
    );
    response
}

pub async fn expand_q(state: &AppState, context: &Value, q: &str) -> Result<String, String> {
    use athena_query::lexer::Token;
    let mut tokens = athena_query::lexer::Lexer::new(q)
        .tokenize()
        .map_err(|e| e.to_string())?;
    if tokens.len() > 512 {
        return Err("Query complexity limit".into());
    }
    for i in 0..tokens.len().saturating_sub(1) {
        if matches!(
            tokens[i + 1],
            Token::Equal
                | Token::NotEqual
                | Token::Greater
                | Token::GreaterEqual
                | Token::Less
                | Token::LessEqual
                | Token::PatternMatch
                | Token::NotPatternMatch
        ) {
            if let Token::Ident(term) = &tokens[i] {
                tokens[i] = Token::Ident(state.processor.expand_term(term, context.clone()).await?);
            }
        }
    }
    Ok(tokens
        .into_iter()
        .map(|t| match t {
            Token::Ident(v) => v,
            Token::StringLit(v) => json!(v).to_string(),
            Token::NumberLit(v) => v.to_string(),
            Token::BoolLit(v) => v.to_string(),
            Token::Equal => "==".into(),
            Token::NotEqual => "!=".into(),
            Token::Greater => ">".into(),
            Token::GreaterEqual => ">=".into(),
            Token::Less => "<".into(),
            Token::LessEqual => "<=".into(),
            Token::PatternMatch => "~=".into(),
            Token::NotPatternMatch => "!~=".into(),
            Token::And => ";".into(),
            Token::Or => "|".into(),
            Token::LParen => "(".into(),
            Token::RParen => ")".into(),
            Token::LBracket => "[".into(),
            Token::RBracket => "]".into(),
            Token::Comma => ",".into(),
            Token::DotDot => "..".into(),
            Token::Eof => String::new(),
        })
        .collect::<Vec<_>>()
        .join(" "))
}
