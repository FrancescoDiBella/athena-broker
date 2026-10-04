use async_trait::async_trait;
use athena_model::{
    BatchOperationResult, CsourceRegistration, Entity, ProblemDetails, Subscription, TemporalQuery,
};
use athena_query::{GeoQuery, QueryExpr};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    Problem(#[from] ProblemDetails),

    #[error("Entity already exists: {0}")]
    EntityAlreadyExists(String),

    #[error("Entity not found: {0}")]
    EntityNotFound(String),

    #[error("Subscription not found: {0}")]
    SubscriptionNotFound(String),

    #[error("Context Source Registration not found: {0}")]
    CsourceNotFound(String),
}

impl StorageError {
    pub fn is_unique_violation(&self) -> bool {
        match self {
            StorageError::Database(sqlx::Error::Database(db_err)) => db_err.is_unique_violation(),
            StorageError::EntityAlreadyExists(_) => true,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct EntityQueryParams {
    pub id: Option<String>,
    pub id_pattern: Option<String>,
    pub type_: Option<String>,
    pub q: Option<QueryExpr>,
    pub geo_q: Option<GeoQuery>,
    pub attrs: Option<Vec<String>>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[async_trait]
pub trait EntityRepository: Send + Sync {
    async fn filter_snapshots(
        &self,
        entities: Vec<Entity>,
        params: &EntityQueryParams,
    ) -> Result<Vec<Entity>, StorageError>;
    async fn mutate_attributes(
        &self,
        id: &str,
        fragment: &serde_json::Value,
        operation: athena_model::AttributeOperation,
    ) -> Result<athena_model::UpdateResult, StorageError>;
    async fn mutate_attribute(
        &self,
        id: &str,
        attribute: &str,
        fragment: &serde_json::Value,
        replace: bool,
    ) -> Result<(), StorageError>;
    async fn replace_entity(
        &self,
        id: &str,
        payload: &serde_json::Value,
    ) -> Result<(), StorageError>;
    async fn create_entity(&self, entity: &Entity) -> Result<(), StorageError>;
    async fn get_entity_by_id(
        &self,
        id: &str,
        attrs: Option<&[String]>,
    ) -> Result<Option<Entity>, StorageError>;
    async fn query_entities(&self, params: &EntityQueryParams)
        -> Result<Vec<Entity>, StorageError>;
    async fn count_entities(&self, params: &EntityQueryParams) -> Result<i64, StorageError>;
    async fn update_entity_attrs(
        &self,
        id: &str,
        new_attrs: &serde_json::Value,
    ) -> Result<(), StorageError>;
    async fn append_entity_attrs(
        &self,
        id: &str,
        new_attrs: &serde_json::Value,
        overwrite: bool,
    ) -> Result<(), StorageError>;
    async fn delete_entity_attr(&self, id: &str, attr_id: &str) -> Result<(), StorageError>;
    async fn delete_entity_attr_instance(
        &self,
        id: &str,
        attr: &str,
        dataset: Option<&str>,
        all: bool,
    ) -> Result<(), StorageError>;
    async fn delete_entity(&self, id: &str) -> Result<(), StorageError>;

    // Batch operations
    async fn batch_create(&self, entities: &[Entity])
        -> Result<BatchOperationResult, StorageError>;
    async fn batch_upsert(&self, entities: &[Entity])
        -> Result<BatchOperationResult, StorageError>;
    async fn batch_update(&self, entities: &[Entity])
        -> Result<BatchOperationResult, StorageError>;
    async fn batch_delete(
        &self,
        entity_ids: &[String],
    ) -> Result<BatchOperationResult, StorageError>;
}

#[async_trait]
pub trait SubscriptionRepository: Send + Sync {
    async fn create_subscription(&self, sub: &Subscription) -> Result<(), StorageError>;
    async fn get_subscription_by_id(&self, id: &str) -> Result<Option<Subscription>, StorageError>;
    async fn list_subscriptions(
        &self,
        limit: Option<i64>,
        offset: Option<i64>,
    ) -> Result<Vec<Subscription>, StorageError>;
    async fn update_subscription(
        &self,
        id: &str,
        patch: &serde_json::Value,
    ) -> Result<(), StorageError>;
    async fn delete_subscription(&self, id: &str) -> Result<(), StorageError>;
    async fn get_active_subscriptions(&self) -> Result<Vec<Subscription>, StorageError>;
    async fn record_notification_success(&self, id: &str) -> Result<(), StorageError>;
    async fn record_notification_failure(&self, id: &str) -> Result<(), StorageError>;
}

#[async_trait]
pub trait CsourceRepository: Send + Sync {
    async fn create_csource(&self, csource: &CsourceRegistration) -> Result<(), StorageError>;
    async fn get_csource_by_id(
        &self,
        id: &str,
    ) -> Result<Option<CsourceRegistration>, StorageError>;
    async fn list_csources(
        &self,
        entity_type: Option<&str>,
        id: Option<&str>,
        limit: Option<i64>,
        offset: Option<i64>,
    ) -> Result<Vec<CsourceRegistration>, StorageError>;
    async fn update_csource(&self, id: &str, patch: &serde_json::Value)
        -> Result<(), StorageError>;
    async fn delete_csource(&self, id: &str) -> Result<(), StorageError>;
    async fn get_matching_csources(
        &self,
        entity_type: Option<&str>,
        entity_id: Option<&str>,
        attrs: Option<&[String]>,
    ) -> Result<Vec<CsourceRegistration>, StorageError>;
}

#[async_trait]
pub trait TemporalRepository: Send + Sync {
    async fn count_temporal_entities(
        &self,
        entity_type: Option<&str>,
        attrs: Option<&[String]>,
        query: &TemporalQuery,
    ) -> Result<i64, StorageError>;
    async fn delete_temporal(
        &self,
        id: &str,
        attribute: Option<&str>,
        dataset: Option<&str>,
        instance: Option<&str>,
        delete_all: bool,
    ) -> Result<(), StorageError>;
    async fn update_temporal_instance(
        &self,
        id: &str,
        attribute: &str,
        instance: &str,
        patch: &serde_json::Value,
    ) -> Result<(), StorageError>;

    async fn create_temporal_entity(
        &self,
        entity_id: &str,
        entity_type: &str,
        attributes: &serde_json::Value,
    ) -> Result<(), StorageError>;

    async fn record_temporal_instance(
        &self,
        entity_id: &str,
        entity_type: &str,
        attr_id: &str,
        observed_at: chrono::DateTime<chrono::Utc>,
        value: &serde_json::Value,
        dataset_id: Option<&str>,
    ) -> Result<(), StorageError>;

    async fn query_temporal(
        &self,
        entity_id: &str,
        attrs: Option<&[String]>,
        query: &TemporalQuery,
    ) -> Result<serde_json::Value, StorageError>;

    async fn query_temporal_entities(
        &self,
        entity_type: Option<&str>,
        attrs: Option<&[String]>,
        query: &TemporalQuery,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<serde_json::Value>, StorageError>;
}
