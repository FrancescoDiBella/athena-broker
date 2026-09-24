mod config;

use std::net::SocketAddr;
use std::sync::Arc;
use tokio::signal;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use athena_api::{create_router, AppState};
use athena_jsonld::ContextResolver;
use athena_storage::{
    create_connection_pool, run_migrations, PgEntityStore, PgSubscriptionStore, PgTemporalStore,
};
use athena_subscription::SubscriptionEngine;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 && (args[1] == "--version" || args[1] == "-v") {
        println!("athena-broker {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    let mut config_path = std::env::var_os("ATHENA_CONFIG").map(std::path::PathBuf::from);
    let mut healthcheck = false;
    let mut check_config = false;
    let mut arguments = args.iter().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--config" => config_path = Some(arguments.next().ok_or_else(|| anyhow::anyhow!("--config requires a path"))?.into()),
            "--healthcheck" => healthcheck = true,
            "--check-config" => check_config = true,
            _ => anyhow::bail!("Unknown argument; expected --config PATH, --check-config, --healthcheck or --version"),
        }
    }
    let config = config::BrokerConfig::load(config_path.as_deref())?;
    if check_config {
        println!("Configuration valid: listen={}:{}, db_pool={}, notification_workers={}, write_concurrency={}",
            config.server.host, config.server.port, config.database.max_connections,
            config.subscriptions.worker_threads, config.limits.max_in_flight_writes);
        return Ok(());
    }
    if healthcheck {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let host = if config.server.host.is_unspecified() {
            if config.server.host.is_ipv6() {
                "::1".parse().unwrap()
            } else {
                "127.0.0.1".parse().unwrap()
            }
        } else {
            config.server.host
        };
        let check = async {
            let mut stream =
                tokio::net::TcpStream::connect(SocketAddr::new(host, config.server.port)).await?;
            stream
                .write_all(b"GET /ready HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                .await?;
            let mut status = [0u8; 12];
            stream.read_exact(&mut status).await?;
            anyhow::ensure!(&status[9..12] == b"200", "Broker is not ready");
            Ok::<_, anyhow::Error>(())
        };
        return tokio::time::timeout(std::time::Duration::from_secs(4), check).await?;
    }
    tracing_subscriber::registry()
        .with(EnvFilter::try_new(&config.server.log_level)?)
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!(
        "Starting Athena NGSI-LD Context Broker v{}",
        env!("CARGO_PKG_VERSION")
    );

    // 2. Database Connection & Schema Migrations
    let db_config = &config.database;
    info!("Connecting to PostgreSQL");

    let pool = create_connection_pool(db_config).await?;
    info!(
        "Database connection pool established (max: {})",
        db_config.max_connections
    );

    info!("Applying PostgreSQL schema migrations...");
    run_migrations(&pool).await?;
    info!("Database schema migrations applied successfully");

    // 3. Storage Repositories & Engine Initialization
    let entity_repo = Arc::new(PgEntityStore::new(pool.clone()));
    let subscription_repo = Arc::new(PgSubscriptionStore::new(pool.clone()));
    let temporal_repo = Arc::new(PgTemporalStore::new(pool.clone()));
    let csource_repo = Arc::new(athena_storage::PgCsourceStore::new(pool.clone()));

    let policy = athena_http::OutboundPolicy {
        allow_private: config.security.allow_internal_endpoints,
    };
    let subscription_engine = Arc::new(
        SubscriptionEngine::with_config(
            subscription_repo.clone(),
            pool.clone(),
            config.subscriptions.clone(),
            policy,
        )
        .map_err(anyhow::Error::msg)?,
    );
    let context_resolver = Arc::new(ContextResolver::with_policy(
        config.jsonld.context_cache_size,
        policy,
    ));
    let processor = Arc::new(athena_jsonld::Processor::with_options(
        config.jsonld.context_cache_size,
        config.jsonld.max_concurrency,
        policy,
    ));

    // 4. Build Application State & Axum Router
    let app_state = AppState::new(
        pool.clone(),
        entity_repo,
        subscription_repo,
        temporal_repo,
        csource_repo,
        subscription_engine.clone(),
        context_resolver,
    )
    .configure(config.limits.clone(), policy, processor);

    let app = create_router(app_state);

    let addr = SocketAddr::new(config.server.host, config.server.port);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!("Athena NGSI-LD Broker listening on http://{}", addr);

    let (stop, mut maintenance_stop) = tokio::sync::watch::channel(false);
    let maintenance_pool = pool.clone();
    let retention = config.retention.clone();
    let maintenance = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = maintenance_stop.changed() => break,
                _ = tokio::time::sleep(std::time::Duration::from_secs(retention.interval_sec)) => {}
            }
            match athena_storage::maintenance::cleanup_once(&maintenance_pool, &retention).await {
                Ok(result) => tracing::debug!(?result, "Retention batch completed"),
                Err(error) => {
                    tracing::error!(%error, "Retention batch failed; next interval will retry")
                }
            }
        }
    });
    let grace = std::time::Duration::from_secs(config.server.shutdown_grace_sec);
    let mut stopped = stop.subscribe();
    let signal_stop = stop.clone();
    let signal_engine = subscription_engine.clone();
    let signal_task = tokio::spawn(async move {
        shutdown_signal().await;
        signal_engine.stop_claiming();
        signal_stop.send_replace(true);
    });
    let shutdown = async move {
        let _ = stopped.wait_for(|stopped| *stopped).await;
    };
    let server = axum::serve(listener, app).with_graceful_shutdown(shutdown);
    let mut server = Box::pin(std::future::IntoFuture::into_future(server));
    let mut observe_stop = stop.subscribe();
    let mut shutdown_deadline = None;
    let result = tokio::select! {
        result = &mut server => result,
        _ = observe_stop.wait_for(|stopped| *stopped) => {
            shutdown_deadline = Some(tokio::time::Instant::now() + grace);
            info!("Draining HTTP requests within the shutdown deadline");
            match tokio::time::timeout(grace, &mut server).await {
                Ok(result) => result,
                Err(_) => { tracing::warn!("HTTP shutdown deadline reached; cancelling remaining requests"); Ok(()) }
            }
        }
    };
    drop(server);
    stop.send_replace(true);
    signal_task.abort();
    let deadline = shutdown_deadline.unwrap_or_else(|| tokio::time::Instant::now() + grace);
    if !subscription_engine
        .shutdown(deadline.saturating_duration_since(tokio::time::Instant::now()))
        .await
    {
        tracing::warn!("Worker shutdown deadline reached; pending leases remain recoverable");
    }
    let mut maintenance = maintenance;
    if tokio::time::timeout_at(deadline, &mut maintenance)
        .await
        .is_err()
    {
        maintenance.abort();
        let _ = maintenance.await;
    }
    // Bound pool close too: cancelled HTTP tasks may still be unwinding.
    let _ = tokio::time::timeout_at(deadline, pool.close()).await;
    result?;
    info!("Athena NGSI-LD Broker shut down");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            info!("Received Ctrl+C, initiating graceful shutdown");
        },
        _ = terminate => {
            info!("Received SIGTERM, initiating graceful shutdown");
        },
    }
}
