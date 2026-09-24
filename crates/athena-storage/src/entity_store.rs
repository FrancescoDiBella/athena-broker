use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{PgPool, Row};
use std::collections::BTreeMap;

use crate::repository::{EntityQueryParams, EntityRepository, StorageError};
use athena_model::{BatchOperationResult, Entity, ProblemDetails};
use athena_query::{SqlCompiler, SqlParam};

pub struct PgEntityStore {
    pool: PgPool,
}

impl PgEntityStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn bulk_write_atomic(
        &self,
        entities: &[Entity],
        upsert: bool,
    ) -> Result<BatchOperationResult, StorageError> {
        let mut builder = sqlx::QueryBuilder::<sqlx::Postgres>::new(
            "INSERT INTO entities(id,type,types,attrs,location,scope) ",
        );
        builder.push_values(entities, |mut row, entity| {
            row.push_bind(entity.id.clone())
                .push_bind(entity.type_.clone())
                .push_bind(entity.types.clone())
                .push_bind(serde_json::to_value(&entity.attributes).expect("JSON attributes"));
            row.push("CASE WHEN ")
                .push_bind_unseparated(Self::extract_location_geojson(entity))
                .push_unseparated("::text IS NULL THEN NULL ELSE ST_SetSRID(ST_GeomFromGeoJSON(")
                .push_bind_unseparated(Self::extract_location_geojson(entity))
                .push_unseparated("),4326) END");
            row.push_bind(entity.scope.clone());
        });
        if upsert {
            builder.push(" ON CONFLICT(id) DO UPDATE SET attrs=EXCLUDED.attrs,type=EXCLUDED.type,types=EXCLUDED.types,scope=EXCLUDED.scope,location=EXCLUDED.location");
        } else {
            builder.push(" ON CONFLICT(id) DO NOTHING");
        }
        builder.push(" RETURNING id");
        let ids: Vec<String> = builder.build_query_scalar().fetch_all(&self.pool).await?;
        let mut inserted: std::collections::HashSet<String> = ids.into_iter().collect();
        let mut result = BatchOperationResult::new();
        for entity in entities {
            if inserted.remove(&entity.id) {
                result.add_success(&entity.id);
            } else {
                result.add_error(
                    &entity.id,
                    ProblemDetails::already_exists("Entity already exists"),
                );
            }
        }
        Ok(result)
    }

    fn extract_location_geojson(entity: &Entity) -> Option<String> {
        let location = entity.attributes.get("location")?;
        let location = if let Some(instances) = location.as_array() {
            instances.iter().find(|v| v.get("datasetId").is_none())?
        } else {
            location
        };
        if location.get("type").and_then(Value::as_str) != Some("GeoProperty")
            || location.get("datasetId").is_some()
        {
            return None;
        }
        location.get("value").map(Value::to_string)
    }
}

#[async_trait]
impl EntityRepository for PgEntityStore {
    async fn create_entity(&self, entity: &Entity) -> Result<(), StorageError> {
        let attrs_json = serde_json::to_value(&entity.attributes)?;
        let loc_geojson = Self::extract_location_geojson(entity);

        let res = sqlx::query(
            r#"
            INSERT INTO entities (id, type, types, attrs, location, scope, created_at, modified_at)
            VALUES (
                $1, $2, $3, $4,
                CASE WHEN $5::text IS NOT NULL THEN ST_SetSRID(ST_GeomFromGeoJSON($5), 4326) ELSE NULL END,
                $6, NOW(), NOW()
            )
            "#,
        )
        .bind(&entity.id)
        .bind(&entity.type_)
        .bind(&entity.types)
        .bind(&attrs_json)
        .bind(&loc_geojson)
        .bind(&entity.scope)
        .execute(&self.pool)
        .await;

        match res {
            Ok(_) => Ok(()),
            Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
                Err(StorageError::EntityAlreadyExists(entity.id.clone()))
            }
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    async fn get_entity_by_id(
        &self,
        id: &str,
        attrs: Option<&[String]>,
    ) -> Result<Option<Entity>, StorageError> {
        let row = sqlx::query(
            r#"
            SELECT id, type, types, attrs, scope, created_at, modified_at
            FROM entities
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        let row = match row {
            Some(r) => r,
            None => return Ok(None),
        };

        let entity_id: String = row.try_get("id")?;
        let entity_type: String = row.try_get("type")?;
        let entity_types: Vec<String> = row.try_get("types")?;
        let attrs_val: Value = row.try_get("attrs")?;
        let scope: Option<Vec<String>> = row.try_get("scope")?;
        let created_at: DateTime<Utc> = row.try_get("created_at")?;
        let modified_at: DateTime<Utc> = row.try_get("modified_at")?;

        let mut attributes: BTreeMap<String, Value> = serde_json::from_value(attrs_val)?;

        // Filter attributes if attrs parameter is provided
        if let Some(filter_attrs) = attrs {
            attributes.retain(|k, _| {
                filter_attrs.iter().any(|f| {
                    f == k
                        || f.strip_prefix("https://uri.etsi.org/ngsi-ld/default-context/")
                            == Some(k.as_str())
                })
            });
        }

        Ok(Some(Entity {
            id: entity_id,
            type_: entity_type,
            types: entity_types,
            attributes,
            scope,
            created_at: Some(created_at),
            modified_at: Some(modified_at),
            context: None,
        }))
    }

    async fn query_entities(
        &self,
        params: &EntityQueryParams,
    ) -> Result<Vec<Entity>, StorageError> {
        let mut sql = String::from(
            "SELECT id, type, types, attrs, scope, created_at, modified_at FROM entities WHERE 1=1",
        );
        let mut bind_params: Vec<SqlParam> = Vec::new();

        if let Some(id) = &params.id {
            let idx = bind_params.len() + 1;
            sql.push_str(&format!(" AND id = ANY(${idx}::text[])"));
            bind_params.push(SqlParam::StringList(
                id.split(',').map(String::from).collect(),
            ));
        }

        if let Some(pattern) = &params.id_pattern {
            let idx = bind_params.len() + 1;
            sql.push_str(&format!(" AND id ~ ${idx}"));
            bind_params.push(SqlParam::String(pattern.clone()));
        }

        if let Some(t) = &params.type_ {
            let idx = bind_params.len() + 1;
            sql.push_str(&format!(
                " AND (type = ANY(${idx}) OR types && ${idx}::text[])"
            ));
            bind_params.push(SqlParam::StringList(
                t.split(',').flat_map(type_aliases).collect(),
            ));
        }

        if let Some(attrs) = &params.attrs {
            let idx = bind_params.len() + 1;
            sql.push_str(&format!(" AND attrs ?| ${idx}::text[]"));
            bind_params.push(SqlParam::StringList(
                attrs.iter().flat_map(|s| type_aliases(s)).collect(),
            ));
        }

        if let Some(q_expr) = &params.q {
            let compiled = SqlCompiler::compile_q(q_expr, bind_params.len());
            sql.push_str(&format!(" AND {}", compiled.where_clause));
            bind_params.extend(compiled.params);
        }

        if let Some(geo_q) = &params.geo_q {
            let compiled = SqlCompiler::compile_geo(geo_q, bind_params.len() + 1);
            sql.push_str(&format!(" AND {}", compiled.where_clause));
            bind_params.extend(compiled.params);
        }

        sql.push_str(" ORDER BY modified_at DESC, id ASC");

        let limit = params.limit.unwrap_or(20).clamp(0, 1000);
        let offset = params.offset.unwrap_or(0).max(0);

        let limit_idx = bind_params.len() + 1;
        let offset_idx = bind_params.len() + 2;
        sql.push_str(&format!(" LIMIT ${limit_idx} OFFSET ${offset_idx}"));
        bind_params.push(SqlParam::Integer(limit as i64));
        bind_params.push(SqlParam::Integer(offset as i64));

        let mut query = sqlx::query(&sql);
        for param in bind_params {
            match param {
                SqlParam::String(s) => query = query.bind(s),
                SqlParam::Number(n) => query = query.bind(n),
                SqlParam::Integer(i) => query = query.bind(i),
                SqlParam::Boolean(b) => query = query.bind(b),
                SqlParam::StringList(list) => query = query.bind(list),
                SqlParam::NumberList(list) => query = query.bind(list),
            }
        }

        let rows = query.fetch_all(&self.pool).await?;
        let mut entities = Vec::new();

        for row in rows {
            let entity_id: String = row.try_get("id")?;
            let entity_type: String = row.try_get("type")?;
            let entity_types: Vec<String> = row.try_get("types")?;
            let attrs_val: Value = row.try_get("attrs")?;
            let scope: Option<Vec<String>> = row.try_get("scope")?;
            let created_at: DateTime<Utc> = row.try_get("created_at")?;
            let modified_at: DateTime<Utc> = row.try_get("modified_at")?;

            let mut attributes: BTreeMap<String, Value> = serde_json::from_value(attrs_val)?;
            if let Some(filter_attrs) = &params.attrs {
                attributes.retain(|k, _| {
                    filter_attrs.iter().any(|f| {
                        f == k
                            || f.strip_prefix("https://uri.etsi.org/ngsi-ld/default-context/")
                                == Some(k.as_str())
                    })
                });
            }

            entities.push(Entity {
                id: entity_id,
                type_: entity_type,
                types: entity_types,
                attributes,
                scope,
                created_at: Some(created_at),
                modified_at: Some(modified_at),
                context: None,
            });
        }

        Ok(entities)
    }

    async fn count_entities(&self, params: &EntityQueryParams) -> Result<i64, StorageError> {
        let mut sql = String::from("SELECT COUNT(*) as cnt FROM entities WHERE 1=1");
        let mut bind_params: Vec<SqlParam> = Vec::new();

        if let Some(id) = &params.id {
            let idx = bind_params.len() + 1;
            sql.push_str(&format!(" AND id = ANY(${idx}::text[])"));
            bind_params.push(SqlParam::StringList(
                id.split(',').map(String::from).collect(),
            ));
        }

        if let Some(pattern) = &params.id_pattern {
            let idx = bind_params.len() + 1;
            sql.push_str(&format!(" AND id ~ ${idx}"));
            bind_params.push(SqlParam::String(pattern.clone()));
        }

        if let Some(t) = &params.type_ {
            let idx = bind_params.len() + 1;
            sql.push_str(&format!(
                " AND (type = ANY(${idx}) OR types && ${idx}::text[])"
            ));
            bind_params.push(SqlParam::StringList(
                t.split(',').flat_map(type_aliases).collect(),
            ));
        }

        if let Some(attrs) = &params.attrs {
            let idx = bind_params.len() + 1;
            sql.push_str(&format!(" AND attrs ?| ${idx}::text[]"));
            bind_params.push(SqlParam::StringList(
                attrs.iter().flat_map(|s| type_aliases(s)).collect(),
            ));
        }

        if let Some(q_expr) = &params.q {
            let compiled = SqlCompiler::compile_q(q_expr, bind_params.len());
            sql.push_str(&format!(" AND {}", compiled.where_clause));
            bind_params.extend(compiled.params);
        }

        if let Some(geo_q) = &params.geo_q {
            let compiled = SqlCompiler::compile_geo(geo_q, bind_params.len() + 1);
            sql.push_str(&format!(" AND {}", compiled.where_clause));
            bind_params.extend(compiled.params);
        }

        let mut query = sqlx::query(&sql);
        for param in bind_params {
            match param {
                SqlParam::String(s) => query = query.bind(s),
                SqlParam::Number(n) => query = query.bind(n),
                SqlParam::Integer(i) => query = query.bind(i),
                SqlParam::Boolean(b) => query = query.bind(b),
                SqlParam::StringList(list) => query = query.bind(list),
                SqlParam::NumberList(list) => query = query.bind(list),
            }
        }

        let row = query.fetch_one(&self.pool).await?;
        let count: i64 = row.try_get("cnt")?;
        Ok(count)
    }

    async fn update_entity_attrs(&self, id: &str, new_attrs: &Value) -> Result<(), StorageError> {
        let clean = athena_model::attributes::without_context(new_attrs)
            .map_err(|e| StorageError::Problem(ProblemDetails::bad_request_data(e.to_string())))?;
        let new_attrs = &Value::Object(clean);
        let loc_geojson = new_attrs.get("location").and_then(|l| {
            if l.get("type").and_then(Value::as_str) == Some("GeoProperty") {
                l.get("value")
                    .map(|v| serde_json::to_string(v).unwrap_or_default())
            } else {
                None
            }
        });

        let res = sqlx::query(
            r#"
            UPDATE entities
            SET attrs = athena_merge_attrs(attrs, $2, true),
                location = CASE WHEN $3::text IS NOT NULL THEN ST_SetSRID(ST_GeomFromGeoJSON($3), 4326) ELSE location END,
                modified_at = NOW()
            WHERE id = $1
            "#,
        )
        .bind(id)
        .bind(new_attrs)
        .bind(loc_geojson)
        .execute(&self.pool)
        .await?;

        if res.rows_affected() == 0 {
            return Err(StorageError::EntityNotFound(id.to_string()));
        }

        Ok(())
    }

    async fn append_entity_attrs(
        &self,
        id: &str,
        new_attrs: &Value,
        overwrite: bool,
    ) -> Result<(), StorageError> {
        let clean = athena_model::attributes::without_context(new_attrs)
            .map_err(|e| StorageError::Problem(ProblemDetails::bad_request_data(e.to_string())))?;
        let new_attrs = &Value::Object(clean);
        if overwrite {
            self.update_entity_attrs(id, new_attrs).await
        } else {
            // Only add keys that do not already exist in attrs
            let res = sqlx::query(
                r#"
                UPDATE entities
                SET attrs = athena_merge_attrs(attrs, $2, false),
                    modified_at = NOW()
                WHERE id = $1
                "#,
            )
            .bind(id)
            .bind(new_attrs)
            .execute(&self.pool)
            .await?;

            if res.rows_affected() == 0 {
                return Err(StorageError::EntityNotFound(id.to_string()));
            }

            Ok(())
        }
    }

    async fn delete_entity_attr(&self, id: &str, attribute: &str) -> Result<(), StorageError> {
        self.delete_entity_attr_instance(id, attribute, None, false)
            .await
    }
    async fn delete_entity_attr_instance(
        &self,
        id: &str,
        attribute: &str,
        dataset: Option<&str>,
        all: bool,
    ) -> Result<(), StorageError> {
        let mut tx = self.pool.begin().await?;
        let attrs: Option<Value> =
            sqlx::query_scalar("SELECT attrs FROM entities WHERE id=$1 FOR UPDATE")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?;
        let mut attrs = attrs.ok_or_else(|| StorageError::EntityNotFound(id.into()))?;
        let attribute = if attrs.get(attribute).is_some() {
            attribute
        } else {
            attribute
                .strip_prefix("https://uri.etsi.org/ngsi-ld/default-context/")
                .unwrap_or(attribute)
        };
        let value = attrs
            .get(attribute)
            .ok_or_else(|| StorageError::EntityNotFound(attribute.into()))?;
        let instances = value
            .as_array()
            .cloned()
            .unwrap_or_else(|| vec![value.clone()]);
        let before = instances.len();
        let mut remaining: Vec<Value> = instances
            .into_iter()
            .filter(|v| !all && v.get("datasetId").and_then(Value::as_str) != dataset)
            .collect();
        if before == remaining.len() {
            return Err(StorageError::EntityNotFound(format!("{attribute} dataset")));
        }
        if remaining.is_empty() {
            attrs.as_object_mut().unwrap().remove(attribute);
        } else {
            attrs[attribute] = if remaining.len() == 1 {
                remaining.remove(0)
            } else {
                Value::Array(remaining)
            };
        }
        sqlx::query("UPDATE entities SET attrs=$2 WHERE id=$1")
            .bind(id)
            .bind(attrs)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn delete_entity(&self, id: &str) -> Result<(), StorageError> {
        let res = sqlx::query("DELETE FROM entities WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;

        if res.rows_affected() == 0 {
            return Err(StorageError::EntityNotFound(id.to_string()));
        }

        Ok(())
    }

    async fn batch_create(
        &self,
        entities: &[Entity],
    ) -> Result<BatchOperationResult, StorageError> {
        if entities.is_empty() {
            return Ok(BatchOperationResult::new());
        }
        let distinct = entities
            .iter()
            .map(|e| &e.id)
            .collect::<std::collections::HashSet<_>>()
            .len()
            == entities.len();
        if entities.len() <= 1000 && distinct {
            // A failed statement rolls back all its trigger side effects. Fall back
            // to per-item savepoints to preserve partial success on invalid data.
            if let Ok(result) = self.bulk_write_atomic(entities, false).await {
                return Ok(result);
            }
        }
        let mut result = BatchOperationResult::new();
        let mut tx = self.pool.begin().await?;

        for entity in entities {
            let attrs_json = serde_json::to_value(&entity.attributes)?;
            let loc_geojson = Self::extract_location_geojson(entity);

            sqlx::query("SAVEPOINT batch_item")
                .execute(&mut *tx)
                .await?;
            let res = sqlx::query(
                r#"
                INSERT INTO entities (id, type, types, attrs, location, scope, created_at, modified_at)
                VALUES ($1, $2, $3, $4,
                    CASE WHEN $5::text IS NOT NULL THEN ST_SetSRID(ST_GeomFromGeoJSON($5), 4326) ELSE NULL END,
                    $6, NOW(), NOW())
                "#,
            )
            .bind(&entity.id)
            .bind(&entity.type_)
            .bind(&entity.types)
            .bind(&attrs_json)
            .bind(&loc_geojson)
            .bind(&entity.scope)
            .execute(&mut *tx)
            .await;

            match res {
                Ok(_) => result.add_success(&entity.id),
                Err(e) => {
                    sqlx::query("ROLLBACK TO SAVEPOINT batch_item")
                        .execute(&mut *tx)
                        .await?;
                    result.add_error(
                        &entity.id,
                        ProblemDetails::already_exists(format!("Failed to insert entity: {e}")),
                    );
                }
            }
            sqlx::query("RELEASE SAVEPOINT batch_item")
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await?;
        Ok(result)
    }

    async fn batch_upsert(
        &self,
        entities: &[Entity],
    ) -> Result<BatchOperationResult, StorageError> {
        if entities.is_empty() {
            return Ok(BatchOperationResult::new());
        }
        let distinct = entities
            .iter()
            .map(|e| &e.id)
            .collect::<std::collections::HashSet<_>>()
            .len()
            == entities.len();
        if entities.len() <= 1000 && distinct {
            // A failed statement rolls back all its trigger side effects. Fall back
            // to per-item savepoints to preserve partial success on invalid data.
            if let Ok(result) = self.bulk_write_atomic(entities, true).await {
                return Ok(result);
            }
        }
        let mut result = BatchOperationResult::new();
        let mut tx = self.pool.begin().await?;

        for entity in entities {
            let attrs_json = serde_json::to_value(&entity.attributes)?;
            let loc_geojson = Self::extract_location_geojson(entity);

            sqlx::query("SAVEPOINT batch_item")
                .execute(&mut *tx)
                .await?;
            let res = sqlx::query(
                r#"
                INSERT INTO entities (id, type, types, attrs, location, scope, created_at, modified_at)
                VALUES ($1, $2, $3, $4,
                    CASE WHEN $5::text IS NOT NULL THEN ST_SetSRID(ST_GeomFromGeoJSON($5), 4326) ELSE NULL END,
                    $6, NOW(), NOW())
                ON CONFLICT (id) DO UPDATE
                SET attrs = EXCLUDED.attrs,
                    type = EXCLUDED.type, types = EXCLUDED.types, scope = EXCLUDED.scope,
                    location = CASE WHEN EXCLUDED.location IS NOT NULL THEN EXCLUDED.location ELSE entities.location END,
                    modified_at = NOW()
                "#,
            )
            .bind(&entity.id)
            .bind(&entity.type_)
            .bind(&entity.types)
            .bind(&attrs_json)
            .bind(&loc_geojson)
            .bind(&entity.scope)
            .execute(&mut *tx)
            .await;

            match res {
                Ok(_) => result.add_success(&entity.id),
                Err(e) => {
                    sqlx::query("ROLLBACK TO SAVEPOINT batch_item")
                        .execute(&mut *tx)
                        .await?;
                    result.add_error(
                        &entity.id,
                        ProblemDetails::bad_request_data(format!("Failed to upsert entity: {e}")),
                    );
                }
            }
            sqlx::query("RELEASE SAVEPOINT batch_item")
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await?;
        Ok(result)
    }

    async fn batch_update(
        &self,
        entities: &[Entity],
    ) -> Result<BatchOperationResult, StorageError> {
        let mut result = BatchOperationResult::new();
        let mut tx = self.pool.begin().await?;

        for entity in entities {
            let attrs_json = serde_json::to_value(&entity.attributes)?;
            let loc_geojson = Self::extract_location_geojson(entity);

            sqlx::query("SAVEPOINT batch_item")
                .execute(&mut *tx)
                .await?;
            let res = sqlx::query(
                r#"
                UPDATE entities
                SET attrs = athena_merge_attrs(attrs, $2, true),
                    location = CASE WHEN $3::text IS NOT NULL THEN ST_SetSRID(ST_GeomFromGeoJSON($3), 4326) ELSE location END,
                    modified_at = NOW()
                WHERE id = $1
                "#,
            )
            .bind(&entity.id)
            .bind(&attrs_json)
            .bind(&loc_geojson)
            .execute(&mut *tx)
            .await;

            match res {
                Ok(r) if r.rows_affected() > 0 => result.add_success(&entity.id),
                Ok(_) => result.add_error(
                    &entity.id,
                    ProblemDetails::not_found(format!("Entity '{}' does not exist", entity.id)),
                ),
                Err(e) => {
                    sqlx::query("ROLLBACK TO SAVEPOINT batch_item")
                        .execute(&mut *tx)
                        .await?;
                    result.add_error(
                        &entity.id,
                        ProblemDetails::bad_request_data(format!("Update failed: {e}")),
                    );
                }
            }
            sqlx::query("RELEASE SAVEPOINT batch_item")
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await?;
        Ok(result)
    }

    async fn batch_delete(
        &self,
        entity_ids: &[String],
    ) -> Result<BatchOperationResult, StorageError> {
        let mut result = BatchOperationResult::new();
        let mut tx = self.pool.begin().await?;

        for id in entity_ids {
            sqlx::query("SAVEPOINT batch_item")
                .execute(&mut *tx)
                .await?;
            let res = sqlx::query("DELETE FROM entities WHERE id = $1")
                .bind(id)
                .execute(&mut *tx)
                .await;

            match res {
                Ok(r) if r.rows_affected() > 0 => result.add_success(id),
                Ok(_) => result.add_error(
                    id,
                    ProblemDetails::not_found(format!("Entity '{id}' not found")),
                ),
                Err(e) => {
                    sqlx::query("ROLLBACK TO SAVEPOINT batch_item")
                        .execute(&mut *tx)
                        .await?;
                    result.add_error(
                        id,
                        ProblemDetails::internal_error(format!("Delete failed: {e}")),
                    );
                }
            }
            sqlx::query("RELEASE SAVEPOINT batch_item")
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await?;
        Ok(result)
    }
}

fn type_aliases(value: &str) -> Vec<String> {
    let mut aliases = vec![value.into()];
    if let Some(legacy) = value.strip_prefix("https://uri.etsi.org/ngsi-ld/default-context/") {
        aliases.push(legacy.into());
    }
    aliases
}
