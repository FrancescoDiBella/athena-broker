use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{PgPool, Row};

use crate::repository::{CsourceRepository, StorageError};
use athena_model::{CsourceRegistration, Geometry, RegistrationInfo, RegistrationStatus};

pub struct PgCsourceStore {
    pool: PgPool,
}

impl PgCsourceStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl CsourceRepository for PgCsourceStore {
    async fn create_csource(&self, csource: &CsourceRegistration) -> Result<(), StorageError> {
        let info_json = serde_json::to_value(&csource.information)?;
        let status_str = match csource.status {
            RegistrationStatus::Active => "active",
            RegistrationStatus::Paused => "paused",
            RegistrationStatus::Expired => "expired",
        };
        let loc_json = csource
            .location
            .as_ref()
            .map(|g| serde_json::to_string(g).unwrap_or_default());

        sqlx::query(
            r#"
            INSERT INTO csource_registrations (
                id, registration_name, description, information, endpoint,
                context_source_info, location, expires_at, status, created_at, modified_at
            )
            VALUES (
                $1, $2, $3, $4, $5, $6,
                CASE WHEN $7::text IS NOT NULL THEN ST_SetSRID(ST_GeomFromGeoJSON($7), 4326) ELSE NULL END,
                $8, $9, NOW(), NOW()
            )
            "#,
        )
        .bind(&csource.id)
        .bind(&csource.registration_name)
        .bind(&csource.description)
        .bind(&info_json)
        .bind(&csource.endpoint)
        .bind(&csource.context_source_info)
        .bind(&loc_json)
        .bind(csource.expires_at)
        .bind(status_str)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn get_csource_by_id(
        &self,
        id: &str,
    ) -> Result<Option<CsourceRegistration>, StorageError> {
        let row = sqlx::query(
            r#"
            SELECT id, registration_name, description, information, endpoint,
                   context_source_info, ST_AsGeoJSON(location) as location_geojson,
                   expires_at, status, created_at, modified_at
            FROM csource_registrations
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

        let csr_id: String = row.try_get("id")?;
        let name: Option<String> = row.try_get("registration_name")?;
        let description: Option<String> = row.try_get("description")?;
        let info_val: Value = row.try_get("information")?;
        let endpoint: String = row.try_get("endpoint")?;
        let ctx_info: Option<Value> = row.try_get("context_source_info")?;
        let loc_geojson: Option<String> = row.try_get("location_geojson")?;
        let expires_at: Option<DateTime<Utc>> = row.try_get("expires_at")?;
        let status_str: String = row.try_get("status")?;
        let created_at: DateTime<Utc> = row.try_get("created_at")?;
        let modified_at: DateTime<Utc> = row.try_get("modified_at")?;

        let information: Vec<RegistrationInfo> = serde_json::from_value(info_val)?;
        let location = loc_geojson.and_then(|s| serde_json::from_str::<Geometry>(&s).ok());

        let status = match status_str.as_str() {
            "paused" => RegistrationStatus::Paused,
            "expired" => RegistrationStatus::Expired,
            _ => RegistrationStatus::Active,
        };

        Ok(Some(CsourceRegistration {
            id: csr_id,
            r#type: "ContextSourceRegistration".to_string(),
            registration_name: name,
            description,
            information,
            endpoint,
            context_source_info: ctx_info,
            location,
            observation_space: None,
            operation_space: None,
            expires_at,
            status,
            created_at: Some(created_at),
            modified_at: Some(modified_at),
            context: None,
        }))
    }

    async fn list_csources(
        &self,
        _entity_type: Option<&str>,
        _id: Option<&str>,
        limit: Option<i64>,
        offset: Option<i64>,
    ) -> Result<Vec<CsourceRegistration>, StorageError> {
        let l = limit.unwrap_or(20).clamp(1, 100);
        let o = offset.unwrap_or(0).max(0);

        let rows = sqlx::query(
            r#"
            SELECT id, registration_name, description, information, endpoint,
                   context_source_info, ST_AsGeoJSON(location) as location_geojson,
                   expires_at, status, created_at, modified_at
            FROM csource_registrations
            ORDER BY created_at DESC
            LIMIT $1 OFFSET $2
            "#,
        )
        .bind(l)
        .bind(o)
        .fetch_all(&self.pool)
        .await?;

        let mut list = Vec::new();
        for row in rows {
            let csr_id: String = row.try_get("id")?;
            let name: Option<String> = row.try_get("registration_name")?;
            let description: Option<String> = row.try_get("description")?;
            let info_val: Value = row.try_get("information")?;
            let endpoint: String = row.try_get("endpoint")?;
            let ctx_info: Option<Value> = row.try_get("context_source_info")?;
            let loc_geojson: Option<String> = row.try_get("location_geojson")?;
            let expires_at: Option<DateTime<Utc>> = row.try_get("expires_at")?;
            let status_str: String = row.try_get("status")?;
            let created_at: DateTime<Utc> = row.try_get("created_at")?;
            let modified_at: DateTime<Utc> = row.try_get("modified_at")?;

            let information: Vec<RegistrationInfo> = serde_json::from_value(info_val)?;
            let location = loc_geojson.and_then(|s| serde_json::from_str::<Geometry>(&s).ok());

            let status = match status_str.as_str() {
                "paused" => RegistrationStatus::Paused,
                "expired" => RegistrationStatus::Expired,
                _ => RegistrationStatus::Active,
            };

            list.push(CsourceRegistration {
                id: csr_id,
                r#type: "ContextSourceRegistration".to_string(),
                registration_name: name,
                description,
                information,
                endpoint,
                context_source_info: ctx_info,
                location,
                observation_space: None,
                operation_space: None,
                expires_at,
                status,
                created_at: Some(created_at),
                modified_at: Some(modified_at),
                context: None,
            });
        }

        Ok(list)
    }

    async fn update_csource(&self, id: &str, patch: &Value) -> Result<(), StorageError> {
        let mut updates = Vec::new();
        let mut idx = 2;

        if patch.get("description").and_then(Value::as_str).is_some() {
            updates.push(format!("description = ${idx}"));
            idx += 1;
        }

        if patch.get("status").and_then(Value::as_str).is_some() {
            updates.push(format!("status = ${idx}"));
        }

        if updates.is_empty() {
            return Ok(());
        }

        updates.push("modified_at = NOW()".to_string());
        let sql = format!(
            "UPDATE csource_registrations SET {} WHERE id = $1",
            updates.join(", ")
        );

        let mut query = sqlx::query(&sql).bind(id);
        if let Some(desc) = patch.get("description").and_then(Value::as_str) {
            query = query.bind(desc);
        }
        if let Some(status) = patch.get("status").and_then(Value::as_str) {
            query = query.bind(status);
        }

        let res = query.execute(&self.pool).await?;
        if res.rows_affected() == 0 {
            return Err(StorageError::CsourceNotFound(id.to_string()));
        }

        Ok(())
    }

    async fn delete_csource(&self, id: &str) -> Result<(), StorageError> {
        let res = sqlx::query("DELETE FROM csource_registrations WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;

        if res.rows_affected() == 0 {
            return Err(StorageError::CsourceNotFound(id.to_string()));
        }

        Ok(())
    }

    async fn get_matching_csources(
        &self,
        entity_type: Option<&str>,
        entity_id: Option<&str>,
        attrs: Option<&[String]>,
    ) -> Result<Vec<CsourceRegistration>, StorageError> {
        let all_active = self.list_csources(None, None, Some(1000), Some(0)).await?;
        let filtered: Vec<CsourceRegistration> = all_active
            .into_iter()
            .filter(|csr| csr.matches_entity_query(entity_type, entity_id, attrs))
            .collect();
        Ok(filtered)
    }
}
