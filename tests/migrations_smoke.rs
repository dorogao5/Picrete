use sqlx::Row;

fn database_url() -> Option<String> {
    // Load .env so POSTGRES_* from .env are available (integration tests don't use app config)
    dotenvy::dotenv().ok();

    if let Ok(url) = std::env::var("DATABASE_URL") {
        if !url.trim().is_empty() {
            return Some(url);
        }
    }

    // Build from POSTGRES_* (same as app config)
    let server = std::env::var("POSTGRES_SERVER").unwrap_or_else(|_| "localhost".into());
    let port = std::env::var("POSTGRES_PORT").unwrap_or_else(|_| "5432".into());
    let user = std::env::var("POSTGRES_USER").unwrap_or_else(|_| "picretesuperuser".into());
    let password = std::env::var("POSTGRES_PASSWORD").unwrap_or_default();
    let db = std::env::var("POSTGRES_DB").unwrap_or_else(|_| "picrete_db".into());

    Some(format!("postgresql://{user}:{password}@{server}:{port}/{db}"))
}

#[tokio::test]
async fn migrations_apply_and_tables_exist() -> anyhow::Result<()> {
    let database_url = match database_url() {
        Some(url) => url,
        None => {
            anyhow::bail!("DATABASE_URL and POSTGRES_* are not set");
        }
    };

    let pool =
        sqlx::postgres::PgPoolOptions::new().max_connections(1).connect(&database_url).await?;

    sqlx::migrate!("./migrations").run(&pool).await?;

    let tables = [
        "users",
        "exams",
        "task_types",
        "task_variants",
        "exam_sessions",
        "submissions",
        "submission_images",
        "submission_scores",
    ];

    for table in tables {
        let row = sqlx::query("SELECT to_regclass($1)::text").bind(table).fetch_one(&pool).await?;
        let regclass: Option<String> = row.try_get(0)?;
        assert!(regclass.is_some(), "expected table {table} to exist after migrations");
    }

    Ok(())
}
