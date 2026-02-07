use anyhow::{Context, Result};
use serde_json::{json, Value};
use sqlx::types::Json;
use sqlx::PgPool;
use time::{OffsetDateTime, PrimitiveDateTime};

use crate::core::state::AppState;
use crate::core::time::primitive_now_utc as now_primitive;
use crate::db::models::{Exam, ExamSession, Submission, SubmissionImage};
use crate::db::types::{ExamStatus, SessionStatus, SubmissionStatus};
use crate::services::ai_grading::{AiGradingService, GradeRequest};

pub(crate) async fn claim_next_submission(pool: &PgPool) -> Result<Option<String>> {
    let now = now_primitive();
    let id = sqlx::query_scalar::<_, String>(
        "WITH candidate AS (
            SELECT id FROM submissions
            WHERE status IN ($1, $2)
              AND ai_request_started_at IS NULL
            ORDER BY CASE WHEN status = $1 THEN 0 ELSE 1 END,
                     COALESCE(ai_retry_count, 0),
                     created_at
            FOR UPDATE SKIP LOCKED
            LIMIT 1
        )
        UPDATE submissions
        SET status = $3,
            ai_request_started_at = $4,
            ai_error = NULL
        FROM candidate
        WHERE submissions.id = candidate.id
        RETURNING submissions.id",
    )
    .bind(SubmissionStatus::Uploaded)
    .bind(SubmissionStatus::Processing)
    .bind(SubmissionStatus::Processing)
    .bind(now)
    .fetch_optional(pool)
    .await
    .context("Failed to claim submission")?;

    Ok(id)
}

pub(crate) async fn grade_submission(
    state: &AppState,
    ai: &AiGradingService,
    submission_id: &str,
) -> Result<()> {
    let submission =
        fetch_submission(state.db(), submission_id).await?.context("Submission not found")?;

    if !matches!(submission.status, SubmissionStatus::Processing | SubmissionStatus::Flagged) {
        tracing::info!(submission_id, status = ?submission.status, "Skipping grading");
        return Ok(());
    }

    let session =
        fetch_session(state.db(), &submission.session_id).await?.context("Session not found")?;
    let exam = fetch_exam(state.db(), &session.exam_id).await?.context("Exam not found")?;

    let mut images = fetch_images(state.db(), &submission.id).await?;
    images.sort_by_key(|image| image.order_index);

    if images.is_empty() {
        return flag_submission(
            state.db(),
            &submission.id,
            "No images available for AI grading",
            vec!["no_images".to_string()],
            false,
        )
        .await;
    }

    let storage = state.storage().ok_or_else(|| anyhow::anyhow!("S3 storage not configured"))?;

    let mut image_urls = Vec::new();
    for image in &images {
        if !image.file_path.starts_with("submissions/") {
            return flag_submission(
                state.db(),
                &submission.id,
                "Image is stored in local storage",
                vec!["storage_mismatch".to_string()],
                true,
            )
            .await;
        }

        let url = storage
            .presign_get(&image.file_path, std::time::Duration::from_secs(3600))
            .await
            .context("Failed to generate presigned URL")?;
        image_urls.push(url);
    }

    let (task_description, reference_solution, rubric, total_max_score) =
        build_task_prompt(state.db(), &exam, &session).await?;

    let request = GradeRequest {
        images: image_urls,
        task_description,
        reference_solution,
        rubric,
        max_score: total_max_score,
        chemistry_rules: None,
        submission_id: Some(submission.id.clone()),
    };

    let started_at = submission.ai_request_started_at.unwrap_or_else(now_primitive);
    let queue_latency =
        (started_at.assume_utc() - submission.created_at.assume_utc()).as_seconds_f64();

    let mut result = match ai.grade_submission(request).await {
        Ok(value) => value,
        Err(err) => {
            tracing::error!(submission_id, error = %err, "AI grading failed");
            metrics::counter!("grading_jobs_total", "status" => "failed").increment(1);
            return flag_submission(
                state.db(),
                &submission.id,
                &err.to_string(),
                vec!["ai_processing_error".to_string()],
                true,
            )
            .await;
        }
    };

    let unreadable = result.get("unreadable").and_then(|value| value.as_bool()).unwrap_or(false);

    let completed_at = now_primitive();
    let duration = (completed_at.assume_utc() - started_at.assume_utc()).as_seconds_f64();

    if unreadable {
        let reason = result
            .get("unreadable_reason")
            .and_then(|value| value.as_str())
            .unwrap_or("Изображение нечитаемо");
        metrics::counter!("grading_jobs_total", "status" => "unreadable").increment(1);
        return flag_submission(
            state.db(),
            &submission.id,
            reason,
            vec!["unreadable_images".to_string()],
            false,
        )
        .await;
    }

    if let Some(map) = result.as_object_mut() {
        map.remove("_metadata");
    }

    let total_score = result.get("total_score").and_then(|value| value.as_f64());

    let feedback =
        result.get("feedback").and_then(|value| value.as_str()).map(|value| value.to_string());

    sqlx::query(
        "UPDATE submissions SET status = $1, ai_score = $2, ai_analysis = $3, ai_comments = $4,
            ai_processed_at = $5, ai_request_completed_at = $6, ai_request_duration_seconds = $7,
            ai_error = NULL, is_flagged = FALSE, flag_reasons = $8, updated_at = $9
         WHERE id = $10",
    )
    .bind(SubmissionStatus::Preliminary)
    .bind(total_score)
    .bind(Json(result.clone()))
    .bind(feedback)
    .bind(completed_at)
    .bind(completed_at)
    .bind(duration)
    .bind(Json(Vec::<String>::new()))
    .bind(completed_at)
    .bind(&submission.id)
    .execute(state.db())
    .await
    .context("Failed to update submission")?;

    metrics::counter!("grading_jobs_total", "status" => "success").increment(1);
    metrics::histogram!("grading_duration_seconds").record(duration);
    metrics::histogram!("grading_queue_latency_seconds").record(queue_latency);

    if let Some(per_page) = result.get("per_page_transcriptions").and_then(|value| value.as_array())
    {
        for (idx, image) in images.iter().enumerate() {
            if let Some(text) = per_page.get(idx).and_then(|value| value.as_str()) {
                sqlx::query(
                    "UPDATE submission_images SET ocr_text = $1, processed_at = $2, is_processed = TRUE
                     WHERE id = $3",
                )
                .bind(text)
                .bind(completed_at)
                .bind(&image.id)
                .execute(state.db())
                .await
                .context("Failed to persist OCR transcription")?;
            }
        }
    }

    tracing::info!(submission_id, "AI grading succeeded");

    Ok(())
}

pub(crate) async fn process_completed_exams(state: &AppState) -> Result<()> {
    let now = now_primitive();

    let exams = sqlx::query_as::<_, Exam>(
        "SELECT id, title, description, start_time, end_time, duration_minutes, timezone,
                max_attempts, allow_breaks, break_duration_minutes, auto_save_interval,
                status, created_by, created_at, updated_at, published_at, settings
         FROM exams
         WHERE status IN ($1, $2)
           AND end_time <= $3",
    )
    .bind(ExamStatus::Active)
    .bind(ExamStatus::Published)
    .bind(now)
    .fetch_all(state.db())
    .await
    .context("Failed to fetch completed exams")?;

    if exams.is_empty() {
        return Ok(());
    }

    let mut queued = 0;

    for exam in &exams {
        let queued_ids = sqlx::query_scalar::<_, String>(
            "UPDATE submissions s
             SET status = $1,
                 ai_request_started_at = NULL,
                 updated_at = $2
             FROM exam_sessions es
             WHERE es.id = s.session_id
               AND es.exam_id = $3
               AND s.status = $4
               AND s.ai_request_started_at IS NULL
             RETURNING s.id",
        )
        .bind(SubmissionStatus::Processing)
        .bind(now)
        .bind(&exam.id)
        .bind(SubmissionStatus::Uploaded)
        .fetch_all(state.db())
        .await
        .context("Failed to queue submissions for processing")?;
        queued += queued_ids.len();

        sqlx::query("UPDATE exams SET status = $1, updated_at = $2 WHERE id = $3")
            .bind(ExamStatus::Completed)
            .bind(now)
            .bind(&exam.id)
            .execute(state.db())
            .await
            .context("Failed to mark exam as completed")?;
    }

    tracing::info!(
        processed_exams = exams.len(),
        queued_submissions = queued,
        "Processed completed exams"
    );
    metrics::counter!("exams_processed_total").increment(exams.len() as u64);
    metrics::counter!("submissions_queued_total").increment(queued as u64);

    Ok(())
}

pub(crate) async fn close_expired_sessions(state: &AppState) -> Result<()> {
    let now = OffsetDateTime::now_utc();

    #[derive(sqlx::FromRow)]
    struct SessionRow {
        id: String,
        expires_at: PrimitiveDateTime,
        exam_end_time: Option<PrimitiveDateTime>,
    }

    let sessions = sqlx::query_as::<_, SessionRow>(
        "SELECT s.id, s.expires_at, e.end_time AS exam_end_time
         FROM exam_sessions s
         LEFT JOIN exams e ON e.id = s.exam_id
         WHERE s.status = $1",
    )
    .bind(SessionStatus::Active)
    .fetch_all(state.db())
    .await
    .context("Failed to fetch active sessions")?;

    let mut closed = 0;

    for session in sessions {
        let hard_deadline = match session.exam_end_time {
            Some(end) => {
                if end < session.expires_at {
                    end
                } else {
                    session.expires_at
                }
            }
            None => session.expires_at,
        };

        if now.unix_timestamp() < hard_deadline.assume_utc().unix_timestamp() {
            continue;
        }

        sqlx::query(
            "UPDATE exam_sessions SET status = $1, submitted_at = COALESCE(submitted_at, $2), updated_at = $3 WHERE id = $4",
        )
        .bind(SessionStatus::Expired)
        .bind(hard_deadline)
        .bind(now_primitive())
        .bind(&session.id)
        .execute(state.db())
        .await
        .context("Failed to expire session")?;

        closed += 1;
    }

    tracing::info!(closed_sessions = closed, "Closed expired sessions");
    metrics::counter!("expired_sessions_closed_total").increment(closed as u64);

    Ok(())
}

pub(crate) async fn retry_failed_submissions(state: &AppState) -> Result<()> {
    let submissions = sqlx::query_scalar::<_, String>(
        "SELECT id FROM submissions
         WHERE status = $1
           AND COALESCE(ai_retry_count, 0) < 3
           AND ai_error IS NOT NULL",
    )
    .bind(SubmissionStatus::Flagged)
    .fetch_all(state.db())
    .await
    .context("Failed to fetch failed submissions")?;

    let mut retried = 0;
    let now = now_primitive();

    for submission_id in submissions {
        let updated = sqlx::query(
            "UPDATE submissions
             SET status = $1,
                 ai_retry_count = COALESCE(ai_retry_count, 0) + 1,
                 ai_error = NULL,
                 ai_request_started_at = NULL,
                 updated_at = $2
             WHERE id = $3",
        )
        .bind(SubmissionStatus::Processing)
        .bind(now)
        .bind(&submission_id)
        .execute(state.db())
        .await
        .context("Failed to requeue failed submission")?;

        if updated.rows_affected() > 0 {
            retried += 1;
        }
    }

    tracing::info!(retried_submissions = retried, "Retried failed submissions");
    metrics::counter!("submissions_retried_total").increment(retried as u64);

    Ok(())
}

async fn build_task_prompt(
    pool: &PgPool,
    exam: &Exam,
    session: &ExamSession,
) -> Result<(String, String, Value, f64)> {
    let task_types = crate::repositories::task_types::list_by_exam(pool, &exam.id)
        .await
        .context("Failed to fetch task types")?;

    let assignments = &session.variant_assignments.0;
    let mut descriptions = Vec::new();
    let mut reference_solutions = Vec::new();
    let mut rubric_items = Vec::new();
    let mut total_max_score = 0.0;

    for task_type in task_types {
        if let Some(variant_id) = assignments.get(&task_type.id) {
            let variant = crate::repositories::task_types::find_variant_by_id(pool, variant_id)
                .await
                .context("Failed to fetch variant")?;

            if let Some(variant) = variant {
                descriptions.push(format!(
                    "Задача {}: {}\n{}\n\nВариант:\n{}",
                    task_type.order_index + 1,
                    task_type.title,
                    task_type.description,
                    variant.content
                ));

                if let Some(reference) = variant.reference_solution {
                    reference_solutions.push(format!(
                        "Эталонное решение для задачи {}:\n{}",
                        task_type.order_index + 1,
                        reference
                    ));
                }

                rubric_items.push(json!({
                    "task_type": task_type.title,
                    "max_score": task_type.max_score,
                    "criteria": "Оценивать по критериям в системном промпте"
                }));

                total_max_score += task_type.max_score;
            }
        }
    }

    if total_max_score == 0.0 {
        total_max_score =
            exam.settings.0.get("max_score").and_then(|value| value.as_f64()).unwrap_or(100.0);
    }

    let rubric = json!({
        "criteria": rubric_items,
        "total_max_score": total_max_score,
    });

    let task_description = descriptions.join("\n\n");
    let reference_solution = if reference_solutions.is_empty() {
        "См. критерии оценивания".to_string()
    } else {
        reference_solutions.join("\n\n")
    };

    Ok((task_description, reference_solution, rubric, total_max_score))
}

async fn fetch_submission(pool: &PgPool, submission_id: &str) -> Result<Option<Submission>> {
    crate::repositories::submissions::find_by_id(pool, submission_id)
        .await
        .context("Failed to fetch submission")
}

async fn fetch_session(pool: &PgPool, session_id: &str) -> Result<Option<ExamSession>> {
    crate::repositories::sessions::find_by_id(pool, session_id)
        .await
        .context("Failed to fetch session")
}

async fn fetch_exam(pool: &PgPool, exam_id: &str) -> Result<Option<Exam>> {
    crate::repositories::exams::find_by_id(pool, exam_id).await.context("Failed to fetch exam")
}

async fn fetch_images(pool: &PgPool, submission_id: &str) -> Result<Vec<SubmissionImage>> {
    crate::repositories::images::list_by_submission(pool, submission_id)
        .await
        .context("Failed to fetch images")
}

async fn flag_submission(
    pool: &PgPool,
    submission_id: &str,
    reason: &str,
    flag_reasons: Vec<String>,
    increment_retry: bool,
) -> Result<()> {
    let now = now_primitive();
    let retry_expr =
        if increment_retry { "ai_retry_count = COALESCE(ai_retry_count,0) + 1," } else { "" };
    let query = format!(
        "UPDATE submissions SET status = $1, ai_error = $2, is_flagged = TRUE, flag_reasons = $3,
            ai_request_completed_at = $4, ai_request_duration_seconds = $5, {retry_expr} updated_at = $6
         WHERE id = $7",
    );

    sqlx::query(&query)
        .bind(SubmissionStatus::Flagged)
        .bind(reason)
        .bind(Json(flag_reasons))
        .bind(now)
        .bind(0.0)
        .bind(now)
        .bind(submission_id)
        .execute(pool)
        .await
        .context("Failed to flag submission")?;

    Ok(())
}
