use anyhow::Context;
use uuid::Uuid;

use crate::core::security;
use crate::core::state::AppState;
use crate::core::time::primitive_now_utc;
use crate::repositories;

pub(crate) async fn ensure_superuser(state: &AppState) -> anyhow::Result<()> {
    let admin = state.settings().admin();
    if admin.first_superuser_password.is_empty() {
        tracing::warn!("FIRST_SUPERUSER_PASSWORD not configured; skipping superuser creation");
        return Ok(());
    }

    let username = &admin.first_superuser_username;

    let user = repositories::users::find_by_username(state.db(), username).await?;

    let now_primitive = primitive_now_utc();

    if let Some(user) = user {
        let mut needs_update = false;
        let verified =
            security::verify_password(&admin.first_superuser_password, &user.hashed_password)
                .context("Failed to verify default superuser password hash")?;

        let hashed_password = if verified {
            None
        } else {
            needs_update = true;
            Some(security::hash_password(&admin.first_superuser_password)?)
        };

        let is_platform_admin = if !user.is_platform_admin {
            needs_update = true;
            Some(true)
        } else {
            None
        };

        let is_active = if !user.is_active {
            needs_update = true;
            Some(true)
        } else {
            None
        };

        if needs_update {
            repositories::users::update(
                state.db(),
                &user.id,
                repositories::users::UpdateUser {
                    full_name: None,
                    is_platform_admin,
                    is_active,
                    hashed_password,
                    updated_at: now_primitive,
                },
            )
            .await?;

            tracing::info!("Updated default superuser {username}");
        } else {
            tracing::info!("Default superuser already up to date");
        }

        return Ok(());
    }

    let hashed_password = security::hash_password(&admin.first_superuser_password)?;
    let user_id = Uuid::new_v4().to_string();

    repositories::users::create(
        state.db(),
        repositories::users::CreateUser {
            id: &user_id,
            username,
            hashed_password,
            full_name: "Super Admin",
            is_platform_admin: true,
            is_active: true,
            pd_consent: false,
            pd_consent_at: None,
            pd_consent_version: None,
            terms_accepted_at: None,
            terms_version: None,
            privacy_version: None,
            created_at: now_primitive,
            updated_at: now_primitive,
        },
    )
    .await?;

    tracing::info!("Created default superuser {username}");
    Ok(())
}
