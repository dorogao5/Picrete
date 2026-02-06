use sqlx::Row;

#[tokio::test]
async fn migrations_apply_and_tables_exist() -> anyhow::Result<()> {
    let database_url = match std::env::var("DATABASE_URL") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => {
            eprintln!("DATABASE_URL not set; skipping migrations smoke test");
            return Ok(());
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
        let row = sqlx::query("SELECT to_regclass($1)").bind(table).fetch_one(&pool).await?;
        let regclass: Option<String> = row.try_get(0)?;
        assert!(regclass.is_some(), "expected table {table} to exist after migrations");
    }

    Ok(())
}
