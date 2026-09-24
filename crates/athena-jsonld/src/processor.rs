//! Standards-based expansion/compaction with a bounded, checked document loader.
use athena_http::{bounded_json, OutboundClient, OutboundPolicy};
use json_ld::{
    compaction::Compact,
    context_processing::{Process, ProcessedOwned},
    expansion::Expand,
    loader::ExtractContext,
};
use json_ld::{
    syntax::{Parse, Print},
    Iri, IriBuf, LoadError, Loader, RemoteDocument,
};
use lru::LruCache;
use serde_json::{json, Value};
use std::{
    num::NonZeroUsize,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub const CORE: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld";
const CANONICAL: &str = "urn:athena:canonical-context";
const OUTPUT: &str = "urn:athena:output-context";
type Document = RemoteDocument<IriBuf>;

#[derive(Clone)]
pub struct Processor {
    client: OutboundClient,
    cache: Arc<Mutex<LruCache<String, (Instant, Document)>>>,
    slots: Arc<tokio::sync::Semaphore>,
    terms: Arc<Mutex<LruCache<(String, String), (Instant, String)>>>,
    processed:
        Arc<Mutex<LruCache<String, (Instant, Arc<ProcessedOwned<IriBuf, json_ld::BlankIdBuf>>)>>>,
}
struct Session<'a> {
    processor: &'a Processor,
    output: Value,
    loads: std::sync::atomic::AtomicUsize,
}
fn failure(url: &Iri, message: impl Into<String>) -> LoadError {
    LoadError::new(url.to_owned(), std::io::Error::other(message.into()))
}
fn document(url: Option<IriBuf>, value: &Value) -> Result<Document, String> {
    let parsed = json_ld::syntax::Value::parse_str(&value.to_string())
        .map_err(|e| e.to_string())?
        .0;
    Ok(RemoteDocument::new(
        url,
        Some("application/ld+json".parse().unwrap()),
        parsed,
    ))
}
fn core() -> Value {
    serde_json::from_str(include_str!("../contexts/core-v1.9.jsonld"))
        .expect("Bundled core context")
}
fn canonical() -> Value {
    let text = include_str!("../contexts/core-v1.9.jsonld")
        .replace("ngsi-ld:", "https://uri.etsi.org/ngsi-ld/")
        .replace("geojson:", "https://purl.org/geojson/vocab#");
    let mut value: Value = serde_json::from_str(&text).unwrap();
    let map = value["@context"].as_object_mut().unwrap();
    map.remove("ngsi-ld");
    map.remove("geojson");
    map.remove("@vocab");
    let definitions = map.clone();
    fn expand(value: &mut Value, definitions: &serde_json::Map<String, Value>) {
        match value {
            Value::String(s) if !s.starts_with('@') && !s.contains(':') => {
                let target = definitions
                    .get(s.as_str())
                    .and_then(|v| v.as_str().or_else(|| v.get("@id").and_then(Value::as_str)));
                *s = target
                    .filter(|v| v.contains(':'))
                    .map(String::from)
                    .unwrap_or_else(|| format!("https://uri.etsi.org/ngsi-ld/default-context/{s}"));
            }
            Value::Array(a) => {
                for item in a {
                    expand(item, definitions);
                }
            }
            Value::Object(o) => {
                for item in o.values_mut() {
                    expand(item, definitions);
                }
            }
            _ => {}
        }
    }
    expand(&mut value, &definitions);
    value
}
fn with_core(context: Value) -> Value {
    let mut contexts = match context {
        Value::Array(values) => values,
        Value::Null => Vec::new(),
        other => vec![other],
    };
    contexts.retain(|v| !v.as_str().is_some_and(crate::is_etsi_core_context));
    contexts.push(json!(CORE));
    Value::Array(contexts)
}
impl Session<'_> {
    async fn process_context(
        &self,
        context: Value,
    ) -> Result<Arc<ProcessedOwned<IriBuf, json_ld::BlankIdBuf>>, String> {
        let key = context.to_string();
        if key.len() > 256 * 1024 {
            return Err("Context exceeds 256 KiB".into());
        }
        {
            let mut cache = self
                .processor
                .processed
                .lock()
                .map_err(|_| "Processed context cache unavailable")?;
            if let Some((at, processed)) = cache.get(&key) {
                if at.elapsed() < Duration::from_secs(3600) {
                    return Ok(processed.clone());
                }
            }
        }
        let syntax = json_ld::syntax::Value::parse_str(&json!({"@context":context}).to_string())
            .map_err(|e| e.to_string())?
            .0;
        let context = syntax.into_ld_context().map_err(|e| e.to_string())?;
        let processed = Arc::new(
            context
                .process(&mut (), self, None)
                .await
                .map_err(|e| e.to_string())?
                .into_owned(),
        );
        self.processor
            .processed
            .lock()
            .map_err(|_| "Processed context cache unavailable")?
            .put(key, (Instant::now(), processed.clone()));
        Ok(processed)
    }
}
impl Loader for Session<'_> {
    async fn load(&self, url: &Iri) -> Result<Document, LoadError> {
        let name = url.as_str();
        let built_in = if name == CANONICAL {
            Some(canonical())
        } else if name == OUTPUT {
            Some(json!({"@context":with_core(self.output.clone())}))
        } else if name == "https://w3id.org/security/data-integrity/v2" {
            Some(
                serde_json::from_str(include_str!("../contexts/data-integrity-v2.jsonld")).unwrap(),
            )
        } else if crate::is_etsi_core_context(name) {
            Some(core())
        } else {
            None
        };
        if let Some(value) = built_in {
            return document(Some(url.to_owned()), &value).map_err(|e| failure(url, e));
        }
        if self
            .loads
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            >= 16
        {
            return Err(failure(url, "Too many remote contexts"));
        }
        {
            let mut cache = self
                .processor
                .cache
                .lock()
                .map_err(|_| failure(url, "Context cache unavailable"))?;
            if let Some((loaded, value)) = cache.get(name) {
                if loaded.elapsed() < Duration::from_secs(3600) {
                    return Ok(value.clone());
                }
            }
        }
        let response = self
            .processor
            .client
            .request(reqwest::Method::GET, name)
            .map_err(|e| failure(url, e))?
            .send()
            .await
            .map_err(|e| failure(url, e.to_string()))?
            .error_for_status()
            .map_err(|e| failure(url, e.to_string()))?;
        let final_url =
            IriBuf::new(response.url().to_string()).map_err(|e| failure(url, e.to_string()))?;
        let value = bounded_json(response, 256 * 1024)
            .await
            .map_err(|e| failure(url, e))?;
        let doc = document(Some(final_url), &value).map_err(|e| failure(url, e))?;
        self.processor
            .cache
            .lock()
            .map_err(|_| failure(url, "Context cache unavailable"))?
            .put(name.into(), (Instant::now(), doc.clone()));
        Ok(doc)
    }
}
impl Default for Processor {
    fn default() -> Self {
        Self::new(128)
    }
}
impl Processor {
    pub fn new(capacity: usize) -> Self {
        Self::with_options(capacity, 16, OutboundPolicy::default())
    }
    pub fn with_options(capacity: usize, concurrency: usize, policy: OutboundPolicy) -> Self {
        Self {
            client: OutboundClient::for_contexts(policy).expect("HTTP transport"),
            cache: Arc::new(Mutex::new(LruCache::new(
                NonZeroUsize::new(capacity.clamp(1, 1024)).unwrap(),
            ))),
            slots: Arc::new(tokio::sync::Semaphore::new(concurrency.clamp(1, 256))),
            terms: Arc::new(Mutex::new(LruCache::new(NonZeroUsize::new(2048).unwrap()))),
            processed: Arc::new(Mutex::new(LruCache::new(
                NonZeroUsize::new(capacity.clamp(1, 1024)).unwrap(),
            ))),
        }
    }
    pub async fn normalize(&self, mut value: Value, context: Value) -> Result<Value, String> {
        value
            .as_object_mut()
            .ok_or("JSON-LD document must be an object")?
            .insert("@context".into(), with_core(context));
        self.transform(value, Value::Null, CANONICAL).await
    }
    pub async fn compact(&self, mut value: Value, context: Value) -> Result<Value, String> {
        let mut input_context = canonical()["@context"].clone();
        input_context["@vocab"] = json!("https://uri.etsi.org/ngsi-ld/default-context/");
        value
            .as_object_mut()
            .ok_or("JSON-LD document must be an object")?
            .insert("@context".into(), input_context);
        self.transform(value, context, OUTPUT).await
    }
    pub async fn expand_term(&self, term: &str, context: Value) -> Result<String, String> {
        let key = (term.to_owned(), context.to_string());
        if key.1.len() > 4096 {
            let result = self
                .normalize(json!({"id":"urn:athena:term","type":term}), context)
                .await?;
            return result
                .get("type")
                .and_then(Value::as_str)
                .map(String::from)
                .ok_or_else(|| "Invalid vocabulary term".into());
        }
        if let Some((at, value)) = self
            .terms
            .lock()
            .map_err(|_| "Term cache unavailable")?
            .get(&key)
            .cloned()
        {
            if at.elapsed() < Duration::from_secs(3600) {
                return Ok(value);
            }
        }
        // Using @type invokes vocabulary expansion without interpreting the term as a document URL.
        let result = self
            .normalize(json!({"id":"urn:athena:term","type":term}), context)
            .await?;
        let value = result
            .get("type")
            .and_then(Value::as_str)
            .map(String::from)
            .ok_or_else(|| "Invalid vocabulary term".to_owned())?;
        self.terms
            .lock()
            .map_err(|_| "Term cache unavailable")?
            .put(key, (Instant::now(), value.clone()));
        Ok(value)
    }
    async fn transform(&self, value: Value, output: Value, target: &str) -> Result<Value, String> {
        let permit = self
            .slots
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| "JSON-LD processor closed")?;
        let processor = self.clone();
        let target = target.to_owned();
        // The processor uses non-Send futures internally. Keep CPU work off the
        // Tokio I/O workers and retain the permit even if the caller disconnects.
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            tokio::runtime::Handle::current().block_on(async move {
                let loader = Session {
                    processor: &processor,
                    output,
                    loads: std::sync::atomic::AtomicUsize::new(0),
                };
                let mut value = value;
                let input_context = value
                    .as_object_mut()
                    .ok_or("Invalid document")?
                    .remove("@context")
                    .ok_or("Missing processing context")?;
                let active = loader.process_context(input_context).await?;
                let output = loader
                    .process_context(if target == CANONICAL {
                        canonical()["@context"].clone()
                    } else {
                        with_core(loader.output.clone())
                    })
                    .await?;
                let input = json_ld::syntax::Value::parse_str(&value.to_string())
                    .map_err(|e| e.to_string())?
                    .0;
                let expanded = input
                    .expand_full(
                        &mut (),
                        active.processed().clone(),
                        None,
                        &loader,
                        Default::default(),
                        (),
                    )
                    .await
                    .map_err(|e| e.to_string())?;
                let compacted = Compact::compact(&expanded, output.as_ref().as_ref(), &loader)
                    .await
                    .map_err(|e| e.to_string())?;
                let mut result: Value = serde_json::from_str(&compacted.pretty_print().to_string())
                    .map_err(|e| e.to_string())?;
                if let Some(map) = result.as_object_mut() {
                    map.remove("@context");
                }
                Ok(result)
            })
        })
        .await
        .map_err(|e| e.to_string())?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn json_payload_is_opaque_and_core_overrides_custom_terms() {
        let processor = Processor::default();
        let value = json!({"id":"urn:sensor:opaque","type":"Sensor","payload":{"type":"JsonProperty","json":{"id":"literal","type":"arbitrary","@context":"not-a-context"}},"members":{"type":"ListRelationship","objectList":["urn:b","urn:a"]}});
        let context = json!({"Property":"https://example.org/wrong"});
        let normalized = processor
            .normalize(value.clone(), context.clone())
            .await
            .unwrap();
        assert_eq!(
            normalized["https://uri.etsi.org/ngsi-ld/default-context/payload"]["json"],
            value["payload"]["json"]
        );
        let result = processor.compact(normalized, context).await.unwrap();
        assert_eq!(result, value);
        let input = json!({"id":"urn:sensor:protected","type":"Sensor","reading":{"type":"Property","value":1}});
        assert_eq!(
            processor
                .normalize(input, json!({"Property":"https://example.org/wrong"}))
                .await
                .unwrap()["https://uri.etsi.org/ngsi-ld/default-context/reading"]["type"],
            "Property"
        );
    }
    #[tokio::test]
    async fn canonical_context_round_trip() {
        let processor = Processor::default();
        let context = json!({"temperature":"https://example.org/temperature","Sensor":"https://example.org/Sensor"});
        let value = json!({"id":"urn:sensor:1","type":"Sensor","temperature":{"type":"Property","value":12},"location":{"type":"GeoProperty","value":{"type":"Point","coordinates":[12.5,41.9]}}});
        let canonical = processor
            .normalize(value.clone(), context.clone())
            .await
            .unwrap();
        assert_eq!(
            canonical["https://example.org/temperature"]["value"], 12,
            "{canonical}"
        );
        assert_eq!(canonical["type"], "https://example.org/Sensor");
        assert_eq!(
            canonical["location"]["value"]["coordinates"],
            json!([12.5, 41.9])
        );
        let result = processor.compact(canonical, context).await.unwrap();
        assert_eq!(result, value);
    }
}
