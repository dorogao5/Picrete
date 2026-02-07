pub(crate) mod models;
pub(crate) mod types;

use std::time::Duration;

use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{ConnectOptions, PgPool};

use crate::core::config::Settings;

pub(crate) async fn init_pool(settings: &Settings) -> Result<PgPool, sqlx::Error> {
    let database_url = settings.database().database_url();
    let mut connect_options: PgConnectOptions = database_url.parse()?;

    connect_options = connect_options
        .application_name("picrete-rust")
        .log_statements(tracing::log::LevelFilter::Off);

    PgPoolOptions::new()
        .max_connections(30)
        .min_connections(1)
        .acquire_timeout(Duration::from_secs(30))
        .test_before_acquire(true)
        .connect_with(connect_options)
        .await
}

pub(crate) async fn run_migrations(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::migrate!("./migrations").run(pool).await?;
    Ok(())
}
