use std::{
    collections::{BTreeMap, HashSet},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use athena_http::{OutboundClient, OutboundPolicy};
use athena_model::Entity;
use athena_storage::{CsourceRepository, EntityQueryParams, EntityRepository};

const PAGE_SIZE: i64 = 1000;
const MAX_ENTITIES: usize = 10_000;
const MAX_SOURCES: usize = 256;

pub struct FederatedResult {
    pub entities: Vec<Entity>,
    pub count: i64,
}

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
        split_entities: bool,
    ) -> Result<FederatedResult, String> {
        let mut sources = csource_repo
            .get_matching_csources(
                params.type_.as_deref(),
                params.id.as_deref(),
                if split_entities {
                    None
                } else {
                    params.attrs.as_deref()
                },
            )
            .await
            .map_err(|e| format!("Context source discovery failed: {e}"))?;
        // Deterministic precedence: local, then registration ID, then remote page order.
        sources.sort_by(|a, b| a.id.cmp(&b.id));
        if sources.len() > MAX_SOURCES {
            return Err(
                "Federated query exceeds 256 matching registrations; narrow the query".into(),
            );
        }
        if sources.is_empty() {
            let count = entity_repo
                .count_entities(params)
                .await
                .map_err(|e| e.to_string())?;
            let entities = entity_repo
                .query_entities(params)
                .await
                .map_err(|e| e.to_string())?;
            return Ok(FederatedResult { entities, count });
        }
        let budget = Arc::new(AtomicUsize::new(0));
        let mut merged = BTreeMap::new();
        let mut local_params = params.clone();
        if split_entities {
            local_params.q = None;
            local_params.geo_q = None;
            local_params.attrs = None;
        }
        local_params.limit = Some(PAGE_SIZE);
        local_params.offset = Some(0);
        loop {
            let page = entity_repo
                .query_entities(&local_params)
                .await
                .map_err(|e| format!("Local query failed: {e}"))?;
            let size = page.len();
            for entity in page {
                // Legacy local keys use the default vocabulary; canonicalize them
                // before merging with fully qualified remote attribute names.
                let value = self
                    .processor
                    .normalize(
                        entity.to_normalized(true),
                        serde_json::json!(athena_model::ETSI_CORE_CONTEXT_URL),
                    )
                    .await?;
                let entity = Entity::from_json(value).map_err(|e| e.to_string())?;
                reserve_bytes(&budget, &entity)?;
                merged.insert(entity.id.clone(), entity);
            }
            if merged.len() > MAX_ENTITIES {
                return Err("Federated query exceeds 10000 entities; narrow the query".into());
            }
            if size < PAGE_SIZE as usize {
                break;
            }
            local_params.offset = Some(local_params.offset.unwrap() + size as i64);
            if local_params.offset.unwrap() > MAX_ENTITIES as i64 {
                return Err(
                    "Local result changed during federated pagination; retry the query".into(),
                );
            }
        }
        let permits = Arc::new(tokio::sync::Semaphore::new(8));
        let mut tasks = tokio::task::JoinSet::new();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        for (rank, source) in sources.into_iter().enumerate() {
            let budget = budget.clone();
            let client = self.client.clone();
            let processor = self.processor.clone();
            let permits = permits.clone();
            // Inputs have already been expanded using the request context by middleware.
            // Always ask for normalized data; apply presentation options at the outer edge.
            let query: Vec<(String, String)> =
                url::form_urlencoded::parse(raw_query_string.unwrap_or("").as_bytes())
                    .into_owned()
                    .filter(|(key, _)| {
                        !matches!(
                            key.as_str(),
                            "limit"
                                | "offset"
                                | "local"
                                | "count"
                                | "options"
                                | "format"
                                | "splitEntities"
                        ) && !(split_entities
                            && matches!(
                                key.as_str(),
                                "q" | "georel"
                                    | "geometry"
                                    | "coordinates"
                                    | "geoproperty"
                                    | "attrs"
                            ))
                    })
                    .collect();
            tasks.spawn(async move {
                let result = tokio::time::timeout_at(deadline, async {
                    let _permit = permits
                        .acquire_owned()
                        .await
                        .map_err(|_| "Federation stopped".to_owned())?;
                    remote_pages(client, processor, &source.endpoint, query, budget).await
                })
                .await
                .map_err(|_| format!("Context source '{}' timed out", source.id))?
                .map_err(|error| format!("Context source '{}': {error}", source.id));
                Ok::<_, String>((rank, result?))
            });
        }
        // JoinSet aborts outstanding work if the incoming request is cancelled.
        let mut first_error = None;
        let mut remote_results = BTreeMap::new();
        while let Some(task) = tasks.join_next().await {
            match task.map_err(|e| e.to_string()).and_then(|r| r) {
                Ok((rank, entities)) => {
                    remote_results.insert(rank, entities);
                }
                Err(error) => {
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                    tasks.abort_all();
                }
            }
        }
        for entities in remote_results.into_values() {
            for remote in entities {
                if let Some(existing) = merged.get_mut(&remote.id) {
                    merge_entity(existing, remote);
                } else {
                    merged.insert(remote.id.clone(), remote);
                }
                if merged.len() > MAX_ENTITIES {
                    return Err("Federated query exceeds 10000 entities; narrow the query".into());
                }
            }
        }
        if let Some(error) = first_error {
            return Err(error);
        }
        let mut entities: Vec<_> = merged.into_values().collect();
        if split_entities {
            entities = entity_repo
                .filter_snapshots(entities, params)
                .await
                .map_err(|e| format!("Merged entity filtering failed: {e}"))?;
        }
        let count = entities.len() as i64;
        entities.sort_by(|a, b| {
            b.modified_at
                .cmp(&a.modified_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        let entities = entities
            .into_iter()
            .skip(params.offset.unwrap_or(0).max(0) as usize)
            .take(params.limit.unwrap_or(20).clamp(0, 1000) as usize)
            .collect();
        Ok(FederatedResult { entities, count })
    }
}

fn merge_entity(existing: &mut Entity, remote: Entity) {
    for kind in remote.types {
        if !existing.types.contains(&kind) {
            existing.types.push(kind);
        }
    }
    for (name, value) in remote.attributes {
        if let Some(previous) = existing.attributes.get_mut(&name) {
            *previous = athena_model::attributes::merge_instances(previous, &value, false);
        } else {
            existing.attributes.insert(name, value);
        }
    }
    if existing.scope.is_none() {
        existing.scope = remote.scope;
    }
}

async fn remote_pages(
    client: OutboundClient,
    processor: Arc<athena_jsonld::Processor>,
    endpoint: &str,
    base_query: Vec<(String, String)>,
    budget: Arc<AtomicUsize>,
) -> Result<Vec<Entity>, String> {
    client.validate(endpoint)?;
    let mut entities = Vec::new();
    let mut seen = HashSet::new();
    let mut offset = 0usize;
    let mut expected_count = None;
    loop {
        let mut query = base_query.clone();
        query.extend([
            ("local".into(), "true".into()),
            ("count".into(), "true".into()),
            ("limit".into(), PAGE_SIZE.to_string()),
            ("offset".into(), offset.to_string()),
            ("options".into(), "sysAttrs".into()),
        ]);
        let qs = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(query)
            .finish();
        let target = format!(
            "{}/ngsi-ld/v1/entities?{qs}",
            endpoint.trim_end_matches('/')
        );
        let response = client
            .request(reqwest::Method::GET, &target)?
            .header("Accept", "application/ld+json")
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if response.status() != reqwest::StatusCode::OK {
            return Err(format!("returned HTTP {}", response.status()));
        }
        let count = response
            .headers()
            .get("NGSILD-Results-Count")
            .map(|v| {
                v.to_str()
                    .ok()
                    .and_then(|s| s.parse::<usize>().ok())
                    .ok_or("Invalid result count")
            })
            .transpose()?;
        if let Some(count) = count {
            if expected_count.is_some_and(|expected| count != expected) {
                return Err(
                    "Remote result count changed during pagination; retry the query".into(),
                );
            }
            expected_count = Some(count);
        }
        if count.is_some_and(|n| n > MAX_ENTITIES) {
            return Err("More than 10000 remote entities; narrow the query".into());
        }
        let fallback = response
            .headers()
            .get_all("link")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(athena_model::LinkHeader::parse_list)
            .find(|v| v.rel == athena_model::JSON_LD_CONTEXT_REL)
            .map(|v| v.uri)
            .unwrap_or_else(|| athena_model::ETSI_CORE_CONTEXT_URL.into());
        let body = athena_http::bounded_json(response, 8 * 1024 * 1024).await?;
        let page = body
            .as_array()
            .ok_or("Remote query did not return an entity array")?;
        if page.is_empty() {
            if expected_count.is_some_and(|n| n != offset) {
                return Err("Remote pagination ended before the advertised count".into());
            }
            break;
        }
        if page.len() > PAGE_SIZE as usize || offset + page.len() > MAX_ENTITIES {
            return Err("Remote query exceeded its result budget".into());
        }
        if expected_count.is_some_and(|count| offset + page.len() > count) {
            return Err("Remote page exceeds its advertised result count".into());
        }
        for item in page {
            let context = item
                .get("@context")
                .cloned()
                .unwrap_or_else(|| serde_json::json!(fallback));
            let normalized = processor.normalize(item.clone(), context).await?;
            let entity = Entity::from_json(normalized).map_err(|e| e.to_string())?;
            if !seen.insert(entity.id.clone()) {
                return Err(
                    "Remote pagination repeated an entity; retry against a stable result set"
                        .into(),
                );
            }
            reserve_bytes(&budget, &entity)?;
            entities.push(entity);
        }
        offset += page.len();
        if expected_count.is_some_and(|n| offset >= n) {
            break;
        }
        // Without a count, continue until empty; providers may cap the requested page size.
    }
    Ok(entities)
}

fn reserve_bytes(budget: &AtomicUsize, entity: &Entity) -> Result<(), String> {
    let bytes = serde_json::to_vec(entity).map_err(|e| e.to_string())?.len();
    budget
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
            used.checked_add(bytes)
                .filter(|total| *total <= 32 * 1024 * 1024)
        })
        .map(|_| ())
        .map_err(|_| {
            "Federated query exceeds the combined 32 MiB response budget; narrow the query".into()
        })
}
