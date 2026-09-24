use std::sync::Arc;

use athena_http::{OutboundClient, OutboundPolicy};
use tracing::{error, warn};

use athena_model::Entity;
use athena_storage::{CsourceRepository, EntityQueryParams, EntityRepository};

pub struct FederationService {
    client: OutboundClient,
    processor: Arc<athena_jsonld::Processor>,
}

impl Default for FederationService {
    fn default() -> Self {
        Self::new()
    }
}

impl FederationService {
    pub fn new() -> Self {
        Self::with_policy(
            OutboundPolicy::default(),
            Arc::new(athena_jsonld::Processor::default()),
        )
    }
    pub fn with_policy(policy: OutboundPolicy, processor: Arc<athena_jsonld::Processor>) -> Self {
        Self {
            client: OutboundClient::new(policy).expect("HTTP client initialization"),
            processor,
        }
    }

    pub async fn query_federated(
        &self,
        entity_repo: &Arc<dyn EntityRepository>,
        csource_repo: &Arc<dyn CsourceRepository>,
        params: &EntityQueryParams,
        raw_query_string: Option<&str>,
    ) -> Result<Vec<Entity>, String> {
        // 1. Fetch local entities
        let mut local_entities = entity_repo
            .query_entities(params)
            .await
            .map_err(|e| format!("Local query failed: {e}"))?;

        // 2. Discover matching registered context sources
        let sources = match csource_repo
            .get_matching_csources(
                params.type_.as_deref(),
                params.id.as_deref(),
                params.attrs.as_deref(),
            )
            .await
        {
            Ok(s) => s,
            Err(e) => {
                warn!("Failed to fetch matching context sources: {e}");
                return Ok(local_entities);
            }
        };

        if sources.is_empty() {
            return Ok(local_entities);
        }

        // 3. Dispatch concurrent requests to remote context sources
        let mut tasks = Vec::new();
        let permits = Arc::new(tokio::sync::Semaphore::new(8));
        for source in sources.into_iter().take(256) {
            let permits = permits.clone();
            let endpoint = source.endpoint.clone();
            let mut query: Vec<(String, String)> =
                url::form_urlencoded::parse(raw_query_string.unwrap_or("").as_bytes())
                    .into_owned()
                    .filter(|(k, _)| k != "local")
                    .collect();
            query.push(("local".into(), "true".into()));
            let qs = url::form_urlencoded::Serializer::new(String::new())
                .extend_pairs(query)
                .finish();
            let client = self.client.clone();
            let processor = self.processor.clone();

            tasks.push(tokio::spawn(async move {
                let Ok(_permit) = permits.acquire_owned().await else {
                    return Vec::new();
                };
                // SSRF Protection verification
                if let Err(ssrf_err) = client.validate(&endpoint) {
                    error!("Skipping unsafe federated source '{endpoint}': {ssrf_err}");
                    return Vec::new();
                }

                let target_url = if endpoint.ends_with('/') {
                    format!("{endpoint}ngsi-ld/v1/entities?{qs}")
                } else {
                    format!("{endpoint}/ngsi-ld/v1/entities?{qs}")
                };

                let request = match client.request(reqwest::Method::GET, &target_url) {
                    Ok(request) => request,
                    Err(_) => return Vec::new(),
                };
                match request.header("Accept", "application/ld+json").send().await {
                    Ok(res) if res.status().is_success() => {
                        let fallback_context = res
                            .headers()
                            .get("link")
                            .and_then(|v| v.to_str().ok())
                            .and_then(athena_model::LinkHeader::parse)
                            .map(|v| v.uri)
                            .unwrap_or_else(|| athena_model::ETSI_CORE_CONTEXT_URL.into());
                        if let Ok(body) = athena_http::bounded_json(res, 8 * 1024 * 1024).await {
                            if let Some(arr) = body.as_array() {
                                let mut entities = Vec::new();
                                for item in arr {
                                    let context = item
                                        .get("@context")
                                        .cloned()
                                        .unwrap_or_else(|| serde_json::json!(fallback_context));
                                    if let Ok(value) =
                                        processor.normalize(item.clone(), context).await
                                    {
                                        if let Ok(entity) = Entity::from_json(value) {
                                            entities.push(entity);
                                        }
                                    }
                                }
                                return entities;
                            }
                        }
                    }
                    Ok(res) => {
                        warn!(
                            "Federated source '{endpoint}' returned HTTP {}",
                            res.status()
                        );
                    }
                    Err(e) => {
                        warn!("Federated query to '{endpoint}' failed: {e}");
                    }
                }
                Vec::new()
            }));
        }

        // 4. Merge results and deduplicate per ETSI CIM 009 clause 5.12
        for task in tasks {
            if let Ok(remote_entities) = task.await {
                for remote in remote_entities {
                    if let Some(existing) = local_entities.iter_mut().find(|e| e.id == remote.id) {
                        // Merge attributes from remote into existing entity if not present locally
                        for (k, v) in remote.attributes {
                            existing.attributes.entry(k).or_insert(v);
                        }
                    } else {
                        local_entities.push(remote);
                    }
                }
            }
        }

        Ok(local_entities)
    }
}
