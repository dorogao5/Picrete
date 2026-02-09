use sqlx::PgPool;

use crate::db::models::CourseInviteCode;
use crate::db::types::CourseRole;

const COLUMNS: &str = "\
    id, course_id, role, code_hash, is_active, rotated_from_id, \
    expires_at, usage_count, created_at, updated_at";

pub(crate) struct CreateInviteCode<'a> {
    pub(crate) id: &'a str,
    pub(crate) course_id: &'a str,
    pub(crate) role: CourseRole,
    pub(crate) code_hash: &'a str,
    pub(crate) is_active: bool,
    pub(crate) rotated_from_id: Option<&'a str>,
    pub(crate) expires_at: Option<time::PrimitiveDateTime>,
    pub(crate) usage_count: i64,
    pub(crate) created_at: time::PrimitiveDateTime,
    pub(crate) updated_at: time::PrimitiveDateTime,
}

pub(crate) async fn create(
    pool: &PgPool,
    params: CreateInviteCode<'_>,
) -> Result<CourseInviteCode, sqlx::Error> {
    sqlx::query_as::<_, CourseInviteCode>(&format!(
        "INSERT INTO course_invite_codes (
            id, course_id, role, code_hash, is_active,
            rotated_from_id, expires_at, usage_count, created_at, updated_at
         ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
         RETURNING {COLUMNS}",
    ))
    .bind(params.id)
    .bind(params.course_id)
    .bind(params.role)
    .bind(params.code_hash)
    .bind(params.is_active)
    .bind(params.rotated_from_id)
    .bind(params.expires_at)
    .bind(params.usage_count)
    .bind(params.created_at)
    .bind(params.updated_at)
    .fetch_one(pool)
    .await
}

pub(crate) async fn find_active_for_course_role(
    pool: &PgPool,
    course_id: &str,
    role: CourseRole,
) -> Result<Option<CourseInviteCode>, sqlx::Error> {
    sqlx::query_as::<_, CourseInviteCode>(&format!(
        "SELECT {COLUMNS}
         FROM course_invite_codes
         WHERE course_id = $1
           AND role = $2
           AND is_active = TRUE
         ORDER BY created_at DESC
         LIMIT 1",
    ))
    .bind(course_id)
    .bind(role)
    .fetch_optional(pool)
    .await
}

pub(crate) async fn deactivate(
    pool: &PgPool,
    invite_id: &str,
    updated_at: time::PrimitiveDateTime,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE course_invite_codes
         SET is_active = FALSE,
             updated_at = $1
         WHERE id = $2",
    )
    .bind(updated_at)
    .bind(invite_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn find_active_by_hash(
    pool: &PgPool,
    code_hash: &str,
    now: time::PrimitiveDateTime,
) -> Result<Option<CourseInviteCode>, sqlx::Error> {
    sqlx::query_as::<_, CourseInviteCode>(&format!(
        "SELECT {COLUMNS}
         FROM course_invite_codes
         WHERE code_hash = $1
           AND is_active = TRUE
           AND (expires_at IS NULL OR expires_at > $2)
         LIMIT 1",
    ))
    .bind(code_hash)
    .bind(now)
    .fetch_optional(pool)
    .await
}

pub(crate) async fn increment_usage(
    pool: &PgPool,
    invite_id: &str,
    updated_at: time::PrimitiveDateTime,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE course_invite_codes
         SET usage_count = usage_count + 1,
             updated_at = $1
         WHERE id = $2",
    )
    .bind(updated_at)
    .bind(invite_id)
    .execute(pool)
    .await?;
    Ok(())
}
