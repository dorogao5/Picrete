use axum::{
    extract::{Path, State},
    Json,
};
use rand::rngs::StdRng;
use rand::{seq::SliceRandom, SeedableRng};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::api::errors::ApiError;
use crate::api::guards::{require_course_role, CurrentUser};
use crate::core::state::AppState;
use crate::db::types::{CourseRole, ExamStatus, SessionStatus};
use crate::repositories;
use crate::schemas::submission::{format_primitive, ExamSessionResponse};

pub(in crate::api::submissions) async fn enter_exam(
    Path((course_id, exam_id)): Path<(String, String)>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
) -> Result<Json<ExamSessionResponse>, ApiError> {
    require_course_role(&state, &user, &course_id, CourseRole::Student).await?;

    let exam =
        crate::api::submissions::helpers::fetch_exam(state.db(), &course_id, &exam_id).await?;

    if !matches!(exam.status, ExamStatus::Published | ExamStatus::Active) {
        return Err(ApiError::BadRequest("Exam is not available".to_string()));
    }

    let now = crate::api::submissions::helpers::now_primitive();

    if now < exam.start_time {
        return Err(ApiError::BadRequest("Exam has not started yet".to_string()));
    }
    if now > exam.end_time {
        return Err(ApiError::BadRequest("Exam has ended".to_string()));
    }

    let task_types = repositories::task_types::list_by_exam(state.db(), &course_id, &exam_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch task types"))?;

    let seed = rand::random::<u32>();
    let variant_seed = i32::from_ne_bytes(seed.to_ne_bytes());
    let mut rng = StdRng::seed_from_u64(seed as u64);
    let mut assignments = serde_json::Map::new();

    for task_type in task_types {
        let variants =
            repositories::task_types::list_variants(state.db(), &course_id, &task_type.id)
                .await
                .map_err(|e| ApiError::internal(e, "Failed to fetch variants"))?;

        if variants.is_empty() {
            return Err(ApiError::BadRequest(format!(
                "Task type '{}' has no variants configured",
                task_type.title
            )));
        }

        if let Some(variant) = variants.choose(&mut rng) {
            assignments.insert(task_type.id.clone(), serde_json::Value::String(variant.id.clone()));
        }
    }

    let expires_candidate = now + Duration::minutes(exam.duration_minutes as i64);
    let expires_at =
        if expires_candidate > exam.end_time { exam.end_time } else { expires_candidate };

    let mut tx = state
        .db()
        .begin()
        .await
        .map_err(|e| ApiError::internal(e, "Failed to start transaction"))?;

    repositories::sessions::acquire_exam_user_lock(&mut *tx, &course_id, &exam_id, &user.id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to acquire session lock"))?;

    let existing = repositories::sessions::find_active(&mut *tx, &course_id, &exam_id, &user.id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch session"))?;

    if let Some(session) = existing {
        tx.commit().await.map_err(|e| ApiError::internal(e, "Failed to commit transaction"))?;
        return Ok(Json(crate::api::submissions::helpers::session_to_response(session)));
    }

    repositories::sessions::acquire_global_lock(&mut *tx, "exam_sessions_active_capacity")
        .await
        .map_err(|e| ApiError::internal(e, "Failed to acquire capacity lock"))?;

    let active_sessions = repositories::sessions::count_active(&mut *tx)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to count active sessions"))?;
    let max_concurrent = state.settings().exam().max_concurrent_exams as i64;
    if active_sessions >= max_concurrent {
        return Err(ApiError::ServiceUnavailable(
            "Exam service is temporarily at capacity. Try again in a few minutes.".to_string(),
        ));
    }

    let attempts =
        repositories::sessions::count_by_exam_and_student(&mut *tx, &course_id, &exam_id, &user.id)
            .await
            .map_err(|e| ApiError::internal(e, "Failed to count attempts"))?;

    if attempts >= exam.max_attempts as i64 {
        return Err(ApiError::BadRequest("Maximum attempts reached".to_string()));
    }

    let session_id = Uuid::new_v4().to_string();
    let inserted = repositories::sessions::create(
        &mut *tx,
        repositories::sessions::CreateSession {
            id: &session_id,
            course_id: &course_id,
            exam_id: &exam_id,
            student_id: &user.id,
            variant_seed,
            variant_assignments: serde_json::Value::Object(assignments.clone()),
            started_at: now,
            expires_at,
            status: SessionStatus::Active,
            attempt_number: (attempts + 1) as i32,
            created_at: now,
            updated_at: now,
        },
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to create session"))?;

    if !inserted {
        let existing =
            repositories::sessions::find_active(&mut *tx, &course_id, &exam_id, &user.id)
                .await
                .map_err(|e| ApiError::internal(e, "Failed to fetch session"))?
                .ok_or_else(|| {
                    ApiError::Conflict("An active session already exists for this exam".to_string())
                })?;
        tx.commit().await.map_err(|e| ApiError::internal(e, "Failed to commit transaction"))?;
        return Ok(Json(crate::api::submissions::helpers::session_to_response(existing)));
    }

    tx.commit().await.map_err(|e| ApiError::internal(e, "Failed to commit transaction"))?;

    let session = repositories::sessions::fetch_one_by_id(state.db(), &course_id, &session_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch session"))?;

    Ok(Json(crate::api::submissions::helpers::session_to_response(session)))
}

pub(in crate::api::submissions) async fn get_session_variant(
    Path((course_id, session_id)): Path<(String, String)>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_course_role(&state, &user, &course_id, CourseRole::Student).await?;
    let session =
        crate::api::submissions::helpers::fetch_session(state.db(), &course_id, &session_id)
            .await?;

    if session.student_id != user.id {
        return Err(ApiError::Forbidden("Access denied"));
    }
    let (hard_deadline, session_status) =
        crate::api::submissions::helpers::enforce_deadline(&session, state.db()).await?;
    if session_status != SessionStatus::Active {
        return Err(ApiError::BadRequest("Session is not active".to_string()));
    }
    if OffsetDateTime::now_utc().unix_timestamp() >= hard_deadline.assume_utc().unix_timestamp() {
        return Err(ApiError::BadRequest("Session has expired".to_string()));
    }

    let tasks = crate::api::submissions::helpers::build_task_context_from_assignments(
        state.db(),
        &course_id,
        &session.exam_id,
        &session.variant_assignments.0,
    )
    .await?;

    let remaining_seconds =
        hard_deadline.assume_utc().unix_timestamp() - OffsetDateTime::now_utc().unix_timestamp();
    let remaining = if remaining_seconds < 0 { 0 } else { remaining_seconds };

    // Include already-uploaded images so the client can restore state after refresh
    let submission_id_opt =
        repositories::submissions::find_id_by_session(state.db(), &course_id, &session_id)
            .await
            .map_err(|e| ApiError::internal(e, "Failed to fetch submission"))?;
    let existing_images: Vec<serde_json::Value> = match submission_id_opt {
        Some(submission_id) => {
            let images = crate::api::submissions::helpers::fetch_images(
                state.db(),
                &course_id,
                &submission_id,
            )
            .await?;
            images
                .into_iter()
                .map(|img| {
                    serde_json::json!({
                        "id": img.id,
                        "filename": img.filename,
                        "order_index": img.order_index,
                        "file_size": img.file_size,
                        "mime_type": img.mime_type,
                    })
                })
                .collect()
        }
        None => vec![],
    };

    Ok(Json(serde_json::json!({
        "session": crate::api::submissions::helpers::session_to_response(session),
        "tasks": tasks,
        "time_remaining": remaining,
        "existing_images": existing_images,
    })))
}

pub(in crate::api::submissions) async fn auto_save(
    Path((course_id, session_id)): Path<(String, String)>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
    Json(payload): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_course_role(&state, &user, &course_id, CourseRole::Student).await?;

    let session =
        crate::api::submissions::helpers::fetch_session(state.db(), &course_id, &session_id)
            .await?;
    if session.student_id != user.id {
        return Err(ApiError::Forbidden("Access denied"));
    }

    let (hard_deadline, session_status) =
        crate::api::submissions::helpers::enforce_deadline(&session, state.db()).await?;
    if session_status != SessionStatus::Active {
        return Err(ApiError::BadRequest("Session is not active".to_string()));
    }

    if OffsetDateTime::now_utc().unix_timestamp() >= hard_deadline.assume_utc().unix_timestamp() {
        return Err(ApiError::BadRequest("Session has expired".to_string()));
    }

    let configured_interval = state.settings().exam().auto_save_interval_seconds.max(1);
    let rate_key = format!("autosave:{course_id}:{session_id}");
    let allowed = match state.redis().rate_limit(&rate_key, 1, configured_interval).await {
        Ok(value) => value,
        Err(err) => {
            tracing::error!(error = %err, "Failed to check auto-save rate limit");
            false
        }
    };
    if !allowed {
        return Err(ApiError::TooManyRequests("Auto-save rate limit exceeded"));
    }

    let now = crate::api::submissions::helpers::now_primitive();
    repositories::sessions::update_auto_save(state.db(), &course_id, &session_id, payload, now)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to save auto data"))?;

    Ok(Json(serde_json::json!({
        "success": true,
        "last_auto_save": format_primitive(now),
        "message": "Data saved successfully"
    })))
}
