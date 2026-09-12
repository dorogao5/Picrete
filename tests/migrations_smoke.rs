use sqlx::Row;

fn database_url() -> String {
    // Never infer a migration-test target from production .env credentials.
    std::env::var("PICRETE_TEST_DATABASE_URL").unwrap_or_else(|_| {
        "postgresql://picrete_test:picrete_test@localhost:5432/picrete_rust_test".to_string()
    })
}

#[tokio::test]
async fn migrations_apply_and_tables_exist() -> anyhow::Result<()> {
    let database_url = database_url();

    let pool =
        sqlx::postgres::PgPoolOptions::new().max_connections(1).connect(&database_url).await?;

    let database_name: String =
        sqlx::query_scalar("SELECT current_database()").fetch_one(&pool).await?;
    anyhow::ensure!(
        database_name.ends_with("_test"),
        "Migration smoke tests require a test database"
    );

    let migrations_dir =
        std::env::var("PICRETE_MIGRATIONS_DIR").unwrap_or_else(|_| "migrations".to_string());
    let migrator = sqlx::migrate::Migrator::new(std::path::Path::new(&migrations_dir)).await?;
    migrator.run(&pool).await?;

    let tables = [
        "course_trainers",
        "practice_attempts",
        "practice_jobs",
        "practice_photos",
        "users",
        "exams",
        "task_types",
        "task_variants",
        "exam_sessions",
        "submissions",
        "submission_images",
        "submission_scores",
        "telegram_user_links",
        "telegram_selected_sessions",
        "telegram_bot_offsets",
    ];

    for table in tables {
        let row = sqlx::query("SELECT to_regclass($1)::text").bind(table).fetch_one(&pool).await?;
        let regclass: Option<String> = row.try_get(0)?;
        assert!(regclass.is_some(), "expected table {table} to exist after migrations");
    }

    Ok(())
}
