use crate::repository::{StorageError, TemporalRepository};
use async_trait::async_trait;
use athena_model::{AggrMethod, ProblemDetails, TemporalQuery, TimeProperty, TimeRel};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use sqlx::{PgPool, Postgres, QueryBuilder, Row};
use std::collections::BTreeMap;

pub struct PgTemporalStore {
    pool: PgPool,
}

fn bad(message: impl Into<String>) -> StorageError {
    StorageError::Problem(ProblemDetails::bad_request_data(message))
}

fn timestamp(value: Option<&Value>) -> Result<Option<DateTime<Utc>>, StorageError> {
    value
        .map(|v| {
            v.as_str()
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .map(|t| t.with_timezone(&Utc))
                .ok_or_else(|| bad("Invalid temporal timestamp"))
        })
        .transpose()
}

fn time_column(query: &TemporalQuery) -> &'static str {
    match query.timeproperty {
        TimeProperty::ObservedAt => "observed_at",
        TimeProperty::CreatedAt => "created_at",
        TimeProperty::ModifiedAt => "modified_at",
    }
}

fn validate_query(query: &TemporalQuery) -> Result<(), StorageError> {
    if query.timerel == TimeRel::Between && query.end_time_at.is_none() {
        return Err(bad("endTimeAt is required for timerel=between"));
    }
    if query
        .end_time_at
        .is_some_and(|end| query.timerel == TimeRel::Between && query.time_at > end)
    {
        return Err(bad("timeAt must not be after endTimeAt"));
    }
    if query.last_n.is_some_and(|n| n == 0 || n > 10_000) {
        return Err(bad("lastN must be between 1 and 10000"));
    }
    Ok(())
}

fn filters(
    builder: &mut QueryBuilder<'_, Postgres>,
    query: &TemporalQuery,
    attrs: Option<&[String]>,
) {
    let column = time_column(query);
    builder.push(column);
    match query.timerel {
        TimeRel::Before => {
            builder.push(" < ").push_bind(query.time_at);
        }
        TimeRel::After => {
            builder.push(" > ").push_bind(query.time_at);
        }
        TimeRel::Between => {
            builder
                .push(" >= ")
                .push_bind(query.time_at)
                .push(" AND ")
                .push(column)
                .push(" <= ")
                .push_bind(query.end_time_at);
        }
    }
    if let Some(attrs) = attrs.filter(|a| !a.is_empty()) {
        builder
            .push(" AND attribute_id = ANY(")
            .push_bind(attrs.to_vec())
            .push(")");
    }
}

impl PgTemporalStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    async fn read_many(
        &self,
        ids: &[String],
        attrs: Option<&[String]>,
        query: &TemporalQuery,
    ) -> Result<Vec<Value>, StorageError> {
        validate_query(query)?;
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let catalog =
            sqlx::query("SELECT id, types FROM temporal_entities WHERE id = ANY($1) ORDER BY id")
                .bind(ids)
                .fetch_all(&self.pool)
                .await?;
        let mut entities = BTreeMap::new();
        for row in catalog {
            let id: String = row.try_get("id")?;
            let types: Vec<String> = row.try_get("types")?;
            let type_value = if types.len() == 1 {
                json!(types[0])
            } else {
                json!(types)
            };
            entities.insert(id.clone(), json!({"id": id, "type": type_value}));
        }
        if query.aggr_method.is_some() || !query.aggr_methods.is_empty() {
            let methods = if query.aggr_methods.is_empty() {
                query.aggr_method.into_iter().collect()
            } else {
                query.aggr_methods.clone()
            };
            if methods.len() > 8 {
                return Err(bad("Too many aggregation methods"));
            }
            for method in methods {
                let mut current = query.clone();
                current.aggr_method = Some(method);
                entities = self.aggregate(entities, ids, attrs, &current).await?;
            }
            return Ok(entities.into_values().collect());
        }
        let column = time_column(query);
        let mut builder = QueryBuilder::<Postgres>::new(
            format!("WITH ranked AS (SELECT entity_id, attribute_id, instance, {column} AS selected_time, instance_id, ROW_NUMBER() OVER (PARTITION BY entity_id, attribute_id, dataset_id ORDER BY {column} DESC, instance_id) AS rn FROM entity_temporal WHERE entity_id = ANY("));
        builder.push_bind(ids.to_vec()).push(") AND ");
        filters(&mut builder, query, attrs);
        builder.push(") SELECT entity_id, attribute_id, instance FROM ranked");
        if let Some(n) = query.last_n {
            builder.push(" WHERE rn <= ").push_bind(n as i64);
        }
        builder
            .push(" ORDER BY entity_id, attribute_id, selected_time DESC, instance_id LIMIT 10001");
        let rows = builder.build().fetch_all(&self.pool).await?;
        if rows.len() > 10_000 {
            return Err(bad(
                "Temporal result exceeds 10000 instances; narrow the interval or attributes",
            ));
        }
        for row in rows {
            let id: String = row.try_get("entity_id")?;
            let attr: String = row.try_get("attribute_id")?;
            let instance: Value = row.try_get("instance")?;
            if let Some(entity) = entities.get_mut(&id) {
                let values = entity
                    .as_object_mut()
                    .ok_or_else(|| bad("Invalid temporal entity"))?
                    .entry(attr)
                    .or_insert_with(|| json!([]));
                if let Some(values) = values.as_array_mut() {
                    values.push(instance);
                }
            }
        }
        Ok(entities.into_values().collect())
    }

    async fn aggregate(
        &self,
        mut entities: BTreeMap<String, Value>,
        ids: &[String],
        attrs: Option<&[String]>,
        query: &TemporalQuery,
    ) -> Result<BTreeMap<String, Value>, StorageError> {
        let method = query
            .aggr_method
            .ok_or_else(|| bad("Missing aggregation method"))?;
        let (name, expression) = match method {
            AggrMethod::Avg => ("avg", "to_jsonb(AVG(aggregate_numeric))"),
            AggrMethod::Min => ("min", "COALESCE(to_jsonb(MIN(aggregate_numeric)), to_jsonb(MIN(value_text)))"),
            AggrMethod::Max => ("max", "COALESCE(to_jsonb(MAX(aggregate_numeric)), to_jsonb(MAX(value_text)))"),
            AggrMethod::Sum => ("sum", "to_jsonb(SUM(aggregate_numeric))"),
            AggrMethod::TotalCount => ("totalCount", "to_jsonb(COUNT(*))"),
            AggrMethod::DistinctCount => ("distinctCount", "to_jsonb(COUNT(DISTINCT COALESCE(instance->'value', instance->'object', instance->'languageMap', instance->'json', instance->'valueList', instance->'objectList')))"),
            AggrMethod::Stddev => ("stddev", "to_jsonb(STDDEV_POP(CASE WHEN jsonb_typeof(value_json)!='array' THEN aggregate_numeric END))"),
            AggrMethod::Sumsq => ("sumsq", "to_jsonb(SUM(CASE WHEN jsonb_typeof(value_json)!='array' THEN aggregate_numeric * aggregate_numeric END))"),
        };
        let seconds = match query.aggr_period_duration.as_deref() {
            None => 0,
            Some(period) => parse_fixed_duration(period)?,
        };
        let column = time_column(query);
        let mut builder = QueryBuilder::<Postgres>::new("WITH selected AS (SELECT *, CASE jsonb_typeof(value_json) WHEN 'number' THEN value_numeric WHEN 'boolean' THEN CASE WHEN value_json='true'::jsonb THEN 1 ELSE 0 END WHEN 'array' THEN jsonb_array_length(value_json) END AS aggregate_numeric, ");
        if seconds == 0 {
            builder.push("NULL::timestamptz");
        } else {
            builder
                .push("date_bin(make_interval(secs => ")
                .push_bind(seconds as f64)
                .push("), ")
                .push(column)
                .push(", ")
                .push_bind(query.time_at)
                .push(")");
        }
        builder
            .push(" AS bucket FROM entity_temporal WHERE entity_id = ANY(")
            .push_bind(ids.to_vec())
            .push(") AND ");
        filters(&mut builder, query, attrs);
        builder.push(format!(") SELECT entity_id, attribute_id, attribute_type, dataset_id, {expression} AS result, COALESCE(bucket, MIN({column})) AS start_time, "));
        if seconds == 0 {
            builder.push(format!("MAX({column})"));
        } else {
            builder
                .push("bucket + make_interval(secs => ")
                .push_bind(seconds as f64)
                .push(")");
        }
        builder.push(" AS end_time FROM selected GROUP BY entity_id, attribute_id, attribute_type, dataset_id, bucket ORDER BY entity_id, attribute_id, bucket LIMIT 10001");
        let rows = builder.build().fetch_all(&self.pool).await?;
        if rows.len() > 10_000 {
            return Err(bad("Too many aggregation buckets"));
        }
        let mut groups: BTreeMap<(String, String, Option<String>), Value> = BTreeMap::new();
        for row in rows {
            let id: String = row.try_get("entity_id")?;
            let attr: String = row.try_get("attribute_id")?;
            let dataset: Option<String> = row.try_get("dataset_id")?;
            let kind: String = row.try_get("attribute_type")?;
            let result: Option<Value> = row.try_get("result")?;
            let start: DateTime<Utc> = row.try_get("start_time")?;
            let end: DateTime<Utc> = row.try_get("end_time")?;
            let group = groups
                .entry((id, attr, dataset.clone()))
                .or_insert_with(|| {
                    let mut value = json!({"type": kind, name: []});
                    if let Some(dataset) = dataset {
                        value["datasetId"] = json!(dataset);
                    }
                    value
                });
            if let Some(values) = group[name].as_array_mut() {
                values.push(json!([result, start, end]));
            }
        }
        for ((id, attr, dataset), group) in groups {
            if let Some(entity) = entities.get_mut(&id) {
                if entity.get(&attr).is_none() {
                    entity[&attr] = group;
                    continue;
                }
                let old = &mut entity[&attr];
                if old.is_object()
                    && old.get("datasetId").and_then(Value::as_str) == dataset.as_deref()
                {
                    old.as_object_mut()
                        .unwrap()
                        .extend(group.as_object().unwrap().clone());
                } else if let Some(values) = old.as_array_mut() {
                    if let Some(existing) = values
                        .iter_mut()
                        .find(|v| v.get("datasetId").and_then(Value::as_str) == dataset.as_deref())
                    {
                        existing
                            .as_object_mut()
                            .unwrap()
                            .extend(group.as_object().unwrap().clone());
                    } else {
                        values.push(group);
                    }
                } else {
                    let previous = old.take();
                    *old = json!([previous, group]);
                }
            }
        }
        Ok(entities)
    }
}

fn parse_fixed_duration(period: &str) -> Result<i64, StorageError> {
    // Calendar periods have variable lengths and require calendar bucketing;
    // reject them explicitly instead of silently approximating months/years.
    let mut seconds = 0i64;
    let mut digits = String::new();
    let mut in_time = false;
    let mut elements = 0;
    if !period.starts_with('P') {
        return Err(bad("Invalid ISO 8601 duration"));
    }
    for c in period[1..].chars() {
        if c == 'T' && !in_time && digits.is_empty() {
            in_time = true;
            continue;
        }
        if c.is_ascii_digit() {
            digits.push(c);
            continue;
        }
        let value: i64 = digits
            .parse()
            .map_err(|_| bad("Invalid duration component"))?;
        digits.clear();
        let multiplier =
            match (c, in_time) {
                ('W', false) => 604800,
                ('D', false) => 86400,
                ('H', true) => 3600,
                ('M', true) => 60,
                ('S', true) => 1,
                _ => return Err(bad(
                    "Only fixed-length weeks/days/hours/minutes/seconds are currently supported",
                )),
            };
        seconds = seconds
            .checked_add(
                value
                    .checked_mul(multiplier)
                    .ok_or_else(|| bad("Duration overflow"))?,
            )
            .ok_or_else(|| bad("Duration overflow"))?;
        elements += 1;
    }
    if !digits.is_empty() || elements == 0 {
        return Err(bad("Invalid duration"));
    }
    Ok(seconds)
}

#[async_trait]
impl TemporalRepository for PgTemporalStore {
    async fn delete_temporal(
        &self,
        id: &str,
        attribute: Option<&str>,
        dataset: Option<&str>,
        instance: Option<&str>,
        delete_all: bool,
    ) -> Result<(), StorageError> {
        let mut tx = self.pool.begin().await?;
        let exists = sqlx::query("SELECT id FROM temporal_entities WHERE id=$1 FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?;
        if exists.is_none() {
            return Err(StorageError::EntityNotFound(id.into()));
        }
        let result = if let Some(attribute) = attribute {
            sqlx::query("DELETE FROM entity_temporal WHERE entity_id=$1 AND attribute_id=$2 AND ($3::boolean OR dataset_id IS NOT DISTINCT FROM $4::text OR $5::text IS NOT NULL) AND ($5::text IS NULL OR instance_id=$5)")
                .bind(id).bind(attribute).bind(delete_all).bind(dataset).bind(instance).execute(&mut *tx).await?
        } else {
            sqlx::query("DELETE FROM entity_temporal WHERE entity_id=$1")
                .bind(id)
                .execute(&mut *tx)
                .await?
        };
        if attribute.is_some() && result.rows_affected() == 0 {
            return Err(StorageError::EntityNotFound(format!(
                "{id} temporal attribute/instance"
            )));
        }
        if attribute.is_none() {
            sqlx::query("DELETE FROM temporal_entities WHERE id=$1")
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    async fn update_temporal_instance(
        &self,
        id: &str,
        attribute: &str,
        iid: &str,
        patch: &Value,
    ) -> Result<(), StorageError> {
        let patch = patch
            .as_object()
            .ok_or_else(|| bad("Instance patch must be an object"))?;
        let mut tx = self.pool.begin().await?;
        let existing:Option<Value>=sqlx::query_scalar("SELECT instance FROM entity_temporal WHERE entity_id=$1 AND attribute_id=$2 AND instance_id=$3 FOR UPDATE")
            .bind(id).bind(attribute).bind(iid).fetch_optional(&mut *tx).await?;
        let mut value = existing.ok_or_else(|| StorageError::EntityNotFound(iid.into()))?;
        for (key, incoming) in patch {
            if key == "@context" {
                continue;
            }
            if matches!(
                key.as_str(),
                "instanceId" | "datasetId" | "createdAt" | "type"
            ) && value.get(key) != Some(incoming)
            {
                return Err(bad("Instance identity, type and createdAt cannot change"));
            }
            value[key] = incoming.clone();
        }
        value["modifiedAt"] = json!(Utc::now());
        athena_model::attributes::validate_attribute(attribute, &value, 0)
            .map_err(|e| bad(e.to_string()))?;
        sqlx::query("UPDATE entity_temporal SET instance=$4,observed_at=$5,modified_at=clock_timestamp(),value_json=$6,value_numeric=$7,value_text=$8 WHERE entity_id=$1 AND attribute_id=$2 AND instance_id=$3")
            .bind(id).bind(attribute).bind(iid).bind(&value).bind(timestamp(value.get("observedAt"))?)
            .bind(value.get("value")).bind(value.get("value").and_then(Value::as_f64)).bind(value.get("value").and_then(Value::as_str)).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn create_temporal_entity(
        &self,
        id: &str,
        entity_type: &str,
        attributes: &Value,
    ) -> Result<(), StorageError> {
        if !athena_model::attributes::valid_uri(id) || entity_type.is_empty() {
            return Err(bad("Invalid temporal entity identity"));
        }
        let object = attributes
            .as_object()
            .ok_or_else(|| bad("Temporal attributes must be an object"))?;
        let now = Utc::now();
        let mut prepared = Vec::new();
        for (name, values) in object {
            if matches!(
                name.as_str(),
                "id" | "type" | "@context" | "createdAt" | "modifiedAt" | "scope"
            ) {
                continue;
            }
            let instances = values
                .as_array()
                .cloned()
                .unwrap_or_else(|| vec![values.clone()]);
            for mut instance in instances {
                athena_model::attributes::validate_attribute(name, &instance, 0)
                    .map_err(|e| bad(e.to_string()))?;
                let iid = instance
                    .get("instanceId")
                    .and_then(Value::as_str)
                    .map(String::from)
                    .unwrap_or_else(|| format!("urn:ngsi-ld:Instance:{}", uuid::Uuid::new_v4()));
                let observed = timestamp(instance.get("observedAt"))?;
                let created = timestamp(instance.get("createdAt"))?.unwrap_or(now);
                let modified = timestamp(instance.get("modifiedAt"))?.unwrap_or(now);
                instance["instanceId"] = json!(iid);
                instance["createdAt"] = json!(created);
                instance["modifiedAt"] = json!(modified);
                prepared.push((name.clone(), iid, observed, created, modified, instance));
                if prepared.len() > 10_000 {
                    return Err(bad("Too many temporal instances"));
                }
            }
        }
        let mut tx = self.pool.begin().await?;
        sqlx::query("INSERT INTO temporal_entities(id, types) VALUES ($1, $2) ON CONFLICT(id) DO UPDATE SET types = EXCLUDED.types, modified_at = NOW()")
            .bind(id).bind(vec![entity_type]).execute(&mut *tx).await?;
        for chunk in prepared.chunks(500) {
            let mut builder = QueryBuilder::<Postgres>::new("INSERT INTO entity_temporal(entity_id, entity_type, attribute_id, instance_id, observed_at, created_at, modified_at, value_numeric, value_text, value_json, dataset_id, attribute_type, instance) ");
            builder.push_values(
                chunk,
                |mut row, (name, iid, observed, created, modified, instance)| {
                    row.push_bind(id)
                        .push_bind(entity_type)
                        .push_bind(name)
                        .push_bind(iid)
                        .push_bind(*observed)
                        .push_bind(*created)
                        .push_bind(*modified)
                        .push_bind(instance.get("value").and_then(Value::as_f64))
                        .push_bind(instance.get("value").and_then(Value::as_str))
                        .push_bind(instance.get("value").cloned())
                        .push_bind(instance.get("datasetId").and_then(Value::as_str))
                        .push_bind(instance.get("type").and_then(Value::as_str))
                        .push_bind(instance);
                },
            );
            builder.push(" ON CONFLICT(entity_id, attribute_id, instance_id) DO UPDATE SET observed_at=EXCLUDED.observed_at, modified_at=EXCLUDED.modified_at, value_numeric=EXCLUDED.value_numeric, value_text=EXCLUDED.value_text, value_json=EXCLUDED.value_json, dataset_id=EXCLUDED.dataset_id, attribute_type=EXCLUDED.attribute_type, instance=EXCLUDED.instance");
            builder.build().execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    async fn record_temporal_instance(
        &self,
        id: &str,
        entity_type: &str,
        attr: &str,
        observed: DateTime<Utc>,
        value: &Value,
        dataset: Option<&str>,
    ) -> Result<(), StorageError> {
        let mut instance = json!({"type": "Property", "value": value, "observedAt": observed});
        if let Some(dataset) = dataset {
            instance["datasetId"] = json!(dataset);
        }
        self.create_temporal_entity(id, entity_type, &json!({attr: instance}))
            .await
    }

    async fn query_temporal(
        &self,
        id: &str,
        attrs: Option<&[String]>,
        query: &TemporalQuery,
    ) -> Result<Value, StorageError> {
        let mut results = self.read_many(&[id.to_string()], attrs, query).await?;
        results
            .pop()
            .ok_or_else(|| StorageError::EntityNotFound(id.into()))
    }

    async fn query_temporal_entities(
        &self,
        entity_type: Option<&str>,
        attrs: Option<&[String]>,
        query: &TemporalQuery,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<Value>, StorageError> {
        validate_query(query)?;
        if limit > 1000 || offset > i64::MAX as usize {
            return Err(bad("Invalid pagination"));
        }
        let mut builder =
            QueryBuilder::<Postgres>::new("SELECT id FROM temporal_entities e WHERE ");
        if let Some(kind) = entity_type {
            builder
                .push_bind(kind.to_owned())
                .push(" = ANY(e.types) AND ");
        }
        builder.push("EXISTS (SELECT 1 FROM entity_temporal WHERE entity_id=e.id AND ");
        filters(&mut builder, query, attrs);
        builder
            .push(") ORDER BY id LIMIT ")
            .push_bind(limit as i64)
            .push(" OFFSET ")
            .push_bind(offset as i64);
        let ids: Vec<String> = builder.build_query_scalar().fetch_all(&self.pool).await?;
        self.read_many(&ids, attrs, query).await
    }
}
