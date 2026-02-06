#![allow(dead_code)]

use axum::{
    extract::{Multipart, Path, Query, State},
    routing::get,
    routing::post,
    Json, Router,
};
use rand::rngs::StdRng;
use rand::{seq::SliceRandom, SeedableRng};
use serde::Deserialize;
use time::{Duration, OffsetDateTime, PrimitiveDateTime};
use uuid::Uuid;

use crate::api::errors::ApiError;
use crate::api::guards::{CurrentTeacher, CurrentUser};
use crate::core::state::AppState;
use crate::db::models::{
    Exam, ExamSession, Submission, SubmissionImage, SubmissionScore, TaskType, TaskVariant,
};
use crate::db::types::{ExamStatus, SessionStatus, SubmissionStatus, UserRole};
use crate::schemas::submission::{
    format_primitive, ExamSessionResponse, SubmissionApproveRequest, SubmissionImageResponse,
    SubmissionOverrideRequest, SubmissionResponse, SubmissionScoreResponse,
};

#[derive(Debug, Deserialize)]
pub(crate) struct PresignQuery {
    filename: String,
    content_type: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ListSubmissionsQuery {
    #[serde(default)]
    status: Option<SubmissionStatus>,
    #[serde(default)]
    skip: i64,
    #[serde(default = "default_limit")]
    limit: i64,
}

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/my-submissions", get(get_my_submissions))
        .route("/exams/:exam_id/enter", post(enter_exam))
        .route("/sessions/:session_id/variant", get(get_session_variant))
        .route("/sessions/:session_id/presigned-upload-url", post(presigned_upload_url))
        .route("/sessions/:session_id/upload", post(upload_image))
        .route("/sessions/:session_id/auto-save", post(auto_save))
        .route("/sessions/:session_id/submit", post(submit_exam))
        .route("/sessions/:session_id/result", get(get_session_result))
        .route("/:submission_id", get(get_submission))
        .route("/:submission_id/approve", post(approve_submission))
        .route("/:submission_id/override-score", post(override_score))
        .route("/images/:image_id/view-url", get(get_image_view_url))
        .route("/:submission_id/regrade", post(regrade_submission))
        .route("/grading-status/:submission_id", get(grading_status))
}

async fn get_my_submissions(
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<serde_json::Value>>, ApiError> {
    let sessions = sqlx::query_as::<_, ExamSession>(
        "SELECT id, exam_id, student_id, variant_seed, variant_assignments,
                started_at, submitted_at, expires_at, status, attempt_number,
                ip_address, user_agent, last_auto_save, auto_save_data, created_at, updated_at
         FROM exam_sessions WHERE student_id = $1",
    )
    .bind(&user.id)
    .fetch_all(state.db())
    .await
    .map_err(|_| ApiError::Internal("Failed to fetch sessions".to_string()))?;

    let mut response = Vec::new();
    for session in sessions {
        let exam_title: Option<String> =
            sqlx::query_scalar("SELECT title FROM exams WHERE id = $1")
                .bind(&session.exam_id)
                .fetch_optional(state.db())
                .await
                .unwrap_or(None);

        let submission = sqlx::query_as::<_, Submission>(
            "SELECT id, session_id, student_id, submitted_at, status, ai_score, final_score, max_score,
                    ai_analysis, ai_comments, ai_processed_at, ai_request_started_at, ai_request_completed_at,
                    ai_request_duration_seconds, ai_error, ai_retry_count, teacher_comments, reviewed_by,
                    reviewed_at, is_flagged, flag_reasons, anomaly_scores, files_hash, created_at, updated_at
             FROM submissions WHERE session_id = $1",
        )
        .bind(&session.id)
        .fetch_optional(state.db())
        .await
        .map_err(|_| ApiError::Internal("Failed to fetch submission".to_string()))?;

        let (images, scores) = if let Some(ref sub) = submission {
            (fetch_images(state.db(), &sub.id).await?, fetch_scores(state.db(), &sub.id).await?)
        } else {
            (vec![], vec![])
        };

        response.push(serde_json::json!({
            "id": submission.as_ref().map(|s| &s.id),
            "session_id": session.id,
            "exam_id": session.exam_id,
            "exam_title": exam_title.unwrap_or_else(|| "Unknown".to_string()),
            "submitted_at": submission.as_ref().map(|s| format_primitive(s.submitted_at)),
            "status": submission.as_ref().map(|s| &s.status),
            "ai_score": submission.as_ref().and_then(|s| s.ai_score),
            "final_score": submission.as_ref().and_then(|s| s.final_score),
            "max_score": submission.as_ref().map(|s| s.max_score),
            "images": images,
            "scores": scores,
            "teacher_comments": submission.as_ref().and_then(|s| s.teacher_comments.clone()),
        }));
    }

    Ok(Json(response))
}

async fn enter_exam(
    Path(exam_id): Path<String>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
) -> Result<Json<ExamSessionResponse>, ApiError> {
    let exam = fetch_exam(state.db(), &exam_id).await?;

    if !matches!(exam.status, ExamStatus::Published | ExamStatus::Active) {
        return Err(ApiError::BadRequest("Exam is not available".to_string()));
    }

    let now = now_primitive();

    if now < exam.start_time {
        return Err(ApiError::BadRequest("Exam has not started yet".to_string()));
    }
    if now > exam.end_time {
        return Err(ApiError::BadRequest("Exam has ended".to_string()));
    }

    let existing = sqlx::query_as::<_, ExamSession>(
        "SELECT id, exam_id, student_id, variant_seed, variant_assignments,
                started_at, submitted_at, expires_at, status, attempt_number,
                ip_address, user_agent, last_auto_save, auto_save_data, created_at, updated_at
         FROM exam_sessions
         WHERE exam_id = $1 AND student_id = $2 AND status = $3",
    )
    .bind(&exam_id)
    .bind(&user.id)
    .bind(SessionStatus::Active)
    .fetch_optional(state.db())
    .await
    .map_err(|_| ApiError::Internal("Failed to fetch session".to_string()))?;

    if let Some(session) = existing {
        return Ok(Json(session_to_response(session)));
    }

    let attempts: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM exam_sessions WHERE exam_id = $1 AND student_id = $2",
    )
    .bind(&exam_id)
    .bind(&user.id)
    .fetch_one(state.db())
    .await
    .unwrap_or(0);

    if attempts >= exam.max_attempts as i64 {
        return Err(ApiError::BadRequest("Maximum attempts reached".to_string()));
    }

    let task_types = sqlx::query_as::<_, TaskType>(
        "SELECT id, exam_id, title, description, order_index, max_score, rubric,
                difficulty, taxonomy_tags, formulas, units, validation_rules,
                created_at, updated_at
         FROM task_types WHERE exam_id = $1",
    )
    .bind(&exam_id)
    .fetch_all(state.db())
    .await
    .map_err(|_| ApiError::Internal("Failed to fetch task types".to_string()))?;

    let seed = rand::random::<u32>();
    let mut rng = StdRng::seed_from_u64(seed as u64);
    let mut assignments = serde_json::Map::new();

    for task_type in task_types {
        let variants = sqlx::query_as::<_, TaskVariant>(
            "SELECT id, task_type_id, content, parameters, reference_solution,
                    reference_answer, answer_tolerance, attachments, created_at
             FROM task_variants WHERE task_type_id = $1",
        )
        .bind(&task_type.id)
        .fetch_all(state.db())
        .await
        .map_err(|_| ApiError::Internal("Failed to fetch variants".to_string()))?;

        if let Some(variant) = variants.choose(&mut rng) {
            assignments.insert(task_type.id.clone(), serde_json::Value::String(variant.id.clone()));
        }
    }

    let expires_candidate = now + Duration::minutes(exam.duration_minutes as i64);
    let expires_at =
        if expires_candidate > exam.end_time { exam.end_time } else { expires_candidate };

    let session_id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO exam_sessions (
            id, exam_id, student_id, variant_seed, variant_assignments,
            started_at, expires_at, status, attempt_number, created_at, updated_at
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
    )
    .bind(&session_id)
    .bind(&exam_id)
    .bind(&user.id)
    .bind(seed as i32)
    .bind(serde_json::Value::Object(assignments.clone()))
    .bind(now)
    .bind(expires_at)
    .bind(SessionStatus::Active)
    .bind((attempts + 1) as i32)
    .bind(now)
    .bind(now)
    .execute(state.db())
    .await
    .map_err(|_| ApiError::Internal("Failed to create session".to_string()))?;

    let session = sqlx::query_as::<_, ExamSession>(
        "SELECT id, exam_id, student_id, variant_seed, variant_assignments,
                started_at, submitted_at, expires_at, status, attempt_number,
                ip_address, user_agent, last_auto_save, auto_save_data, created_at, updated_at
         FROM exam_sessions WHERE id = $1",
    )
    .bind(&session_id)
    .fetch_one(state.db())
    .await
    .map_err(|_| ApiError::Internal("Failed to fetch session".to_string()))?;

    Ok(Json(session_to_response(session)))
}

async fn get_session_variant(
    Path(session_id): Path<String>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let session = fetch_session(state.db(), &session_id).await?;

    if session.student_id != user.id {
        return Err(ApiError::Forbidden("Access denied"));
    }

    let task_types = fetch_task_types(state.db(), &session.exam_id).await?;
    let mut tasks = Vec::new();

    let assignments = session.variant_assignments.0.clone();

    for task_type in task_types {
        let variants = sqlx::query_as::<_, TaskVariant>(
            "SELECT id, task_type_id, content, parameters, reference_solution,
                    reference_answer, answer_tolerance, attachments, created_at
             FROM task_variants WHERE task_type_id = $1",
        )
        .bind(&task_type.id)
        .fetch_all(state.db())
        .await
        .map_err(|_| ApiError::Internal("Failed to fetch variants".to_string()))?;

        if let Some(variant_id) = assignments.get(&task_type.id) {
            if let Some(variant) = variants.into_iter().find(|v| &v.id == variant_id) {
                tasks.push(serde_json::json!({
                    "task_type": {
                        "id": task_type.id,
                        "title": task_type.title,
                        "description": task_type.description,
                        "order_index": task_type.order_index,
                        "max_score": task_type.max_score,
                        "formulas": task_type.formulas.0,
                        "units": task_type.units.0,
                    },
                    "variant": {
                        "id": variant.id,
                        "content": variant.content,
                        "parameters": variant.parameters.0,
                        "attachments": variant.attachments.0,
                    }
                }));
            }
        }
    }

    let remaining = if session.status == SessionStatus::Active {
        let remaining_seconds = session.expires_at.assume_utc().unix_timestamp()
            - OffsetDateTime::now_utc().unix_timestamp();
        if remaining_seconds < 0 {
            0
        } else {
            remaining_seconds
        }
    } else {
        0
    };

    Ok(Json(serde_json::json!({
        "session": session_to_response(session),
        "tasks": tasks,
        "time_remaining": remaining,
    })))
}

async fn presigned_upload_url(
    Path(session_id): Path<String>,
    Query(query): Query<PresignQuery>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let session = fetch_session(state.db(), &session_id).await?;
    if session.student_id != user.id {
        return Err(ApiError::Forbidden("Access denied"));
    }

    let (hard_deadline, session_status) = enforce_deadline(&session, state.db()).await?;
    if session_status != SessionStatus::Active {
        return Err(ApiError::BadRequest("Session is not active".to_string()));
    }

    if OffsetDateTime::now_utc().unix_timestamp() >= hard_deadline.assume_utc().unix_timestamp() {
        return Err(ApiError::BadRequest("Session has expired".to_string()));
    }

    if !matches!(query.content_type.as_str(), "image/jpeg" | "image/png") {
        return Err(ApiError::BadRequest("Only JPEG and PNG images are allowed".to_string()));
    }

    let storage = state.storage().ok_or_else(|| {
        ApiError::BadRequest(
            "Direct upload not available. Use standard upload endpoint.".to_string(),
        )
    })?;

    let filename = sanitized_filename(&query.filename);
    let key = format!("submissions/{}/{}", session_id, filename);
    let expires =
        std::time::Duration::from_secs(state.settings().exam().presigned_url_expire_minutes * 60);
    let presigned = storage
        .presign_put(&key, &query.content_type, expires)
        .await
        .map_err(|_| ApiError::Internal("Failed to generate upload URL".to_string()))?;

    Ok(Json(serde_json::json!({
        "upload_url": presigned,
        "s3_key": key,
        "method": "PUT",
        "headers": {"Content-Type": query.content_type}
    })))
}

async fn upload_image(
    Path(session_id): Path<String>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, ApiError> {
    let session = fetch_session(state.db(), &session_id).await?;
    if session.student_id != user.id {
        return Err(ApiError::Forbidden("Access denied"));
    }

    let (hard_deadline, session_status) = enforce_deadline(&session, state.db()).await?;
    if session_status != SessionStatus::Active {
        return Err(ApiError::BadRequest("Session is not active".to_string()));
    }
    if OffsetDateTime::now_utc().unix_timestamp() >= hard_deadline.assume_utc().unix_timestamp() {
        return Err(ApiError::BadRequest("Session has expired".to_string()));
    }

    let storage = state.storage().ok_or_else(|| {
        ApiError::BadRequest(
            "S3 storage is not configured. Please configure Yandex Object Storage.".to_string(),
        )
    })?;
    let submission_id = ensure_submission(state.db(), &session).await?;
    let current_images: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM submission_images WHERE submission_id = $1")
            .bind(&submission_id)
            .fetch_one(state.db())
            .await
            .map_err(|_| ApiError::Internal("Failed to count submission images".to_string()))?;

    let max_images = state.settings().storage().max_images_per_submission as i64;
    if current_images >= max_images {
        return Err(ApiError::BadRequest(format!(
            "Maximum number of images per submission exceeded ({max_images})"
        )));
    }

    let mut file_bytes: Option<Vec<u8>> = None;
    let mut filename: Option<String> = None;
    let mut content_type: Option<String> = None;
    let mut order_index: Option<i32> = None;
    let max_bytes = state.settings().storage().max_upload_size_mb * 1024 * 1024;

    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|_| ApiError::BadRequest("Invalid multipart data".to_string()))?
    {
        let name = field.name().unwrap_or("").to_string();
        if name == "file" {
            filename = field.file_name().map(|s| s.to_string());
            content_type = field.content_type().map(|s| s.to_string());
            let mut bytes = Vec::new();
            while let Some(chunk) = field
                .chunk()
                .await
                .map_err(|_| ApiError::BadRequest("Failed to read file".to_string()))?
            {
                let next_size = bytes.len() as u64 + chunk.len() as u64;
                if next_size > max_bytes {
                    return Err(ApiError::BadRequest(format!(
                        "File size exceeds {}MB limit",
                        state.settings().storage().max_upload_size_mb
                    )));
                }
                bytes.extend_from_slice(&chunk);
            }
            file_bytes = Some(bytes);
        } else if name == "order_index" {
            let text = field
                .text()
                .await
                .map_err(|_| ApiError::BadRequest("Invalid order index".to_string()))?;
            order_index = text.parse::<i32>().ok();
        }
    }

    let file_bytes =
        file_bytes.ok_or_else(|| ApiError::BadRequest("File is required".to_string()))?;
    let filename = filename.unwrap_or_else(|| "image.jpg".to_string());
    let content_type = content_type.unwrap_or_else(|| "application/octet-stream".to_string());
    let order_index =
        order_index.ok_or_else(|| ApiError::BadRequest("order_index is required".to_string()))?;

    if !matches!(content_type.as_str(), "image/jpeg" | "image/png") {
        return Err(ApiError::BadRequest("Only JPEG and PNG images are allowed".to_string()));
    }

    if file_bytes.len() as u64 > max_bytes {
        return Err(ApiError::BadRequest(format!(
            "File size exceeds {}MB limit",
            state.settings().storage().max_upload_size_mb
        )));
    }

    let image_id = Uuid::new_v4().to_string();
    let key = format!("submissions/{}/{}_{}", session_id, image_id, sanitized_filename(&filename));

    let (file_size, _hash) = storage
        .upload_bytes(&key, &content_type, file_bytes)
        .await
        .map_err(|_| ApiError::Internal("Failed to upload file to S3".to_string()))?;

    sqlx::query(
        "INSERT INTO submission_images (
            id, submission_id, filename, file_path, file_size, mime_type,
            order_index, is_processed, uploaded_at
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
    )
    .bind(&image_id)
    .bind(&submission_id)
    .bind(&filename)
    .bind(&key)
    .bind(file_size)
    .bind(&content_type)
    .bind(order_index)
    .bind(false)
    .bind(now_primitive())
    .execute(state.db())
    .await
    .map_err(|_| ApiError::Internal("Failed to store image metadata".to_string()))?;

    Ok(Json(serde_json::json!({
        "message": "File uploaded successfully",
        "image_id": image_id
    })))
}

async fn auto_save(
    Path(session_id): Path<String>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
    Json(payload): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let rate_key = format!("autosave:{session_id}");
    let allowed = state.redis().rate_limit(&rate_key, 1, 5).await.unwrap_or(true);

    if !allowed {
        return Err(ApiError::BadRequest("Auto-save rate limit exceeded".to_string()));
    }

    let session = fetch_session(state.db(), &session_id).await?;
    if session.student_id != user.id {
        return Err(ApiError::Forbidden("Access denied"));
    }

    let (hard_deadline, session_status) = enforce_deadline(&session, state.db()).await?;
    if session_status != SessionStatus::Active {
        return Err(ApiError::BadRequest("Session is not active".to_string()));
    }

    if OffsetDateTime::now_utc().unix_timestamp() >= hard_deadline.assume_utc().unix_timestamp() {
        return Err(ApiError::BadRequest("Session has expired".to_string()));
    }

    let now = now_primitive();
    sqlx::query("UPDATE exam_sessions SET auto_save_data = $1, last_auto_save = $2 WHERE id = $3")
        .bind(payload)
        .bind(now)
        .bind(&session_id)
        .execute(state.db())
        .await
        .map_err(|_| ApiError::Internal("Failed to save auto data".to_string()))?;

    Ok(Json(serde_json::json!({
        "success": true,
        "last_auto_save": format_primitive(now),
        "message": "Data saved successfully"
    })))
}

async fn submit_exam(
    Path(session_id): Path<String>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
) -> Result<Json<SubmissionResponse>, ApiError> {
    let session = fetch_session(state.db(), &session_id).await?;
    if session.student_id != user.id {
        return Err(ApiError::Forbidden("Access denied"));
    }

    let (hard_deadline, session_status) = enforce_deadline(&session, state.db()).await?;
    let now_offset = OffsetDateTime::now_utc();
    let now = now_primitive();
    let recently_expired =
        now_offset.unix_timestamp() <= hard_deadline.assume_utc().unix_timestamp() + 300;

    if session_status != SessionStatus::Active && !recently_expired {
        return Err(ApiError::BadRequest("Session is not active or has expired".to_string()));
    }

    let mut submission = sqlx::query_as::<_, Submission>(
        "SELECT id, session_id, student_id, submitted_at, status, ai_score, final_score, max_score,
                ai_analysis, ai_comments, ai_processed_at, ai_request_started_at, ai_request_completed_at,
                ai_request_duration_seconds, ai_error, ai_retry_count, teacher_comments, reviewed_by,
                reviewed_at, is_flagged, flag_reasons, anomaly_scores, files_hash, created_at, updated_at
         FROM submissions WHERE session_id = $1",
    )
    .bind(&session_id)
    .fetch_optional(state.db())
    .await
    .map_err(|_| ApiError::Internal("Failed to fetch submission".to_string()))?;

    if submission.is_none() {
        let max_score: f64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(max_score), 100) FROM task_types WHERE exam_id = $1",
        )
        .bind(&session.exam_id)
        .fetch_one(state.db())
        .await
        .unwrap_or(100.0);

        let submission_id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO submissions (id, session_id, student_id, status, max_score, submitted_at, created_at, updated_at)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
        )
        .bind(&submission_id)
        .bind(&session_id)
        .bind(&session.student_id)
        .bind(SubmissionStatus::Uploaded)
        .bind(max_score)
        .bind(now)
        .bind(now)
        .bind(now)
        .execute(state.db())
        .await
        .map_err(|_| ApiError::Internal("Failed to create submission".to_string()))?;

        submission = sqlx::query_as::<_, Submission>(
            "SELECT id, session_id, student_id, submitted_at, status, ai_score, final_score, max_score,
                    ai_analysis, ai_comments, ai_processed_at, ai_request_started_at, ai_request_completed_at,
                    ai_request_duration_seconds, ai_error, ai_retry_count, teacher_comments, reviewed_by,
                    reviewed_at, is_flagged, flag_reasons, anomaly_scores, files_hash, created_at, updated_at
             FROM submissions WHERE id = $1",
        )
        .bind(&submission_id)
        .fetch_optional(state.db())
        .await
        .map_err(|_| ApiError::Internal("Failed to fetch submission".to_string()))?;
    }

    sqlx::query("UPDATE exam_sessions SET status = $1, submitted_at = $2 WHERE id = $3")
        .bind(SessionStatus::Submitted)
        .bind(now)
        .bind(&session_id)
        .execute(state.db())
        .await
        .map_err(|_| ApiError::Internal("Failed to update session".to_string()))?;

    sqlx::query("UPDATE submissions SET status = $1, submitted_at = $2 WHERE session_id = $3")
        .bind(SubmissionStatus::Uploaded)
        .bind(now)
        .bind(&session_id)
        .execute(state.db())
        .await
        .map_err(|_| ApiError::Internal("Failed to update submission".to_string()))?;

    let submission =
        submission.ok_or_else(|| ApiError::Internal("Submission missing".to_string()))?;
    let images = fetch_images(state.db(), &submission.id).await?;
    let scores = fetch_scores(state.db(), &submission.id).await?;

    Ok(Json(to_submission_response(submission, images, scores)))
}

async fn get_session_result(
    Path(session_id): Path<String>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let session = fetch_session(state.db(), &session_id).await?;
    if session.student_id != user.id {
        return Err(ApiError::Forbidden("Access denied"));
    }

    let submission = sqlx::query_as::<_, Submission>(
        "SELECT id, session_id, student_id, submitted_at, status, ai_score, final_score, max_score,
                ai_analysis, ai_comments, ai_processed_at, ai_request_started_at, ai_request_completed_at,
                ai_request_duration_seconds, ai_error, ai_retry_count, teacher_comments, reviewed_by,
                reviewed_at, is_flagged, flag_reasons, anomaly_scores, files_hash, created_at, updated_at
         FROM submissions WHERE session_id = $1",
    )
    .bind(&session_id)
    .fetch_optional(state.db())
    .await
    .map_err(|_| ApiError::Internal("Failed to fetch submission".to_string()))?;

    let Some(submission) = submission else {
        return Err(ApiError::BadRequest("No submission found for this session".to_string()));
    };

    let attempts: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM exam_sessions WHERE exam_id = $1 AND student_id = $2",
    )
    .bind(&session.exam_id)
    .bind(&user.id)
    .fetch_one(state.db())
    .await
    .unwrap_or(0);
    let exam = fetch_exam(state.db(), &session.exam_id).await?;

    let images = fetch_images(state.db(), &submission.id).await?;
    let scores = fetch_scores(state.db(), &submission.id).await?;

    Ok(Json(serde_json::json!({
        "id": submission.id,
        "session_id": submission.session_id,
        "student_id": submission.student_id,
        "submitted_at": format_primitive(submission.submitted_at),
        "status": submission.status,
        "ai_score": submission.ai_score,
        "final_score": submission.final_score,
        "max_score": submission.max_score,
        "ai_analysis": submission.ai_analysis.map(|v| v.0),
        "ai_comments": submission.ai_comments,
        "teacher_comments": submission.teacher_comments,
        "is_flagged": submission.is_flagged,
        "flag_reasons": submission.flag_reasons.0,
        "reviewed_by": submission.reviewed_by,
        "reviewed_at": submission.reviewed_at.map(format_primitive),
        "images": images,
        "scores": scores,
        "exam": {
            "id": session.exam_id,
            "title": exam.title,
            "max_attempts": exam.max_attempts,
        },
        "session": {
            "id": session.id,
            "attempt_number": session.attempt_number,
            "total_attempts": attempts,
        }
    })))
}

async fn get_submission(
    Path(submission_id): Path<String>,
    CurrentTeacher(_teacher): CurrentTeacher,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let submission = sqlx::query_as::<_, Submission>(
        "SELECT id, session_id, student_id, submitted_at, status, ai_score, final_score, max_score,
                ai_analysis, ai_comments, ai_processed_at, ai_request_started_at, ai_request_completed_at,
                ai_request_duration_seconds, ai_error, ai_retry_count, teacher_comments, reviewed_by,
                reviewed_at, is_flagged, flag_reasons, anomaly_scores, files_hash, created_at, updated_at
         FROM submissions WHERE id = $1",
    )
    .bind(&submission_id)
    .fetch_optional(state.db())
    .await
    .map_err(|_| ApiError::Internal("Failed to fetch submission".to_string()))?;

    let Some(submission) = submission else {
        return Err(ApiError::BadRequest("Submission not found".to_string()));
    };

    let session = fetch_session(state.db(), &submission.session_id).await?;
    let exam = fetch_exam(state.db(), &session.exam_id).await?;

    let student = sqlx::query_scalar::<_, String>("SELECT full_name FROM users WHERE id = $1")
        .bind(&submission.student_id)
        .fetch_optional(state.db())
        .await
        .unwrap_or(None);
    let student_isu = sqlx::query_scalar::<_, String>("SELECT isu FROM users WHERE id = $1")
        .bind(&submission.student_id)
        .fetch_optional(state.db())
        .await
        .unwrap_or(None);

    let images = fetch_images(state.db(), &submission.id).await?;
    let scores = fetch_scores(state.db(), &submission.id).await?;

    let tasks_payload = build_task_context(state.db(), &session).await?;

    Ok(Json(serde_json::json!({
        "id": submission.id,
        "session_id": submission.session_id,
        "student_id": submission.student_id,
        "submitted_at": format_primitive(submission.submitted_at),
        "status": submission.status,
        "ai_score": submission.ai_score,
        "final_score": submission.final_score,
        "max_score": submission.max_score,
        "ai_analysis": submission.ai_analysis.map(|v| v.0),
        "ai_comments": submission.ai_comments,
        "teacher_comments": submission.teacher_comments,
        "is_flagged": submission.is_flagged,
        "flag_reasons": submission.flag_reasons.0,
        "reviewed_by": submission.reviewed_by,
        "reviewed_at": submission.reviewed_at.map(format_primitive),
        "images": images,
        "scores": scores,
        "student_name": student,
        "student_isu": student_isu,
        "exam": {"id": exam.id, "title": exam.title},
        "tasks": tasks_payload,
    })))
}

async fn approve_submission(
    Path(submission_id): Path<String>,
    CurrentTeacher(teacher): CurrentTeacher,
    State(state): State<AppState>,
    Json(payload): Json<SubmissionApproveRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let submission = sqlx::query_as::<_, Submission>(
        "SELECT id, session_id, student_id, submitted_at, status, ai_score, final_score, max_score,
                ai_analysis, ai_comments, ai_processed_at, ai_request_started_at, ai_request_completed_at,
                ai_request_duration_seconds, ai_error, ai_retry_count, teacher_comments, reviewed_by,
                reviewed_at, is_flagged, flag_reasons, anomaly_scores, files_hash, created_at, updated_at
         FROM submissions WHERE id = $1",
    )
    .bind(&submission_id)
    .fetch_optional(state.db())
    .await
    .map_err(|_| ApiError::Internal("Failed to fetch submission".to_string()))?;

    let Some(submission) = submission else {
        return Err(ApiError::BadRequest("Submission not found".to_string()));
    };

    if submission.ai_score.is_none() {
        return Err(ApiError::BadRequest(
            "Cannot approve: AI has not finished grading yet. Use override-score to set a manual score.".to_string()
        ));
    }

    let now = now_primitive();
    sqlx::query(
        "UPDATE submissions SET status = $1, final_score = $2, teacher_comments = $3,
            reviewed_by = $4, reviewed_at = $5 WHERE id = $6",
    )
    .bind(SubmissionStatus::Approved)
    .bind(submission.ai_score)
    .bind(payload.teacher_comments)
    .bind(teacher.id)
    .bind(now)
    .bind(&submission_id)
    .execute(state.db())
    .await
    .map_err(|_| ApiError::Internal("Failed to approve submission".to_string()))?;

    Ok(Json(serde_json::json!({"message": "Submission approved"})))
}

async fn override_score(
    Path(submission_id): Path<String>,
    CurrentTeacher(teacher): CurrentTeacher,
    State(state): State<AppState>,
    Json(payload): Json<SubmissionOverrideRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let submission = sqlx::query_as::<_, Submission>(
        "SELECT id, session_id, student_id, submitted_at, status, ai_score, final_score, max_score,
                ai_analysis, ai_comments, ai_processed_at, ai_request_started_at, ai_request_completed_at,
                ai_request_duration_seconds, ai_error, ai_retry_count, teacher_comments, reviewed_by,
                reviewed_at, is_flagged, flag_reasons, anomaly_scores, files_hash, created_at, updated_at
         FROM submissions WHERE id = $1",
    )
    .bind(&submission_id)
    .fetch_optional(state.db())
    .await
    .map_err(|_| ApiError::Internal("Failed to fetch submission".to_string()))?;

    let Some(_submission) = submission else {
        return Err(ApiError::BadRequest("Submission not found".to_string()));
    };

    let now = now_primitive();
    sqlx::query(
        "UPDATE submissions SET final_score = $1, teacher_comments = $2,
            status = $3, reviewed_by = $4, reviewed_at = $5 WHERE id = $6",
    )
    .bind(payload.final_score)
    .bind(payload.teacher_comments)
    .bind(SubmissionStatus::Approved)
    .bind(teacher.id)
    .bind(now)
    .bind(&submission_id)
    .execute(state.db())
    .await
    .map_err(|_| ApiError::Internal("Failed to override score".to_string()))?;

    Ok(Json(serde_json::json!({"message": "Score overridden successfully"})))
}

async fn get_image_view_url(
    Path(image_id): Path<String>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let image = sqlx::query_as::<_, SubmissionImage>(
        "SELECT id, submission_id, filename, file_path, file_size, mime_type,
                is_processed, ocr_text, quality_score, order_index, perceptual_hash,
                uploaded_at, processed_at
         FROM submission_images WHERE id = $1",
    )
    .bind(&image_id)
    .fetch_optional(state.db())
    .await
    .map_err(|_| ApiError::Internal("Failed to fetch image".to_string()))?;

    let Some(image) = image else {
        return Err(ApiError::BadRequest("Image not found".to_string()));
    };

    let submission = sqlx::query_as::<_, Submission>(
        "SELECT id, session_id, student_id, submitted_at, status, ai_score, final_score, max_score,
                ai_analysis, ai_comments, ai_processed_at, ai_request_started_at, ai_request_completed_at,
                ai_request_duration_seconds, ai_error, ai_retry_count, teacher_comments, reviewed_by,
                reviewed_at, is_flagged, flag_reasons, anomaly_scores, files_hash, created_at, updated_at
         FROM submissions WHERE id = $1",
    )
    .bind(&image.submission_id)
    .fetch_one(state.db())
    .await
    .map_err(|_| ApiError::Internal("Failed to fetch submission".to_string()))?;

    let is_owner = submission.student_id == user.id;
    let is_teacher = matches!(user.role, UserRole::Teacher | UserRole::Admin);

    if !is_owner && !is_teacher {
        return Err(ApiError::Forbidden("Access denied"));
    }

    if !image.file_path.starts_with("submissions/") {
        return Err(ApiError::BadRequest(
            "Image is stored in local storage. Please migrate to S3 storage.".to_string(),
        ));
    }

    let storage = state
        .storage()
        .ok_or_else(|| ApiError::BadRequest("S3 storage not configured".to_string()))?;

    let url = storage
        .presign_get(&image.file_path, std::time::Duration::from_secs(300))
        .await
        .map_err(|_| ApiError::Internal("Failed to generate view URL".to_string()))?;

    Ok(Json(serde_json::json!({
        "view_url": url,
        "expires_in": 300,
        "filename": image.filename,
        "mime_type": image.mime_type,
    })))
}

async fn regrade_submission(
    Path(submission_id): Path<String>,
    CurrentTeacher(teacher): CurrentTeacher,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let submission = sqlx::query_as::<_, Submission>(
        "SELECT id, session_id, student_id, submitted_at, status, ai_score, final_score, max_score,
                ai_analysis, ai_comments, ai_processed_at, ai_request_started_at, ai_request_completed_at,
                ai_request_duration_seconds, ai_error, ai_retry_count, teacher_comments, reviewed_by,
                reviewed_at, is_flagged, flag_reasons, anomaly_scores, files_hash, created_at, updated_at
         FROM submissions WHERE id = $1",
    )
    .bind(&submission_id)
    .fetch_optional(state.db())
    .await
    .map_err(|_| ApiError::Internal("Failed to fetch submission".to_string()))?;

    let Some(_submission) = submission else {
        return Err(ApiError::BadRequest("Submission not found".to_string()));
    };

    sqlx::query(
        "UPDATE submissions SET status = $1,
            ai_retry_count = COALESCE(ai_retry_count,0) + 1,
            ai_error = NULL,
            ai_request_started_at = NULL,
            ai_request_completed_at = NULL,
            ai_request_duration_seconds = NULL,
            ai_processed_at = NULL,
            is_flagged = FALSE,
            flag_reasons = $2,
            updated_at = $3
         WHERE id = $4",
    )
    .bind(SubmissionStatus::Processing)
    .bind(sqlx::types::Json(Vec::<String>::new()))
    .bind(now_primitive())
    .bind(&submission_id)
    .execute(state.db())
    .await
    .map_err(|_| ApiError::Internal("Failed to update submission".to_string()))?;

    tracing::info!(
        teacher_id = %teacher.id,
        submission_id = %submission_id,
        action = "submission_regrade",
        "Submission regrade queued"
    );

    Ok(Json(serde_json::json!({
        "message": "Re-grading queued successfully",
        "submission_id": submission_id,
        "task_id": null,
        "status": "processing"
    })))
}

async fn grading_status(
    Path(submission_id): Path<String>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let submission = sqlx::query_as::<_, Submission>(
        "SELECT id, session_id, student_id, submitted_at, status, ai_score, final_score, max_score,
                ai_analysis, ai_comments, ai_processed_at, ai_request_started_at, ai_request_completed_at,
                ai_request_duration_seconds, ai_error, ai_retry_count, teacher_comments, reviewed_by,
                reviewed_at, is_flagged, flag_reasons, anomaly_scores, files_hash, created_at, updated_at
         FROM submissions WHERE id = $1",
    )
    .bind(&submission_id)
    .fetch_optional(state.db())
    .await
    .map_err(|_| ApiError::Internal("Failed to fetch submission".to_string()))?;

    let Some(submission) = submission else {
        return Err(ApiError::BadRequest("Submission not found".to_string()));
    };

    let is_owner = submission.student_id == user.id;
    let is_teacher = matches!(user.role, UserRole::Teacher | UserRole::Admin);

    if !is_owner && !is_teacher {
        return Err(ApiError::Forbidden("Access denied"));
    }

    let (progress, status_message) = match submission.status {
        SubmissionStatus::Uploaded => (10, "В очереди на проверку"),
        SubmissionStatus::Processing => {
            let mut progress = 50;
            let mut message = "Проверяется ИИ...";
            if let Some(started) = submission.ai_request_started_at {
                let elapsed = OffsetDateTime::now_utc().unix_timestamp()
                    - started.assume_utc().unix_timestamp();
                if elapsed > 120 {
                    progress = 70;
                    message = "Финальная обработка...";
                }
            }
            (progress, message)
        }
        SubmissionStatus::Preliminary => (100, "Проверено ИИ, ожидает подтверждения преподавателя"),
        SubmissionStatus::Approved => (100, "Проверено и одобрено"),
        SubmissionStatus::Flagged => (50, "Требует ручной проверки"),
        SubmissionStatus::Rejected => (50, "Отклонено"),
    };

    Ok(Json(serde_json::json!({
        "submission_id": submission_id,
        "status": submission.status,
        "progress": progress,
        "status_message": status_message,
        "ai_score": submission.ai_score,
        "final_score": submission.final_score,
        "max_score": submission.max_score,
        "ai_comments": submission.ai_comments,
        "ai_error": submission.ai_error,
        "ai_retry_count": submission.ai_retry_count,
        "processing_times": {
            "started_at": submission.ai_request_started_at.map(format_primitive),
            "completed_at": submission.ai_request_completed_at.map(format_primitive),
            "duration_seconds": submission.ai_request_duration_seconds
        }
    })))
}

fn session_to_response(session: ExamSession) -> ExamSessionResponse {
    ExamSessionResponse {
        id: session.id,
        exam_id: session.exam_id,
        student_id: session.student_id,
        variant_seed: session.variant_seed,
        variant_assignments: serde_json::to_value(&session.variant_assignments.0)
            .unwrap_or_else(|_| serde_json::json!({})),
        started_at: format_primitive(session.started_at),
        submitted_at: session.submitted_at.map(format_primitive),
        expires_at: format_primitive(session.expires_at),
        status: session.status,
        attempt_number: session.attempt_number,
    }
}

fn to_submission_response(
    submission: Submission,
    images: Vec<SubmissionImageResponse>,
    scores: Vec<SubmissionScoreResponse>,
) -> SubmissionResponse {
    SubmissionResponse {
        id: submission.id,
        session_id: submission.session_id,
        student_id: submission.student_id,
        submitted_at: format_primitive(submission.submitted_at),
        status: submission.status,
        ai_score: submission.ai_score,
        final_score: submission.final_score,
        max_score: submission.max_score,
        ai_analysis: submission.ai_analysis.map(|value| value.0),
        ai_comments: submission.ai_comments,
        teacher_comments: submission.teacher_comments,
        is_flagged: submission.is_flagged,
        flag_reasons: submission.flag_reasons.0,
        reviewed_by: submission.reviewed_by,
        reviewed_at: submission.reviewed_at.map(format_primitive),
        images,
        scores,
    }
}

async fn fetch_exam(pool: &sqlx::PgPool, exam_id: &str) -> Result<Exam, ApiError> {
    sqlx::query_as::<_, Exam>(
        "SELECT id, title, description, start_time, end_time, duration_minutes, timezone,
                max_attempts, allow_breaks, break_duration_minutes, auto_save_interval,
                status, created_by, created_at, updated_at, published_at, settings
         FROM exams WHERE id = $1",
    )
    .bind(exam_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| ApiError::Internal("Failed to fetch exam".to_string()))?
    .ok_or_else(|| ApiError::BadRequest("Exam not found".to_string()))
}

async fn fetch_session(pool: &sqlx::PgPool, session_id: &str) -> Result<ExamSession, ApiError> {
    sqlx::query_as::<_, ExamSession>(
        "SELECT id, exam_id, student_id, variant_seed, variant_assignments,
                started_at, submitted_at, expires_at, status, attempt_number,
                ip_address, user_agent, last_auto_save, auto_save_data, created_at, updated_at
         FROM exam_sessions WHERE id = $1",
    )
    .bind(session_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| ApiError::Internal("Failed to fetch session".to_string()))?
    .ok_or_else(|| ApiError::BadRequest("Session not found".to_string()))
}

async fn fetch_task_types(pool: &sqlx::PgPool, exam_id: &str) -> Result<Vec<TaskType>, ApiError> {
    sqlx::query_as::<_, TaskType>(
        "SELECT id, exam_id, title, description, order_index, max_score, rubric,
                difficulty, taxonomy_tags, formulas, units, validation_rules,
                created_at, updated_at
         FROM task_types WHERE exam_id = $1 ORDER BY order_index",
    )
    .bind(exam_id)
    .fetch_all(pool)
    .await
    .map_err(|_| ApiError::Internal("Failed to fetch task types".to_string()))
}

async fn fetch_images(
    pool: &sqlx::PgPool,
    submission_id: &str,
) -> Result<Vec<SubmissionImageResponse>, ApiError> {
    let images = sqlx::query_as::<_, SubmissionImage>(
        "SELECT id, submission_id, filename, file_path, file_size, mime_type,
                is_processed, ocr_text, quality_score, order_index, perceptual_hash,
                uploaded_at, processed_at
         FROM submission_images WHERE submission_id = $1 ORDER BY order_index",
    )
    .bind(submission_id)
    .fetch_all(pool)
    .await
    .map_err(|_| ApiError::Internal("Failed to fetch images".to_string()))?;

    Ok(images
        .into_iter()
        .map(|image| SubmissionImageResponse {
            id: image.id,
            filename: image.filename,
            order_index: image.order_index,
            file_path: image.file_path,
            file_size: image.file_size,
            mime_type: image.mime_type,
            is_processed: image.is_processed,
            quality_score: image.quality_score,
            uploaded_at: format_primitive(image.uploaded_at),
        })
        .collect())
}

async fn fetch_scores(
    pool: &sqlx::PgPool,
    submission_id: &str,
) -> Result<Vec<SubmissionScoreResponse>, ApiError> {
    let scores = sqlx::query_as::<_, SubmissionScore>(
        "SELECT id, submission_id, task_type_id, criterion_name, criterion_description,
                ai_score, final_score, max_score, ai_comment, teacher_comment, created_at, updated_at
         FROM submission_scores WHERE submission_id = $1",
    )
    .bind(submission_id)
    .fetch_all(pool)
    .await
    .map_err(|_| ApiError::Internal("Failed to fetch scores".to_string()))?;

    Ok(scores
        .into_iter()
        .map(|score| SubmissionScoreResponse {
            id: score.id,
            submission_id: score.submission_id,
            task_type_id: score.task_type_id,
            criterion_name: score.criterion_name,
            criterion_description: score.criterion_description,
            ai_score: score.ai_score,
            final_score: score.final_score,
            ai_comment: score.ai_comment,
            teacher_comment: score.teacher_comment,
            max_score: score.max_score,
        })
        .collect())
}

async fn build_task_context(
    pool: &sqlx::PgPool,
    session: &ExamSession,
) -> Result<Vec<serde_json::Value>, ApiError> {
    let task_types = fetch_task_types(pool, &session.exam_id).await?;
    let mut tasks = Vec::new();
    let assignments = session.variant_assignments.0.clone();

    for task_type in task_types {
        let variants = sqlx::query_as::<_, TaskVariant>(
            "SELECT id, task_type_id, content, parameters, reference_solution,
                    reference_answer, answer_tolerance, attachments, created_at
             FROM task_variants WHERE task_type_id = $1",
        )
        .bind(&task_type.id)
        .fetch_all(pool)
        .await
        .map_err(|_| ApiError::Internal("Failed to fetch variants".to_string()))?;

        if let Some(variant_id) = assignments.get(&task_type.id) {
            if let Some(variant) = variants.into_iter().find(|v| &v.id == variant_id) {
                tasks.push(serde_json::json!({
                    "task_type": {
                        "id": task_type.id,
                        "title": task_type.title,
                        "description": task_type.description,
                        "order_index": task_type.order_index,
                        "max_score": task_type.max_score,
                        "formulas": task_type.formulas.0,
                        "units": task_type.units.0,
                    },
                    "variant": {
                        "id": variant.id,
                        "content": variant.content,
                        "parameters": variant.parameters.0,
                        "attachments": variant.attachments.0,
                    }
                }));
            }
        }
    }

    Ok(tasks)
}

async fn enforce_deadline(
    session: &ExamSession,
    pool: &sqlx::PgPool,
) -> Result<(PrimitiveDateTime, SessionStatus), ApiError> {
    let exam = fetch_exam(pool, &session.exam_id).await?;
    let hard_deadline =
        if exam.end_time < session.expires_at { exam.end_time } else { session.expires_at };

    if OffsetDateTime::now_utc().unix_timestamp() >= hard_deadline.assume_utc().unix_timestamp()
        && session.status == SessionStatus::Active
    {
        sqlx::query("UPDATE exam_sessions SET status = $1 WHERE id = $2")
            .bind(SessionStatus::Expired)
            .bind(&session.id)
            .execute(pool)
            .await
            .ok();
        return Ok((hard_deadline, SessionStatus::Expired));
    }

    Ok((hard_deadline, session.status))
}

fn now_primitive() -> PrimitiveDateTime {
    let now = OffsetDateTime::now_utc();
    PrimitiveDateTime::new(now.date(), now.time())
}

fn sanitized_filename(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '_' || *c == '-')
        .collect();

    if sanitized.is_empty() {
        "upload".to_string()
    } else {
        sanitized
    }
}

async fn ensure_submission(pool: &sqlx::PgPool, session: &ExamSession) -> Result<String, ApiError> {
    let submission_id =
        sqlx::query_scalar::<_, String>("SELECT id FROM submissions WHERE session_id = $1")
            .bind(&session.id)
            .fetch_optional(pool)
            .await
            .map_err(|_| ApiError::Internal("Failed to fetch submission".to_string()))?;

    if let Some(id) = submission_id {
        return Ok(id);
    }

    let max_score: f64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(max_score), 100) FROM task_types WHERE exam_id = $1",
    )
    .bind(&session.exam_id)
    .fetch_one(pool)
    .await
    .unwrap_or(100.0);

    let now = now_primitive();
    let id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO submissions (id, session_id, student_id, status, max_score, submitted_at, created_at, updated_at)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
    )
    .bind(&id)
    .bind(&session.id)
    .bind(&session.student_id)
    .bind(SubmissionStatus::Uploaded)
    .bind(max_score)
    .bind(now)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .map_err(|_| ApiError::Internal("Failed to create submission".to_string()))?;

    Ok(id)
}

fn default_limit() -> i64 {
    100
}

#[cfg(test)]
mod tests {
    use super::sanitized_filename;
    use axum::http::{Method, StatusCode};
    use serde_json::json;
    use time::{Duration, OffsetDateTime, PrimitiveDateTime};
    use tower::ServiceExt;
    use uuid::Uuid;

    use crate::db::types::UserRole;
    use crate::test_support;

    #[test]
    fn sanitized_filename_filters_disallowed_chars() {
        let input = "report (final)!.png";
        let sanitized = sanitized_filename(input);
        assert_eq!(sanitized, "reportfinal.png");
    }

    #[test]
    fn sanitized_filename_falls_back_on_empty() {
        let input = "###";
        let sanitized = sanitized_filename(input);
        assert_eq!(sanitized, "upload");
    }

    fn exam_payload() -> serde_json::Value {
        let now = OffsetDateTime::now_utc().replace_nanosecond(0).expect("nanoseconds");
        let start_time = now - Duration::hours(1);
        let end_time = now + Duration::hours(2);

        json!({
            "title": "Autosave exam",
            "description": "Autosave flow",
            "start_time": start_time,
            "end_time": end_time,
            "duration_minutes": 60,
            "timezone": "UTC",
            "max_attempts": 1,
            "allow_breaks": false,
            "break_duration_minutes": 0,
            "auto_save_interval": 10,
            "settings": {},
            "task_types": [
                {
                    "title": "Task 1",
                    "description": "Auto-save task",
                    "order_index": 1,
                    "max_score": 10.0,
                    "rubric": {"criteria": []},
                    "difficulty": "easy",
                    "taxonomy_tags": [],
                    "formulas": [],
                    "units": [],
                    "validation_rules": {},
                    "variants": [
                        {
                            "content": "Balance equation",
                            "parameters": {},
                            "reference_solution": null,
                            "reference_answer": null,
                            "answer_tolerance": 0.01,
                            "attachments": []
                        }
                    ]
                }
            ]
        })
    }

    async fn create_published_exam(app: axum::Router, token: &str) -> String {
        let response = app
            .clone()
            .oneshot(test_support::json_request(
                Method::POST,
                "/api/v1/exams",
                Some(token),
                Some(exam_payload()),
            ))
            .await
            .expect("create exam");

        let status = response.status();
        let created = test_support::read_json(response).await;
        assert_eq!(status, StatusCode::CREATED, "response: {created}");
        let exam_id = created["id"].as_str().expect("exam id").to_string();

        let response = app
            .oneshot(test_support::json_request(
                Method::POST,
                &format!("/api/v1/exams/{exam_id}/publish"),
                Some(token),
                None,
            ))
            .await
            .expect("publish exam");

        let status = response.status();
        let published = test_support::read_json(response).await;
        assert_eq!(status, StatusCode::OK, "response: {published}");
        exam_id
    }

    async fn signup_student(
        app: axum::Router,
        isu: &str,
        full_name: &str,
        password: &str,
    ) -> (String, String) {
        let payload = json!({
            "isu": isu,
            "full_name": full_name,
            "password": password,
            "pd_consent": true
        });

        let response = app
            .oneshot(test_support::json_request(
                Method::POST,
                "/api/v1/auth/signup",
                None,
                Some(payload),
            ))
            .await
            .expect("signup");

        let status = response.status();
        let body = test_support::read_json(response).await;
        assert_eq!(status, StatusCode::CREATED, "response: {body}");

        let token = body["access_token"].as_str().expect("token").to_string();
        let user_id = body["user"]["id"].as_str().expect("user id").to_string();

        (token, user_id)
    }

    async fn login_student(app: axum::Router, isu: &str, password: &str) -> String {
        let payload = json!({
            "isu": isu,
            "password": password
        });

        let response = app
            .oneshot(test_support::json_request(
                Method::POST,
                "/api/v1/auth/login",
                None,
                Some(payload),
            ))
            .await
            .expect("login");

        let status = response.status();
        let body = test_support::read_json(response).await;
        assert_eq!(status, StatusCode::OK, "response: {body}");
        body["access_token"].as_str().expect("token").to_string()
    }

    #[tokio::test]
    async fn student_auto_save_is_rate_limited() {
        let ctx = test_support::setup_test_context().await;

        let teacher = test_support::insert_user(
            ctx.state.db(),
            "000010",
            "Teacher User",
            UserRole::Teacher,
            "teacher-pass",
        )
        .await;
        let student = test_support::insert_user(
            ctx.state.db(),
            "000011",
            "Student User",
            UserRole::Student,
            "student-pass",
        )
        .await;

        let teacher_token = test_support::bearer_token(&teacher.id, ctx.state.settings());
        let student_token = test_support::bearer_token(&student.id, ctx.state.settings());

        let exam_id = create_published_exam(ctx.app.clone(), &teacher_token).await;

        let response = ctx
            .app
            .clone()
            .oneshot(test_support::json_request(
                Method::POST,
                &format!("/api/v1/submissions/exams/{exam_id}/enter"),
                Some(&student_token),
                None,
            ))
            .await
            .expect("enter exam");

        let status = response.status();
        let session = test_support::read_json(response).await;
        assert_eq!(status, StatusCode::OK, "response: {session}");
        let session_id = session["id"].as_str().expect("session id");

        let payload = json!({ "draft": { "q1": "answer" } });
        let response = ctx
            .app
            .clone()
            .oneshot(test_support::json_request(
                Method::POST,
                &format!("/api/v1/submissions/sessions/{session_id}/auto-save"),
                Some(&student_token),
                Some(payload.clone()),
            ))
            .await
            .expect("auto-save");

        let status = response.status();
        let body = test_support::read_json(response).await;
        assert_eq!(status, StatusCode::OK, "response: {body}");
        assert_eq!(body["success"], true);

        let response = ctx
            .app
            .oneshot(test_support::json_request(
                Method::POST,
                &format!("/api/v1/submissions/sessions/{session_id}/auto-save"),
                Some(&student_token),
                Some(payload),
            ))
            .await
            .expect("auto-save rate limit");

        let status = response.status();
        let error = test_support::read_json(response).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "response: {error}");
        assert_eq!(error["detail"], "Auto-save rate limit exceeded");
    }

    #[tokio::test]
    async fn student_can_submit_exam() {
        let ctx = test_support::setup_test_context().await;

        let teacher = test_support::insert_user(
            ctx.state.db(),
            "000020",
            "Teacher User",
            UserRole::Teacher,
            "teacher-pass",
        )
        .await;
        let student = test_support::insert_user(
            ctx.state.db(),
            "000021",
            "Student User",
            UserRole::Student,
            "student-pass",
        )
        .await;

        let teacher_token = test_support::bearer_token(&teacher.id, ctx.state.settings());
        let student_token = test_support::bearer_token(&student.id, ctx.state.settings());

        let exam_id = create_published_exam(ctx.app.clone(), &teacher_token).await;

        let response = ctx
            .app
            .clone()
            .oneshot(test_support::json_request(
                Method::POST,
                &format!("/api/v1/submissions/exams/{exam_id}/enter"),
                Some(&student_token),
                None,
            ))
            .await
            .expect("enter exam");

        let status = response.status();
        let session = test_support::read_json(response).await;
        assert_eq!(status, StatusCode::OK, "response: {session}");
        let session_id = session["id"].as_str().expect("session id");

        let response = ctx
            .app
            .oneshot(test_support::json_request(
                Method::POST,
                &format!("/api/v1/submissions/sessions/{session_id}/submit"),
                Some(&student_token),
                None,
            ))
            .await
            .expect("submit exam");

        let status = response.status();
        let submission = test_support::read_json(response).await;
        assert_eq!(status, StatusCode::OK, "response: {submission}");
        assert_eq!(submission["session_id"], session_id);
        assert_eq!(submission["status"], "uploaded");
    }

    #[tokio::test]
    async fn view_url_returns_presigned_url() {
        let ctx = test_support::setup_test_context_with_storage().await;

        let teacher = test_support::insert_user(
            ctx.state.db(),
            "000030",
            "Teacher User",
            UserRole::Teacher,
            "teacher-pass",
        )
        .await;
        let teacher_token = test_support::bearer_token(&teacher.id, ctx.state.settings());

        let (student_token, student_id) =
            signup_student(ctx.app.clone(), "000031", "Student User", "student-pass").await;

        let exam_id = create_published_exam(ctx.app.clone(), &teacher_token).await;

        let response = ctx
            .app
            .clone()
            .oneshot(test_support::json_request(
                Method::POST,
                &format!("/api/v1/submissions/exams/{exam_id}/enter"),
                Some(&student_token),
                None,
            ))
            .await
            .expect("enter exam");

        let status = response.status();
        let session = test_support::read_json(response).await;
        assert_eq!(status, StatusCode::OK, "response: {session}");
        let session_id = session["id"].as_str().expect("session id");

        let response = ctx
            .app
            .clone()
            .oneshot(test_support::json_request(
                Method::POST,
                &format!("/api/v1/submissions/sessions/{session_id}/submit"),
                Some(&student_token),
                None,
            ))
            .await
            .expect("submit exam");

        let status = response.status();
        let submission = test_support::read_json(response).await;
        assert_eq!(status, StatusCode::OK, "response: {submission}");
        let submission_id = submission["id"].as_str().expect("submission id");

        let image_id = Uuid::new_v4().to_string();
        let file_path = format!("submissions/{session_id}/image.png");
        let now_offset = OffsetDateTime::now_utc();
        let now = PrimitiveDateTime::new(now_offset.date(), now_offset.time());

        sqlx::query(
            "INSERT INTO submission_images (
                id, submission_id, filename, file_path, file_size, mime_type,
                order_index, is_processed, uploaded_at
            ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
        )
        .bind(&image_id)
        .bind(submission_id)
        .bind("image.png")
        .bind(&file_path)
        .bind(1024_i64)
        .bind("image/png")
        .bind(0_i32)
        .bind(false)
        .bind(now)
        .execute(ctx.state.db())
        .await
        .expect("insert image");

        let response = ctx
            .app
            .oneshot(test_support::json_request(
                Method::GET,
                &format!("/api/v1/submissions/images/{image_id}/view-url"),
                Some(&student_token),
                None,
            ))
            .await
            .expect("view url");

        let status = response.status();
        let body = test_support::read_json(response).await;
        assert_eq!(status, StatusCode::OK, "response: {body}");
        assert!(body["view_url"].as_str().unwrap_or("").contains("image.png"));
        assert_eq!(body["mime_type"], "image/png");
        assert_eq!(body["filename"], "image.png");

        let owner: Option<String> =
            sqlx::query_scalar("SELECT student_id FROM submissions WHERE id = $1")
                .bind(submission_id)
                .fetch_optional(ctx.state.db())
                .await
                .expect("owner");
        assert_eq!(owner.as_deref(), Some(student_id.as_str()));
    }

    #[tokio::test]
    async fn full_flow_signup_login_submit_and_approve() {
        let ctx = test_support::setup_test_context_with_storage().await;

        let teacher = test_support::insert_user(
            ctx.state.db(),
            "000040",
            "Teacher User",
            UserRole::Teacher,
            "teacher-pass",
        )
        .await;
        let teacher_token = test_support::bearer_token(&teacher.id, ctx.state.settings());

        let (student_token, _student_id) =
            signup_student(ctx.app.clone(), "000041", "Student User", "student-pass").await;
        let login_token = login_student(ctx.app.clone(), "000041", "student-pass").await;
        assert!(!login_token.is_empty());

        let exam_id = create_published_exam(ctx.app.clone(), &teacher_token).await;

        let response = ctx
            .app
            .clone()
            .oneshot(test_support::json_request(
                Method::POST,
                &format!("/api/v1/submissions/exams/{exam_id}/enter"),
                Some(&student_token),
                None,
            ))
            .await
            .expect("enter exam");

        let status = response.status();
        let session = test_support::read_json(response).await;
        assert_eq!(status, StatusCode::OK, "response: {session}");
        let session_id = session["id"].as_str().expect("session id");

        let response = ctx
            .app
            .clone()
            .oneshot(test_support::json_request(
                Method::POST,
                &format!("/api/v1/submissions/sessions/{session_id}/presigned-upload-url?filename=work.png&content_type=image/png"),
                Some(&student_token),
                None,
            ))
            .await
            .expect("presign url");

        let status = response.status();
        let presign = test_support::read_json(response).await;
        assert_eq!(status, StatusCode::OK, "response: {presign}");
        assert!(presign["upload_url"].as_str().unwrap_or("").contains("work.png"));

        let response = ctx
            .app
            .clone()
            .oneshot(test_support::json_request(
                Method::POST,
                &format!("/api/v1/submissions/sessions/{session_id}/submit"),
                Some(&student_token),
                None,
            ))
            .await
            .expect("submit exam");

        let status = response.status();
        let submission = test_support::read_json(response).await;
        assert_eq!(status, StatusCode::OK, "response: {submission}");
        let submission_id = submission["id"].as_str().expect("submission id");

        let response = ctx
            .app
            .clone()
            .oneshot(test_support::json_request(
                Method::POST,
                &format!("/api/v1/submissions/{submission_id}/regrade"),
                Some(&teacher_token),
                None,
            ))
            .await
            .expect("regrade");

        let status = response.status();
        let regrade = test_support::read_json(response).await;
        assert_eq!(status, StatusCode::OK, "response: {regrade}");
        assert_eq!(regrade["status"], "processing");

        let response = ctx
            .app
            .clone()
            .oneshot(test_support::json_request(
                Method::POST,
                &format!("/api/v1/submissions/{submission_id}/approve"),
                Some(&teacher_token),
                Some(json!({"teacher_comments": "Looks good"})),
            ))
            .await
            .expect("approve");

        let status = response.status();
        let approve = test_support::read_json(response).await;
        assert_eq!(status, StatusCode::OK, "response: {approve}");

        let response = ctx
            .app
            .oneshot(test_support::json_request(
                Method::GET,
                &format!("/api/v1/submissions/{submission_id}"),
                Some(&teacher_token),
                None,
            ))
            .await
            .expect("get submission");

        let status = response.status();
        let fetched = test_support::read_json(response).await;
        assert_eq!(status, StatusCode::OK, "response: {fetched}");
        assert_eq!(fetched["status"], "approved");
    }
}
