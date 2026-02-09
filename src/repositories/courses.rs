use sqlx::PgPool;

use crate::db::models::{Course, CourseIdentityPolicy};

const COURSE_COLUMNS: &str =
    "id, slug, title, organization, is_active, created_by, created_at, updated_at";

pub(crate) struct CreateCourse<'a> {
    pub(crate) id: &'a str,
    pub(crate) slug: &'a str,
    pub(crate) title: &'a str,
    pub(crate) organization: Option<&'a str>,
    pub(crate) is_active: bool,
    pub(crate) created_by: &'a str,
    pub(crate) created_at: time::PrimitiveDateTime,
    pub(crate) updated_at: time::PrimitiveDateTime,
}

pub(crate) struct UpdateCourse {
    pub(crate) title: Option<String>,
    pub(crate) organization: Option<String>,
    pub(crate) is_active: Option<bool>,
    pub(crate) updated_at: time::PrimitiveDateTime,
}

pub(crate) async fn create(pool: &PgPool, params: CreateCourse<'_>) -> Result<Course, sqlx::Error> {
    sqlx::query_as::<_, Course>(&format!(
        "INSERT INTO courses (
            id, slug, title, organization, is_active, created_by, created_at, updated_at
         ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
         RETURNING {COURSE_COLUMNS}",
    ))
    .bind(params.id)
    .bind(params.slug)
    .bind(params.title)
    .bind(params.organization)
    .bind(params.is_active)
    .bind(params.created_by)
    .bind(params.created_at)
    .bind(params.updated_at)
    .fetch_one(pool)
    .await
}

pub(crate) async fn find_by_id(
    pool: &PgPool,
    course_id: &str,
) -> Result<Option<Course>, sqlx::Error> {
    sqlx::query_as::<_, Course>(&format!("SELECT {COURSE_COLUMNS} FROM courses WHERE id = $1"))
        .bind(course_id)
        .fetch_optional(pool)
        .await
}

pub(crate) async fn fetch_one_by_id(pool: &PgPool, course_id: &str) -> Result<Course, sqlx::Error> {
    sqlx::query_as::<_, Course>(&format!("SELECT {COURSE_COLUMNS} FROM courses WHERE id = $1"))
        .bind(course_id)
        .fetch_one(pool)
        .await
}

pub(crate) async fn update(
    pool: &PgPool,
    course_id: &str,
    params: UpdateCourse,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE courses SET
            title = COALESCE($1, title),
            organization = COALESCE($2, organization),
            is_active = COALESCE($3, is_active),
            updated_at = $4
         WHERE id = $5",
    )
    .bind(params.title)
    .bind(params.organization)
    .bind(params.is_active)
    .bind(params.updated_at)
    .bind(course_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn ensure_default_identity_policy(
    pool: &PgPool,
    course_id: &str,
    updated_at: time::PrimitiveDateTime,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO course_identity_policies (course_id, rule_type, rule_config, updated_at)
         VALUES ($1, 'none', '{}'::jsonb, $2)
         ON CONFLICT (course_id) DO NOTHING",
    )
    .bind(course_id)
    .bind(updated_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn upsert_identity_policy(
    pool: &PgPool,
    course_id: &str,
    rule_type: &str,
    rule_config: serde_json::Value,
    updated_at: time::PrimitiveDateTime,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO course_identity_policies (course_id, rule_type, rule_config, updated_at)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (course_id)
         DO UPDATE SET rule_type = EXCLUDED.rule_type,
                       rule_config = EXCLUDED.rule_config,
                       updated_at = EXCLUDED.updated_at",
    )
    .bind(course_id)
    .bind(rule_type)
    .bind(rule_config)
    .bind(updated_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn find_identity_policy(
    pool: &PgPool,
    course_id: &str,
) -> Result<Option<CourseIdentityPolicy>, sqlx::Error> {
    sqlx::query_as::<_, CourseIdentityPolicy>(
        "SELECT course_id, rule_type, rule_config, updated_at
         FROM course_identity_policies
         WHERE course_id = $1",
    )
    .bind(course_id)
    .fetch_optional(pool)
    .await
}
