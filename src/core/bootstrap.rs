#![allow(dead_code)]

use time::{OffsetDateTime, PrimitiveDateTime};
use uuid::Uuid;

use crate::core::security;
use crate::core::state::AppState;
use crate::db::models::User;
use crate::db::types::UserRole;

pub(crate) async fn ensure_superuser(state: &AppState) -> anyhow::Result<()> {
    let admin = state.settings().admin();
    if admin.first_superuser_password.is_empty() {
        tracing::warn!("FIRST_SUPERUSER_PASSWORD not configured; skipping superuser creation");
        return Ok(());
    }

    let isu = &admin.first_superuser_isu;

    let user = sqlx::query_as::<_, User>(
        "SELECT id, isu, hashed_password, full_name, role, is_active, is_verified,
                pd_consent, pd_consent_at, pd_consent_version, terms_accepted_at,
                terms_version, privacy_version, created_at, updated_at
         FROM users WHERE isu = $1",
    )
    .bind(isu)
    .fetch_optional(state.db())
    .await?;

    let now_offset = OffsetDateTime::now_utc();
    let now_primitive = primitive_now_utc(now_offset);

    if let Some(user) = user {
        let mut needs_update = false;
        let verified =
            security::verify_password(&admin.first_superuser_password, &user.hashed_password)
                .unwrap_or(false);

        let hashed_password = if verified {
            user.hashed_password.clone()
        } else {
            needs_update = true;
            security::hash_password(&admin.first_superuser_password)?
        };

        let role = if user.role != UserRole::Admin {
            needs_update = true;
            UserRole::Admin
        } else {
            user.role
        };

        let is_active = if !user.is_active {
            needs_update = true;
            true
        } else {
            user.is_active
        };

        let is_verified = if !user.is_verified {
            needs_update = true;
            true
        } else {
            user.is_verified
        };

        if needs_update {
            sqlx::query(
                "UPDATE users
                 SET hashed_password = $1,
                     role = $2,
                     is_active = $3,
                     is_verified = $4,
                     updated_at = $5
                 WHERE id = $6",
            )
            .bind(hashed_password)
            .bind(role)
            .bind(is_active)
            .bind(is_verified)
            .bind(now_primitive)
            .bind(user.id)
            .execute(state.db())
            .await?;

            tracing::info!("Updated default superuser {isu}");
        } else {
            tracing::info!("Default superuser already up to date");
        }

        return Ok(());
    }

    let hashed_password = security::hash_password(&admin.first_superuser_password)?;

    sqlx::query(
        "INSERT INTO users (
            id, isu, hashed_password, full_name, role, is_active, is_verified, created_at, updated_at
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(isu)
    .bind(hashed_password)
    .bind("Super Admin")
    .bind(UserRole::Admin)
    .bind(true)
    .bind(true)
    .bind(now_primitive)
    .bind(now_primitive)
    .execute(state.db())
    .await?;

    tracing::info!("Created default superuser {isu}");
    Ok(())
}

fn primitive_now_utc(offset: OffsetDateTime) -> PrimitiveDateTime {
    PrimitiveDateTime::new(offset.date(), offset.time())
}
