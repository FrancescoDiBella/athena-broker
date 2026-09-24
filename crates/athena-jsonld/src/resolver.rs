use lru::LruCache;
use serde_json::Value;
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};
use thiserror::Error;

use crate::core_context::{get_etsi_core_mappings, is_etsi_core_context};

#[derive(Debug, Error)]
pub enum JsonLdError {
    #[error("Failed to fetch remote context '{uri}': {details}")]
    FetchError { uri: String, details: String },

    #[error("Invalid JSON-LD context format")]
    InvalidFormat,

    #[error("Parse error: {0}")]
    ParseError(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Default)]
pub struct ResolvedContext {
    pub terms: HashMap<String, String>,
    pub reverse: HashMap<String, String>,
}

impl ResolvedContext {
    pub fn new() -> Self {
        let mut ctx = Self::default();
        ctx.merge(&get_etsi_core_mappings());
        ctx
    }

    pub fn merge(&mut self, mappings: &HashMap<String, String>) {
        for (k, v) in mappings {
            self.terms.insert(k.clone(), v.clone());
            self.reverse.insert(v.clone(), k.clone());
        }
    }

    pub fn expand(&self, term: &str) -> String {
        if let Some(expanded) = self.terms.get(term) {
            return expanded.clone();
        }
        // Handle CURIE prefixes like "schema:name"
        if let Some(idx) = term.find(':') {
            let prefix = &term[..idx];
            let suffix = &term[idx + 1..];
            if let Some(prefix_uri) = self.terms.get(prefix) {
                return format!("{prefix_uri}{suffix}");
            }
        }
        term.to_string()
    }

    pub fn compact(&self, iri: &str) -> String {
        if let Some(compact) = self.reverse.get(iri) {
            return compact.clone();
        }
        for (prefix, base_uri) in &self.terms {
            if base_uri.ends_with('/') || base_uri.ends_with('#') {
                if let Some(suffix) = iri.strip_prefix(base_uri) {
                    return format!("{prefix}:{suffix}");
                }
            }
        }
        iri.to_string()
    }
}

pub struct ContextResolver {
    cache: Arc<Mutex<LruCache<String, HashMap<String, String>>>>,
    core_mappings: HashMap<String, String>,
    client: athena_http::OutboundClient,
}

impl Default for ContextResolver {
    fn default() -> Self {
        Self::new(1000)
    }
}

impl ContextResolver {
    pub fn new(cache_size: usize) -> Self {
        Self::with_policy(cache_size, athena_http::OutboundPolicy::default())
    }
    pub fn with_policy(cache_size: usize, policy: athena_http::OutboundPolicy) -> Self {
        let capacity = NonZeroUsize::new(cache_size).unwrap_or(NonZeroUsize::new(1000).unwrap());
        Self {
            cache: Arc::new(Mutex::new(LruCache::new(capacity))),
            core_mappings: get_etsi_core_mappings(),
            client: athena_http::OutboundClient::for_contexts(policy)
                .expect("HTTP client initialization"),
        }
    }

    pub async fn resolve(&self, context_value: &Value) -> Result<ResolvedContext, JsonLdError> {
        let mut resolved = ResolvedContext::new();

        match context_value {
            Value::String(uri) => {
                let map = self.resolve_single_uri(uri).await?;
                resolved.merge(&map);
            }
            Value::Object(obj) => {
                let map = self.parse_context_object(obj);
                resolved.merge(&map);
            }
            Value::Array(arr) => {
                for item in arr {
                    match item {
                        Value::String(uri) => {
                            let map = self.resolve_single_uri(uri).await?;
                            resolved.merge(&map);
                        }
                        Value::Object(obj) => {
                            let map = self.parse_context_object(obj);
                            resolved.merge(&map);
                        }
                        _ => {}
                    }
                }
            }
            _ => return Err(JsonLdError::InvalidFormat),
        }

        Ok(resolved)
    }

    async fn resolve_single_uri(&self, uri: &str) -> Result<HashMap<String, String>, JsonLdError> {
        if is_etsi_core_context(uri) {
            return Ok(self.core_mappings.clone());
        }

        {
            let mut cache = self.cache.lock().unwrap();
            if let Some(cached) = cache.get(uri) {
                return Ok(cached.clone());
            }
        }

        let failure = |details: String| JsonLdError::FetchError {
            uri: uri.into(),
            details,
        };
        let response = self
            .client
            .request(reqwest::Method::GET, uri)
            .map_err(failure)?
            .send()
            .await
            .map_err(|e| failure(e.to_string()))?
            .error_for_status()
            .map_err(|e| failure(e.to_string()))?;
        let body = athena_http::bounded_json(response, 1024 * 1024)
            .await
            .map_err(failure)?;
        if !body.get("@context").is_some_and(Value::is_object) {
            return Err(JsonLdError::InvalidFormat);
        }
        let mappings = self.extract_mappings_from_json(&body);
        self.cache
            .lock()
            .unwrap()
            .put(uri.to_string(), mappings.clone());
        Ok(mappings)
    }

    fn parse_context_object(
        &self,
        obj: &serde_json::Map<String, Value>,
    ) -> HashMap<String, String> {
        let mut map = HashMap::new();
        let target = if let Some(Value::Object(inner)) = obj.get("@context") {
            inner
        } else {
            obj
        };

        for (k, v) in target {
            if let Some(s) = v.as_str() {
                map.insert(k.clone(), s.to_string());
            } else if let Some(inner_obj) = v.as_object() {
                if let Some(id_val) = inner_obj.get("@id").and_then(Value::as_str) {
                    map.insert(k.clone(), id_val.to_string());
                }
            }
        }
        map
    }

    fn extract_mappings_from_json(&self, val: &Value) -> HashMap<String, String> {
        if let Some(obj) = val.as_object() {
            if let Some(Value::Object(ctx)) = obj.get("@context") {
                return self.parse_context_object(ctx);
            }
            return self.parse_context_object(obj);
        }
        HashMap::new()
    }
}
