pub mod config;
pub mod maintenance;
pub const SCHEMA_VERSION: i64 = 10;
pub mod csource_store;
pub mod entity_store;
pub mod repository;
pub mod subscription_store;
pub mod temporal_store;

pub use config::DatabaseConfig;
pub use csource_store::PgCsourceStore;
pub use entity_store::PgEntityStore;
pub use repository::{
    CsourceRepository, EntityQueryParams, EntityRepository, StorageError, SubscriptionRepository,
    TemporalRepository,
};
pub use subscription_store::PgSubscriptionStore;
pub use temporal_store::PgTemporalStore;

use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

pub async fn create_connection_pool(config: &DatabaseConfig) -> Result<PgPool, sqlx::Error> {
    let statement_timeout = format!("{}ms", config.statement_timeout_ms);
    let lock_timeout = format!("{}ms", config.lock_timeout_ms);
    PgPoolOptions::new()
        .max_connections(config.max_connections)
        .min_connections(config.min_connections)
        .acquire_timeout(config.connect_timeout())
        .idle_timeout(config.idle_timeout())
        .after_connect(move |connection, _| {
            let statement_timeout = statement_timeout.clone();
            let lock_timeout = lock_timeout.clone();
            Box::pin(async move {
                sqlx::query("SELECT set_config('statement_timeout',$1,false),set_config('lock_timeout',$2,false)")
                    .bind(statement_timeout).bind(lock_timeout)
                    .execute(&mut *connection)
                    .await?;
                Ok(())
            })
        })
        .connect(&config.url)
        .await
}

pub async fn run_migrations(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    use sqlx::migrate::{Migration, MigrationType, Migrator};
    use std::borrow::Cow;
    let scripts = [
        (
            1,
            "initial schema",
            include_str!("../migrations/0001_init_schema.sql"),
        ),
        (
            2,
            "temporal storage",
            include_str!("../migrations/0002_temporal_hypertable.sql"),
        ),
        (
            3,
            "context sources",
            include_str!("../migrations/0003_csource_registrations.sql"),
        ),
        (
            4,
            "durable history and notifications",
            include_str!("../migrations/0004_durable_history.sql"),
        ),
        (
            5,
            "dataset mutations",
            include_str!("../migrations/0005_dataset_mutations.sql"),
        ),
        (
            6,
            "history dataset identity join",
            include_str!("../migrations/0006_history_dataset_join.sql"),
        ),
        (
            7,
            "operations and subscription lifecycle",
            include_str!("../migrations/0007_operations_lifecycle.sql"),
        ),
        (
            8,
            "subscription delivery ordering",
            include_str!("../migrations/0008_subscription_delivery_order.sql"),
        ),
        (
            9,
            "notification claim indexes",
            include_str!("../migrations/0009_notification_claim_indexes.sql"),
        ),
        (
            10,
            "persistent periodic subscriptions",
            include_str!("../migrations/0010_subscription_scheduling.sql"),
        ),
    ];
    let migrations = scripts
        .into_iter()
        .map(|(version, description, sql)| {
            Migration::new(
                version,
                Cow::Borrowed(description),
                MigrationType::Simple,
                Cow::Borrowed(sql),
            )
        })
        .collect::<Vec<_>>();
    Migrator {
        migrations: Cow::Owned(migrations),
        ..Migrator::DEFAULT
    }
    .run(pool)
    .await
}
