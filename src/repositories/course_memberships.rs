use sqlx::PgPool;
use uuid::Uuid;

use crate::db::types::{CourseRole, MembershipStatus};

#[derive(Debug, Clone)]
pub(crate) struct MembershipView {
    pub(crate) membership_id: String,
    pub(crate) course_id: String,
    pub(crate) course_slug: String,
    pub(crate) course_title: String,
    pub(crate) status: MembershipStatus,
    pub(crate) joined_at: time::PrimitiveDateTime,
    pub(crate) roles: Vec<CourseRole>,
}

#[derive(Debug, sqlx::FromRow)]
struct MembershipBaseRow {
    membership_id: String,
    course_id: String,
    course_slug: String,
    course_title: String,
    status: MembershipStatus,
    joined_at: time::PrimitiveDateTime,
}

pub(crate) struct EnsureMembershipParams<'a> {
    pub(crate) course_id: &'a str,
    pub(crate) user_id: &'a str,
    pub(crate) invited_by: Option<&'a str>,
    pub(crate) identity_payload: serde_json::Value,
    pub(crate) role: CourseRole,
    pub(crate) joined_at: time::PrimitiveDateTime,
}

pub(crate) async fn ensure_membership_with_role(
    pool: &PgPool,
    params: EnsureMembershipParams<'_>,
) -> Result<String, sqlx::Error> {
    let mut tx = pool.begin().await?;

    let existing_id = sqlx::query_scalar::<_, String>(
        "SELECT id
         FROM course_memberships
         WHERE course_id = $1 AND user_id = $2
         FOR UPDATE",
    )
    .bind(params.course_id)
    .bind(params.user_id)
    .fetch_optional(&mut *tx)
    .await?;

    let membership_id = if let Some(id) = existing_id {
        sqlx::query(
            "UPDATE course_memberships
             SET status = $1,
                 invited_by = COALESCE($2, invited_by),
                 identity_payload = CASE
                    WHEN $3::jsonb = '{}'::jsonb THEN identity_payload
                    ELSE $3::jsonb
                 END
             WHERE id = $4",
        )
        .bind(MembershipStatus::Active)
        .bind(params.invited_by)
        .bind(params.identity_payload)
        .bind(&id)
        .execute(&mut *tx)
        .await?;
        id
    } else {
        let id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO course_memberships (
                id, course_id, user_id, status, joined_at, invited_by, identity_payload
             ) VALUES ($1,$2,$3,$4,$5,$6,$7)",
        )
        .bind(&id)
        .bind(params.course_id)
        .bind(params.user_id)
        .bind(MembershipStatus::Active)
        .bind(params.joined_at)
        .bind(params.invited_by)
        .bind(params.identity_payload)
        .execute(&mut *tx)
        .await?;
        id
    };

    sqlx::query(
        "INSERT INTO course_membership_roles (membership_id, role, granted_at)
         VALUES ($1,$2,$3)
         ON CONFLICT (membership_id, role) DO NOTHING",
    )
    .bind(&membership_id)
    .bind(params.role)
    .bind(params.joined_at)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(membership_id)
}

pub(crate) async fn list_for_user(
    pool: &PgPool,
    user_id: &str,
) -> Result<Vec<MembershipView>, sqlx::Error> {
    let base_rows = sqlx::query_as::<_, MembershipBaseRow>(
        "SELECT cm.id AS membership_id,
                cm.course_id,
                c.slug AS course_slug,
                c.title AS course_title,
                cm.status,
                cm.joined_at
         FROM course_memberships cm
         JOIN courses c ON c.id = cm.course_id
         WHERE cm.user_id = $1
         ORDER BY cm.joined_at DESC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    hydrate_roles(pool, base_rows).await
}

pub(crate) async fn find_for_user_course(
    pool: &PgPool,
    user_id: &str,
    course_id: &str,
) -> Result<Option<MembershipView>, sqlx::Error> {
    let base = sqlx::query_as::<_, MembershipBaseRow>(
        "SELECT cm.id AS membership_id,
                cm.course_id,
                c.slug AS course_slug,
                c.title AS course_title,
                cm.status,
                cm.joined_at
         FROM course_memberships cm
         JOIN courses c ON c.id = cm.course_id
         WHERE cm.user_id = $1 AND cm.course_id = $2",
    )
    .bind(user_id)
    .bind(course_id)
    .fetch_optional(pool)
    .await?;

    let Some(base) = base else {
        return Ok(None);
    };

    let roles = list_roles_for_membership(pool, &base.membership_id).await?;
    Ok(Some(MembershipView {
        membership_id: base.membership_id,
        course_id: base.course_id,
        course_slug: base.course_slug,
        course_title: base.course_title,
        status: base.status,
        joined_at: base.joined_at,
        roles,
    }))
}

pub(crate) async fn list_roles_for_membership(
    pool: &PgPool,
    membership_id: &str,
) -> Result<Vec<CourseRole>, sqlx::Error> {
    sqlx::query_scalar::<_, CourseRole>(
        "SELECT role
         FROM course_membership_roles
         WHERE membership_id = $1
         ORDER BY role",
    )
    .bind(membership_id)
    .fetch_all(pool)
    .await
}

async fn hydrate_roles(
    pool: &PgPool,
    base_rows: Vec<MembershipBaseRow>,
) -> Result<Vec<MembershipView>, sqlx::Error> {
    let membership_ids = base_rows.iter().map(|row| row.membership_id.clone()).collect::<Vec<_>>();

    let role_rows = if membership_ids.is_empty() {
        Vec::new()
    } else {
        sqlx::query_as::<_, (String, CourseRole)>(
            "SELECT membership_id, role
             FROM course_membership_roles
             WHERE membership_id = ANY($1)",
        )
        .bind(&membership_ids)
        .fetch_all(pool)
        .await?
    };

    let mut roles_by_membership: std::collections::HashMap<String, Vec<CourseRole>> =
        std::collections::HashMap::new();
    for (membership_id, role) in role_rows {
        roles_by_membership.entry(membership_id).or_default().push(role);
    }

    let mut output = Vec::with_capacity(base_rows.len());
    for row in base_rows {
        output.push(MembershipView {
            membership_id: row.membership_id.clone(),
            course_id: row.course_id,
            course_slug: row.course_slug,
            course_title: row.course_title,
            status: row.status,
            joined_at: row.joined_at,
            roles: roles_by_membership.remove(&row.membership_id).unwrap_or_default(),
        });
    }

    Ok(output)
}
