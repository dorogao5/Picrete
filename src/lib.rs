pub(crate) mod api;
pub(crate) mod core;
pub(crate) mod db;
pub(crate) mod repositories;
pub(crate) mod schemas;
pub(crate) mod services;
pub(crate) mod tasks;

#[cfg(test)]
mod test_support;

use crate::core::{config::Settings, redis::RedisHandle, state::AppState, telemetry};
use crate::services::storage::StorageService;

pub async fn run() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let settings = Settings::load()?;
    telemetry::init_tracing(&settings)?;
    core::metrics::init(&settings)?;

    let db_pool = db::init_pool(&settings).await?;
    db::run_migrations(&db_pool).await?;

    let redis = RedisHandle::new(settings.redis().redis_url());
    if let Err(err) = redis.connect().await {
        tracing::error!(error = %err, "Failed to connect to Redis; continuing without cache");
    } else {
        tracing::info!("Redis connected successfully");
    }

    let storage = StorageService::from_settings(&settings).await?;
    let state = AppState::new(settings, db_pool, redis.clone(), storage);

    if let Err(err) = core::bootstrap::ensure_superuser(&state).await {
        tracing::error!(error = %err, "Failed to ensure default superuser");
    }
    let app = api::router::router(state.clone());
    let listener = tokio::net::TcpListener::bind(state.settings().server_addr()).await?;

    tracing::info!(
        host = %state.settings().server_host(),
        port = state.settings().server_port(),
        environment = %state.settings().runtime().environment.as_str(),
        "Picrete Rust API listening"
    );

    let result =
        axum::serve(listener, app).with_graceful_shutdown(core::shutdown::shutdown_signal()).await;

    redis.disconnect().await;
    tracing::info!("Redis disconnected");

    result?;

    Ok(())
}

pub async fn run_worker() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let settings = Settings::load()?;
    telemetry::init_tracing(&settings)?;
    core::metrics::init(&settings)?;

    let db_pool = db::init_pool(&settings).await?;
    db::run_migrations(&db_pool).await?;

    let redis = RedisHandle::new(settings.redis().redis_url());
    if let Err(err) = redis.connect().await {
        tracing::error!(error = %err, "Failed to connect to Redis; continuing without cache");
    } else {
        tracing::info!("Redis connected successfully");
    }

    let storage = StorageService::from_settings(&settings).await?;
    let state = AppState::new(settings, db_pool, redis.clone(), storage);

    let result = tasks::scheduler::run(state).await;

    redis.disconnect().await;
    tracing::info!("Redis disconnected");

    result?;

    Ok(())
}
