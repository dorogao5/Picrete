use axum::{
    extract::{Multipart, Path, Query, State},
    Json,
};
use rand::rngs::StdRng;
use rand::{seq::SliceRandom, SeedableRng};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::api::errors::ApiError;
use crate::api::guards::CurrentUser;
use crate::core::state::AppState;
use crate::db::types::{ExamStatus, SessionStatus, SubmissionStatus};
use crate::repositories;
use crate::schemas::submission::{format_primitive, ExamSessionResponse, SubmissionResponse};

use super::PresignQuery;

pub(super) async fn get_my_submissions(
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<serde_json::Value>>, ApiError> {
    let sessions = repositories::sessions::list_by_student(state.db(), &user.id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch sessions"))?;

    let mut response = Vec::new();
    for session in sessions {
        let exam_title = repositories::exams::find_title_by_id(state.db(), &session.exam_id)
            .await
            .unwrap_or(None);

        let submission = repositories::submissions::find_by_session(state.db(), &session.id)
            .await
            .map_err(|e| ApiError::internal(e, "Failed to fetch submission"))?;

        let (images, scores) = if let Some(ref sub) = submission {
            (
                super::helpers::fetch_images(state.db(), &sub.id).await?,
                super::helpers::fetch_scores(state.db(), &sub.id).await?,
            )
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

pub(super) async fn enter_exam(
    Path(exam_id): Path<String>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
) -> Result<Json<ExamSessionResponse>, ApiError> {
    let exam = super::helpers::fetch_exam(state.db(), &exam_id).await?;

    if !matches!(exam.status, ExamStatus::Published | ExamStatus::Active) {
        return Err(ApiError::BadRequest("Exam is not available".to_string()));
    }

    let now = super::helpers::now_primitive();

    if now < exam.start_time {
        return Err(ApiError::BadRequest("Exam has not started yet".to_string()));
    }
    if now > exam.end_time {
        return Err(ApiError::BadRequest("Exam has ended".to_string()));
    }

    let existing = repositories::sessions::find_active(state.db(), &exam_id, &user.id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch session"))?;

    if let Some(session) = existing {
        return Ok(Json(super::helpers::session_to_response(session)));
    }

    let attempts = repositories::sessions::count_by_exam_and_student(state.db(), &exam_id, &user.id).await;

    if attempts >= exam.max_attempts as i64 {
        return Err(ApiError::BadRequest("Maximum attempts reached".to_string()));
    }

    let task_types = repositories::task_types::list_by_exam(state.db(), &exam_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch task types"))?;

    let seed = rand::random::<u32>();
    let mut rng = StdRng::seed_from_u64(seed as u64);
    let mut assignments = serde_json::Map::new();

    for task_type in task_types {
        let variants = repositories::task_types::list_variants(state.db(), &task_type.id)
            .await
            .map_err(|e| ApiError::internal(e, "Failed to fetch variants"))?;

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
    .map_err(|e| ApiError::internal(e, "Failed to create session"))?;

    let session = repositories::sessions::fetch_one_by_id(state.db(), &session_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch session"))?;

    Ok(Json(super::helpers::session_to_response(session)))
}

pub(super) async fn get_session_variant(
    Path(session_id): Path<String>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let session = super::helpers::fetch_session(state.db(), &session_id).await?;

    if session.student_id != user.id {
        return Err(ApiError::Forbidden("Access denied"));
    }

    let task_types = super::helpers::fetch_task_types(state.db(), &session.exam_id).await?;
    let mut tasks = Vec::new();

    let assignments = session.variant_assignments.0.clone();

    for task_type in task_types {
        let variants = repositories::task_types::list_variants(state.db(), &task_type.id)
            .await
            .map_err(|e| ApiError::internal(e, "Failed to fetch variants"))?;

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
        "session": super::helpers::session_to_response(session),
        "tasks": tasks,
        "time_remaining": remaining,
    })))
}

pub(super) async fn presigned_upload_url(
    Path(session_id): Path<String>,
    Query(query): Query<PresignQuery>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let session = super::helpers::fetch_session(state.db(), &session_id).await?;
    if session.student_id != user.id {
        return Err(ApiError::Forbidden("Access denied"));
    }

    let (hard_deadline, session_status) =
        super::helpers::enforce_deadline(&session, state.db()).await?;
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

    let filename = super::helpers::sanitized_filename(&query.filename);
    let key = format!("submissions/{session_id}/{filename}");
    let expires =
        std::time::Duration::from_secs(state.settings().exam().presigned_url_expire_minutes * 60);
    let presigned = storage
        .presign_put(&key, &query.content_type, expires)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to generate upload URL"))?;

    Ok(Json(serde_json::json!({
        "upload_url": presigned,
        "s3_key": key,
        "method": "PUT",
        "headers": {"Content-Type": query.content_type}
    })))
}

pub(super) async fn upload_image(
    Path(session_id): Path<String>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, ApiError> {
    let session = super::helpers::fetch_session(state.db(), &session_id).await?;
    if session.student_id != user.id {
        return Err(ApiError::Forbidden("Access denied"));
    }

    let (hard_deadline, session_status) =
        super::helpers::enforce_deadline(&session, state.db()).await?;
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
    let submission_id = super::helpers::ensure_submission(state.db(), &session).await?;

    let current_images = repositories::images::count_by_submission(state.db(), &submission_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to count submission images"))?;

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
    let key = format!(
        "submissions/{}/{}_{}",
        session_id,
        image_id,
        super::helpers::sanitized_filename(&filename)
    );

    let (file_size, _hash) = storage
        .upload_bytes(&key, &content_type, file_bytes)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to upload file to S3"))?;

    repositories::images::insert(
        state.db(),
        &image_id,
        &submission_id,
        &filename,
        &key,
        file_size,
        &content_type,
        order_index,
        super::helpers::now_primitive(),
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to store image metadata"))?;

    Ok(Json(serde_json::json!({
        "message": "File uploaded successfully",
        "image_id": image_id
    })))
}

pub(super) async fn auto_save(
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

    let session = super::helpers::fetch_session(state.db(), &session_id).await?;
    if session.student_id != user.id {
        return Err(ApiError::Forbidden("Access denied"));
    }

    let (hard_deadline, session_status) =
        super::helpers::enforce_deadline(&session, state.db()).await?;
    if session_status != SessionStatus::Active {
        return Err(ApiError::BadRequest("Session is not active".to_string()));
    }

    if OffsetDateTime::now_utc().unix_timestamp() >= hard_deadline.assume_utc().unix_timestamp() {
        return Err(ApiError::BadRequest("Session has expired".to_string()));
    }

    let now = super::helpers::now_primitive();
    repositories::sessions::update_auto_save(state.db(), &session_id, payload, now)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to save auto data"))?;

    Ok(Json(serde_json::json!({
        "success": true,
        "last_auto_save": format_primitive(now),
        "message": "Data saved successfully"
    })))
}

pub(super) async fn submit_exam(
    Path(session_id): Path<String>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
) -> Result<Json<SubmissionResponse>, ApiError> {
    let session = super::helpers::fetch_session(state.db(), &session_id).await?;
    if session.student_id != user.id {
        return Err(ApiError::Forbidden("Access denied"));
    }

    let (hard_deadline, session_status) =
        super::helpers::enforce_deadline(&session, state.db()).await?;
    let now_offset = OffsetDateTime::now_utc();
    let now = super::helpers::now_primitive();
    let recently_expired =
        now_offset.unix_timestamp() <= hard_deadline.assume_utc().unix_timestamp() + 300;

    if session_status != SessionStatus::Active && !recently_expired {
        return Err(ApiError::BadRequest("Session is not active or has expired".to_string()));
    }

    let mut submission = repositories::submissions::find_by_session(state.db(), &session_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch submission"))?;

    if submission.is_none() {
        let max_score = repositories::exams::max_score_for_exam(state.db(), &session.exam_id).await;

        let submission_id = Uuid::new_v4().to_string();
        repositories::submissions::create(
            state.db(),
            &submission_id,
            &session_id,
            &session.student_id,
            SubmissionStatus::Uploaded,
            max_score,
            now,
            now,
        )
        .await
        .map_err(|e| ApiError::internal(e, "Failed to create submission"))?;

        submission = repositories::submissions::find_by_id(state.db(), &submission_id)
            .await
            .map_err(|e| ApiError::internal(e, "Failed to fetch submission"))?;
    }

    repositories::sessions::submit(state.db(), &session_id, now)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to update session"))?;

    repositories::submissions::update_status_by_session(
        state.db(),
        &session_id,
        SubmissionStatus::Uploaded,
        now,
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to update submission"))?;

    let submission =
        submission.ok_or_else(|| ApiError::Internal("Submission missing".to_string()))?;
    let images = super::helpers::fetch_images(state.db(), &submission.id).await?;
    let scores = super::helpers::fetch_scores(state.db(), &submission.id).await?;

    Ok(Json(super::helpers::to_submission_response(submission, images, scores)))
}

pub(super) async fn get_session_result(
    Path(session_id): Path<String>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let session = super::helpers::fetch_session(state.db(), &session_id).await?;
    if session.student_id != user.id {
        return Err(ApiError::Forbidden("Access denied"));
    }

    let submission = repositories::submissions::find_by_session(state.db(), &session_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch submission"))?;

    let Some(submission) = submission else {
        return Err(ApiError::BadRequest("No submission found for this session".to_string()));
    };

    let attempts = repositories::sessions::count_by_exam_and_student(
        state.db(),
        &session.exam_id,
        &user.id,
    )
    .await;
    let exam = super::helpers::fetch_exam(state.db(), &session.exam_id).await?;

    let images = super::helpers::fetch_images(state.db(), &submission.id).await?;
    let scores = super::helpers::fetch_scores(state.db(), &submission.id).await?;

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
