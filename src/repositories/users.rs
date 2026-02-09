use sqlx::PgPool;
use sqlx::{Postgres, QueryBuilder};

use crate::db::models::User;

const COLUMNS: &str = "\
    id, username, hashed_password, full_name, is_platform_admin, is_active, is_verified, \
    pd_consent, pd_consent_at, pd_consent_version, terms_accepted_at, \
    terms_version, privacy_version, created_at, updated_at";

#[derive(Clone, Copy, Default)]
pub(crate) struct UserListFilters<'a> {
    pub(crate) username: Option<&'a str>,
    pub(crate) is_platform_admin: Option<bool>,
    pub(crate) is_active: Option<bool>,
    pub(crate) is_verified: Option<bool>,
}

pub(crate) async fn find_by_id(pool: &PgPool, id: &str) -> Result<Option<User>, sqlx::Error> {
    sqlx::query_as::<_, User>(&format!("SELECT {COLUMNS} FROM users WHERE id = $1"))
        .bind(id)
        .fetch_optional(pool)
        .await
}

pub(crate) async fn find_by_username(
    pool: &PgPool,
    username: &str,
) -> Result<Option<User>, sqlx::Error> {
    sqlx::query_as::<_, User>(&format!("SELECT {COLUMNS} FROM users WHERE username = $1"))
        .bind(username)
        .fetch_optional(pool)
        .await
}

pub(crate) async fn exists_by_username(
    pool: &PgPool,
    username: &str,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar::<_, String>("SELECT id FROM users WHERE username = $1")
        .bind(username)
        .fetch_optional(pool)
        .await
}

pub(crate) async fn list(
    pool: &PgPool,
    filters: UserListFilters<'_>,
    skip: i64,
    limit: i64,
) -> Result<Vec<User>, sqlx::Error> {
    let mut builder = QueryBuilder::<Postgres>::new(format!("SELECT {COLUMNS} FROM users"));
    apply_filters(&mut builder, filters);
    builder.push(" ORDER BY created_at DESC");
    builder.push(" OFFSET ");
    builder.push_bind(skip.max(0));
    builder.push(" LIMIT ");
    builder.push_bind(limit.clamp(1, 1000));

    builder.build_query_as::<User>().fetch_all(pool).await
}

pub(crate) async fn count(pool: &PgPool, filters: UserListFilters<'_>) -> Result<i64, sqlx::Error> {
    let mut builder = QueryBuilder::<Postgres>::new("SELECT COUNT(*) FROM users");
    apply_filters(&mut builder, filters);
    builder.build_query_scalar::<i64>().fetch_one(pool).await
}

pub(crate) struct CreateUser<'a> {
    pub id: &'a str,
    pub username: &'a str,
    pub hashed_password: String,
    pub full_name: &'a str,
    pub is_platform_admin: bool,
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
            id, username, hashed_password, full_name, is_platform_admin, is_active, is_verified,
            pd_consent, pd_consent_at, pd_consent_version,
            terms_accepted_at, terms_version, privacy_version,
            created_at, updated_at
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)
        RETURNING {COLUMNS}",
    ))
    .bind(params.id)
    .bind(params.username)
    .bind(params.hashed_password)
    .bind(params.full_name)
    .bind(params.is_platform_admin)
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
    pub is_platform_admin: Option<bool>,
    pub is_active: Option<bool>,
    pub is_verified: Option<bool>,
    pub hashed_password: Option<String>,
    pub updated_at: time::PrimitiveDateTime,
}

pub(crate) async fn update(pool: &PgPool, id: &str, params: UpdateUser) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE users SET
            full_name = COALESCE($1, full_name),
            is_platform_admin = COALESCE($2, is_platform_admin),
            is_active = COALESCE($3, is_active),
            is_verified = COALESCE($4, is_verified),
            hashed_password = COALESCE($5, hashed_password),
            updated_at = $6
         WHERE id = $7",
    )
    .bind(params.full_name)
    .bind(params.is_platform_admin)
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

pub(crate) async fn delete(pool: &PgPool, id: &str) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM users WHERE id = $1").bind(id).execute(pool).await?;
    Ok(result.rows_affected() > 0)
}

fn apply_filters<'a>(builder: &mut QueryBuilder<'a, Postgres>, filters: UserListFilters<'a>) {
    let mut has_where = false;

    if let Some(username) = filters.username {
        push_filter_separator(builder, &mut has_where);
        builder.push("username = ");
        builder.push_bind(username);
    }

    if let Some(is_platform_admin) = filters.is_platform_admin {
        push_filter_separator(builder, &mut has_where);
        builder.push("is_platform_admin = ");
        builder.push_bind(is_platform_admin);
    }

    if let Some(is_active) = filters.is_active {
        push_filter_separator(builder, &mut has_where);
        builder.push("is_active = ");
        builder.push_bind(is_active);
    }

    if let Some(is_verified) = filters.is_verified {
        push_filter_separator(builder, &mut has_where);
        builder.push("is_verified = ");
        builder.push_bind(is_verified);
    }
}

fn push_filter_separator(builder: &mut QueryBuilder<'_, Postgres>, has_where: &mut bool) {
    if *has_where {
        builder.push(" AND ");
    } else {
        builder.push(" WHERE ");
        *has_where = true;
    }
}
