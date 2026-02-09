use anyhow::{Context, Result};
use serde_json::{json, Value};
use sqlx::PgPool;

use crate::core::state::AppState;
use crate::core::time::primitive_now_utc as now_primitive;
use crate::db::models::{Exam, ExamSession, Submission, SubmissionImage, TaskVariant};
use crate::db::types::SubmissionStatus;
use crate::repositories;
use crate::services::ai_grading::{AiGradingService, GradeRequest};

pub(crate) async fn claim_next_submission(pool: &PgPool) -> Result<Option<(String, String)>> {
    let now = now_primitive();
    repositories::submissions::claim_next_for_processing(pool, now)
        .await
        .context("Failed to claim submission")
}

pub(crate) async fn grade_submission(
    state: &AppState,
    ai: &AiGradingService,
    course_id: &str,
    submission_id: &str,
) -> Result<()> {
    let submission = fetch_submission(state.db(), course_id, submission_id)
        .await?
        .context("Submission not found")?;

    if !matches!(submission.status, SubmissionStatus::Processing | SubmissionStatus::Flagged) {
        tracing::info!(course_id, submission_id, status = ?submission.status, "Skipping grading");
        return Ok(());
    }

    let session = fetch_session(state.db(), course_id, &submission.session_id)
        .await?
        .context("Session not found")?;
    let exam =
        fetch_exam(state.db(), course_id, &session.exam_id).await?.context("Exam not found")?;

    let mut images = fetch_images(state.db(), course_id, &submission.id).await?;
    images.sort_by_key(|image| image.order_index);

    if images.is_empty() {
        return flag_submission(
            state.db(),
            course_id,
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
                course_id,
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
            tracing::error!(course_id, submission_id, error = %err, "AI grading failed");
            metrics::counter!("grading_jobs_total", "status" => "failed").increment(1);
            return flag_submission(
                state.db(),
                course_id,
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
            course_id,
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

    repositories::submissions::mark_preliminary(
        state.db(),
        course_id,
        &submission.id,
        repositories::submissions::PreliminaryUpdate {
            ai_score: total_score,
            ai_analysis: result.clone(),
            ai_comments: feedback,
            completed_at,
            duration_seconds: duration,
        },
    )
    .await
    .context("Failed to update submission")?;

    metrics::counter!("grading_jobs_total", "status" => "success").increment(1);
    metrics::histogram!("grading_duration_seconds").record(duration);
    metrics::histogram!("grading_queue_latency_seconds").record(queue_latency);

    if let Some(per_page) = result.get("per_page_transcriptions").and_then(|value| value.as_array())
    {
        for (idx, image) in images.iter().enumerate() {
            if let Some(text) = per_page.get(idx).and_then(|value| value.as_str()) {
                repositories::images::mark_ocr_processed(state.db(), &image.id, text, completed_at)
                    .await
                    .context("Failed to persist OCR transcription")?;
            }
        }
    }

    tracing::info!(course_id, submission_id, "AI grading succeeded");

    Ok(())
}

async fn build_task_prompt(
    pool: &PgPool,
    exam: &Exam,
    session: &ExamSession,
) -> Result<(String, String, Value, f64)> {
    let task_types = repositories::task_types::list_by_exam(pool, &exam.course_id, &exam.id)
        .await
        .context("Failed to fetch task types")?;
    let task_type_ids = task_types.iter().map(|task_type| task_type.id.clone()).collect::<Vec<_>>();
    let variants = repositories::task_types::list_variants_by_task_type_ids(
        pool,
        &exam.course_id,
        &task_type_ids,
    )
    .await
    .context("Failed to fetch variants")?;

    let mut variants_by_task_id =
        std::collections::HashMap::<String, std::collections::HashMap<String, TaskVariant>>::new();
    for variant in variants {
        variants_by_task_id
            .entry(variant.task_type_id.clone())
            .or_default()
            .insert(variant.id.clone(), variant);
    }

    let assignments = &session.variant_assignments.0;
    let mut descriptions = Vec::new();
    let mut reference_solutions = Vec::new();
    let mut rubric_items = Vec::new();
    let mut total_max_score = 0.0;

    for task_type in task_types {
        if let Some(variant_id) = assignments.get(&task_type.id) {
            if let Some(variant) =
                variants_by_task_id.get(&task_type.id).and_then(|variants| variants.get(variant_id))
            {
                descriptions.push(format!(
                    "Задача {}: {}\n{}\n\nВариант:\n{}",
                    task_type.order_index + 1,
                    task_type.title,
                    task_type.description,
                    variant.content
                ));

                if let Some(reference) = variant.reference_solution.as_deref() {
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

async fn fetch_submission(
    pool: &PgPool,
    course_id: &str,
    submission_id: &str,
) -> Result<Option<Submission>> {
    repositories::submissions::find_by_id(pool, course_id, submission_id)
        .await
        .context("Failed to fetch submission")
}

async fn fetch_session(
    pool: &PgPool,
    course_id: &str,
    session_id: &str,
) -> Result<Option<ExamSession>> {
    repositories::sessions::find_by_id(pool, course_id, session_id)
        .await
        .context("Failed to fetch session")
}

async fn fetch_exam(pool: &PgPool, course_id: &str, exam_id: &str) -> Result<Option<Exam>> {
    repositories::exams::find_by_id(pool, course_id, exam_id).await.context("Failed to fetch exam")
}

async fn fetch_images(
    pool: &PgPool,
    course_id: &str,
    submission_id: &str,
) -> Result<Vec<SubmissionImage>> {
    repositories::images::list_by_submission(pool, course_id, submission_id)
        .await
        .context("Failed to fetch images")
}

async fn flag_submission(
    pool: &PgPool,
    course_id: &str,
    submission_id: &str,
    reason: &str,
    flag_reasons: Vec<String>,
    increment_retry: bool,
) -> Result<()> {
    let now = now_primitive();
    repositories::submissions::flag(
        pool,
        course_id,
        submission_id,
        reason,
        flag_reasons,
        now,
        increment_retry,
    )
    .await
    .context("Failed to flag submission")?;

    Ok(())
}
