use sqlx::PgPool;

use crate::db::models::User;
use crate::db::types::UserRole;

const COLUMNS: &str = "\
    id, isu, hashed_password, full_name, role, is_active, is_verified, \
    pd_consent, pd_consent_at, pd_consent_version, terms_accepted_at, \
    terms_version, privacy_version, created_at, updated_at";

pub(crate) async fn find_by_id(pool: &PgPool, id: &str) -> Result<Option<User>, sqlx::Error> {
    sqlx::query_as::<_, User>(&format!("SELECT {COLUMNS} FROM users WHERE id = $1"))
        .bind(id)
        .fetch_optional(pool)
        .await
}

pub(crate) async fn find_by_isu(pool: &PgPool, isu: &str) -> Result<Option<User>, sqlx::Error> {
    sqlx::query_as::<_, User>(&format!("SELECT {COLUMNS} FROM users WHERE isu = $1"))
        .bind(isu)
        .fetch_optional(pool)
        .await
}

pub(crate) async fn exists_by_isu(pool: &PgPool, isu: &str) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar::<_, String>("SELECT id FROM users WHERE isu = $1")
        .bind(isu)
        .fetch_optional(pool)
        .await
}

pub(crate) async fn find_name_by_id(pool: &PgPool, id: &str) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar::<_, String>("SELECT full_name FROM users WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
}

pub(crate) async fn find_isu_by_id(pool: &PgPool, id: &str) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar::<_, String>("SELECT isu FROM users WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
}

pub(crate) struct CreateUser<'a> {
    pub id: &'a str,
    pub isu: &'a str,
    pub hashed_password: String,
    pub full_name: &'a str,
    pub role: UserRole,
    pub is_active: bool,
    pub is_verified: bool,
    pub pd_consent: bool,
    pub pd_consent_at: Option<time::OffsetDateTime>,
    pub pd_consent_version: Option<String>,
    pub terms_accepted_at: Option<time::OffsetDateTime>,
    pub terms_version: Option<String>,
    pub privacy_version: Option<String>,
    pub created_at: time::PrimitiveDateTime,
    pub updated_at: time::PrimitiveDateTime,
}

pub(crate) async fn create(pool: &PgPool, params: CreateUser<'_>) -> Result<User, sqlx::Error> {
    sqlx::query_as::<_, User>(&format!(
        "INSERT INTO users (
            id, isu, hashed_password, full_name, role, is_active, is_verified,
            pd_consent, pd_consent_at, pd_consent_version,
            terms_accepted_at, terms_version, privacy_version,
            created_at, updated_at
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)
        RETURNING {COLUMNS}",
    ))
    .bind(params.id)
    .bind(params.isu)
    .bind(params.hashed_password)
    .bind(params.full_name)
    .bind(params.role)
    .bind(params.is_active)
    .bind(params.is_verified)
    .bind(params.pd_consent)
    .bind(params.pd_consent_at)
    .bind(params.pd_consent_version)
    .bind(params.terms_accepted_at)
    .bind(params.terms_version)
    .bind(params.privacy_version)
    .bind(params.created_at)
    .bind(params.updated_at)
    .fetch_one(pool)
    .await
}

pub(crate) struct UpdateUser {
    pub full_name: Option<String>,
    pub role: Option<UserRole>,
    pub is_active: Option<bool>,
    pub is_verified: Option<bool>,
    pub hashed_password: Option<String>,
    pub updated_at: time::PrimitiveDateTime,
}

pub(crate) async fn update(pool: &PgPool, id: &str, params: UpdateUser) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE users SET
            full_name = COALESCE($1, full_name),
            role = COALESCE($2, role),
            is_active = COALESCE($3, is_active),
            is_verified = COALESCE($4, is_verified),
            hashed_password = COALESCE($5, hashed_password),
            updated_at = $6
         WHERE id = $7",
    )
    .bind(params.full_name)
    .bind(params.role)
    .bind(params.is_active)
    .bind(params.is_verified)
    .bind(params.hashed_password)
    .bind(params.updated_at)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn fetch_one_by_id(pool: &PgPool, id: &str) -> Result<User, sqlx::Error> {
    sqlx::query_as::<_, User>(&format!("SELECT {COLUMNS} FROM users WHERE id = $1"))
        .bind(id)
        .fetch_one(pool)
        .await
}
