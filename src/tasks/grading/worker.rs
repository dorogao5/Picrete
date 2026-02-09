use anyhow::{Context, Result};
use serde_json::{json, Value};
use sqlx::PgPool;

use crate::core::state::AppState;
use crate::core::time::primitive_now_utc as now_primitive;
use crate::db::models::{Exam, ExamSession, Submission, SubmissionImage, TaskVariant};
use crate::db::types::{OcrOverallStatus, SubmissionStatus};
use crate::repositories;
use crate::services::ai_grading::{AiGradingService, LlmPrecheckRequest};
use crate::services::datalab_ocr::DatalabOcrService;
use crate::services::work_processing::WorkProcessingSettings;

pub(crate) async fn claim_next_ocr_submission(pool: &PgPool) -> Result<Option<(String, String)>> {
    let now = now_primitive();
    repositories::submissions::claim_next_for_ocr(pool, now)
        .await
        .context("Failed to claim submission for OCR")
}

pub(crate) async fn claim_next_llm_submission(pool: &PgPool) -> Result<Option<(String, String)>> {
    let now = now_primitive();
    repositories::submissions::claim_next_for_llm_precheck(pool, now)
        .await
        .context("Failed to claim submission for LLM precheck")
}

pub(crate) async fn process_submission_ocr(
    state: &AppState,
    datalab: &DatalabOcrService,
    course_id: &str,
    submission_id: &str,
) -> Result<()> {
    let submission = fetch_submission(state.db(), course_id, submission_id)
        .await?
        .context("Submission not found")?;

    if submission.ocr_overall_status != OcrOverallStatus::Processing {
        tracing::info!(
            course_id,
            submission_id,
            ocr_status = ?submission.ocr_overall_status,
            "Skipping OCR processing for submission"
        );
        return Ok(());
    }

    let session = fetch_session(state.db(), course_id, &submission.session_id)
        .await?
        .context("Session not found")?;
    let exam =
        fetch_exam(state.db(), course_id, &session.exam_id).await?.context("Exam not found")?;
    let processing = WorkProcessingSettings::from_exam_settings(&exam.settings.0);

    if !processing.ocr_enabled {
        repositories::submissions::skip_llm_precheck_after_ocr(
            state.db(),
            course_id,
            &submission.id,
            OcrOverallStatus::NotRequired,
            false,
            None,
            now_primitive(),
        )
        .await
        .context("Failed to bypass OCR for disabled work")?;
        return Ok(());
    }

    let mut images = fetch_images(state.db(), course_id, &submission.id).await?;
    images.sort_by_key(|image| image.order_index);

    if images.is_empty() {
        mark_ocr_failed(
            state.db(),
            course_id,
            &submission.id,
            "No images available for OCR",
            vec!["no_images".to_string()],
        )
        .await?;
        return Ok(());
    }

    let Some(storage) = state.storage() else {
        mark_ocr_failed(
            state.db(),
            course_id,
            &submission.id,
            "S3 storage not configured for OCR processing",
            vec!["storage_unavailable".to_string()],
        )
        .await?;
        return Ok(());
    };
    let started_at = now_primitive();
    let mut processed_images = 0_u64;

    for image in &images {
        if !image.file_path.starts_with("submissions/") {
            mark_ocr_failed(
                state.db(),
                course_id,
                &submission.id,
                "Image is stored in unsupported local storage path",
                vec!["storage_mismatch".to_string()],
            )
            .await?;
            return Ok(());
        }

        repositories::images::mark_ocr_processing(state.db(), &image.id, None, now_primitive())
            .await
            .context("Failed to mark image OCR processing")?;

        let file_url = storage
            .presign_get(&image.file_path, std::time::Duration::from_secs(900))
            .await
            .context("Failed to generate presigned URL for OCR")?;

        let result = datalab
            .run_marker_for_file_url(&file_url)
            .await
            .with_context(|| format!("DataLab OCR failed for image {}", image.id));

        let result = match result {
            Ok(value) => value,
            Err(err) => {
                repositories::images::mark_ocr_failed(
                    state.db(),
                    &image.id,
                    &err.to_string(),
                    now_primitive(),
                )
                .await
                .context("Failed to update image OCR failure state")?;
                mark_ocr_failed(
                    state.db(),
                    course_id,
                    &submission.id,
                    &err.to_string(),
                    vec!["ocr_processing_error".to_string()],
                )
                .await?;
                return Ok(());
            }
        };

        if let Some(chunks) = result.chunks.as_ref() {
            if !chunks_have_geometry(chunks) {
                let reason =
                    "OCR chunks do not contain geometry (bbox/polygon), cannot persist training data";
                repositories::images::mark_ocr_failed(
                    state.db(),
                    &image.id,
                    reason,
                    now_primitive(),
                )
                .await
                .context("Failed to mark image OCR geometry failure")?;
                mark_ocr_failed(
                    state.db(),
                    course_id,
                    &submission.id,
                    reason,
                    vec!["ocr_geometry_missing".to_string()],
                )
                .await?;
                return Ok(());
            }
        } else {
            let reason = "OCR chunks are missing in DataLab response";
            repositories::images::mark_ocr_failed(state.db(), &image.id, reason, now_primitive())
                .await
                .context("Failed to mark image OCR missing chunks")?;
            mark_ocr_failed(
                state.db(),
                course_id,
                &submission.id,
                reason,
                vec!["ocr_chunks_missing".to_string()],
            )
            .await?;
            return Ok(());
        }

        let completed_at = now_primitive();
        let ocr_markdown = result.markdown.clone();
        let ocr_text = ocr_markdown.clone();

        repositories::images::mark_ocr_ready(
            state.db(),
            &image.id,
            ocr_text.as_deref(),
            ocr_markdown.as_deref(),
            result.chunks.as_ref(),
            result.model.as_deref(),
            completed_at,
        )
        .await
        .context("Failed to persist OCR result")?;

        processed_images += 1;
    }

    repositories::ocr_reviews::clear_reviews_by_submission(state.db(), course_id, &submission.id)
        .await
        .context("Failed to clear stale OCR reviews")?;
    repositories::submissions::mark_ocr_in_review(
        state.db(),
        course_id,
        &submission.id,
        now_primitive(),
    )
    .await
    .context("Failed to mark submission OCR in_review")?;

    let duration = (now_primitive().assume_utc() - started_at.assume_utc()).as_seconds_f64();
    metrics::counter!("ocr_jobs_total", "status" => "success").increment(1);
    metrics::histogram!("ocr_job_duration_seconds").record(duration);
    metrics::counter!("ocr_images_processed_total").increment(processed_images);
    tracing::info!(course_id, submission_id, images = processed_images, "OCR processing completed");

    Ok(())
}

pub(crate) async fn run_llm_precheck(
    state: &AppState,
    ai: &AiGradingService,
    course_id: &str,
    submission_id: &str,
) -> Result<()> {
    let submission = fetch_submission(state.db(), course_id, submission_id)
        .await?
        .context("Submission not found")?;

    if submission.status != SubmissionStatus::Processing {
        tracing::info!(
            course_id,
            submission_id,
            status = ?submission.status,
            "Skipping LLM precheck for submission"
        );
        return Ok(());
    }

    let session = fetch_session(state.db(), course_id, &submission.session_id)
        .await?
        .context("Session not found")?;
    let exam =
        fetch_exam(state.db(), course_id, &session.exam_id).await?.context("Exam not found")?;
    let mut images = fetch_images(state.db(), course_id, &submission.id).await?;
    images.sort_by_key(|image| image.order_index);

    let ocr_pages = images
        .iter()
        .map(|image| {
            image.ocr_markdown.clone().or_else(|| image.ocr_text.clone()).unwrap_or_default()
        })
        .collect::<Vec<_>>();

    if ocr_pages.is_empty() || ocr_pages.iter().all(|page| page.trim().is_empty()) {
        repositories::submissions::mark_llm_precheck_failed(
            state.db(),
            course_id,
            &submission.id,
            "LLM precheck cannot start: OCR markdown is missing",
            now_primitive(),
        )
        .await
        .context("Failed to mark LLM precheck failure")?;
        metrics::counter!("llm_precheck_jobs_total", "status" => "missing_ocr").increment(1);
        return Ok(());
    }

    let issues =
        repositories::ocr_reviews::list_issues_by_submission(state.db(), course_id, &submission.id)
            .await
            .context("Failed to fetch OCR issues for precheck")?;
    let issue_payload = issues
        .into_iter()
        .map(|issue| {
            json!({
                "image_id": issue.image_id,
                "anchor": issue.anchor.0,
                "original_text": issue.original_text,
                "suggested_text": issue.suggested_text,
                "note": issue.note,
                "severity": issue.severity,
            })
        })
        .collect::<Vec<_>>();

    let (task_description, reference_solution, rubric, total_max_score) =
        build_task_prompt(state.db(), &exam, &session).await?;

    let request = LlmPrecheckRequest {
        submission_id: Some(submission.id.clone()),
        ocr_markdown_pages: ocr_pages,
        ocr_report_issues: issue_payload,
        report_summary: submission.report_summary.clone(),
        task_description,
        reference_solution,
        rubric,
        max_score: total_max_score,
        chemistry_rules: None,
    };

    let started_at = submission.ai_request_started_at.unwrap_or_else(now_primitive);
    let queue_latency =
        (started_at.assume_utc() - submission.created_at.assume_utc()).as_seconds_f64();

    let mut result = match ai.run_precheck(request).await {
        Ok(value) => value,
        Err(err) => {
            repositories::submissions::mark_llm_precheck_failed(
                state.db(),
                course_id,
                &submission.id,
                &err.to_string(),
                now_primitive(),
            )
            .await
            .context("Failed to mark LLM precheck failure")?;
            metrics::counter!("llm_precheck_jobs_total", "status" => "failed").increment(1);
            return Ok(());
        }
    };

    let unreadable = result.get("unreadable").and_then(Value::as_bool).unwrap_or(false);
    if unreadable {
        let reason = result
            .get("unreadable_reason")
            .and_then(Value::as_str)
            .unwrap_or("OCR text is unreadable for LLM precheck");
        repositories::submissions::mark_llm_precheck_failed(
            state.db(),
            course_id,
            &submission.id,
            reason,
            now_primitive(),
        )
        .await
        .context("Failed to mark LLM unreadable failure")?;
        metrics::counter!("llm_precheck_jobs_total", "status" => "unreadable").increment(1);
        return Ok(());
    }

    if let Some(map) = result.as_object_mut() {
        map.remove("_metadata");
    }

    let total_score = result.get("total_score").and_then(Value::as_f64);
    let feedback = result.get("feedback").and_then(Value::as_str).map(|value| value.to_string());
    let completed_at = now_primitive();
    let duration = (completed_at.assume_utc() - started_at.assume_utc()).as_seconds_f64();

    repositories::submissions::mark_preliminary(
        state.db(),
        course_id,
        &submission.id,
        repositories::submissions::PreliminaryUpdate {
            ai_score: total_score,
            ai_analysis: result,
            ai_comments: feedback,
            completed_at,
            duration_seconds: duration,
        },
    )
    .await
    .context("Failed to persist LLM precheck result")?;

    metrics::counter!("llm_precheck_jobs_total", "status" => "success").increment(1);
    metrics::histogram!("llm_precheck_duration_seconds").record(duration);
    metrics::histogram!("llm_precheck_queue_latency_seconds").record(queue_latency);
    tracing::info!(course_id, submission_id, "LLM precheck completed");

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
        total_max_score = exam.settings.0.get("max_score").and_then(Value::as_f64).unwrap_or(100.0);
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

async fn mark_ocr_failed(
    pool: &PgPool,
    course_id: &str,
    submission_id: &str,
    reason: &str,
    flag_reasons: Vec<String>,
) -> Result<()> {
    let now = now_primitive();
    repositories::submissions::mark_ocr_failed(pool, course_id, submission_id, reason, now)
        .await
        .context("Failed to mark OCR failure")?;
    metrics::counter!("ocr_jobs_total", "status" => "failed").increment(1);
    if !flag_reasons.is_empty() {
        tracing::warn!(course_id, submission_id, reason, reasons = ?flag_reasons, "OCR failure");
    }
    Ok(())
}

fn chunks_have_geometry(chunks: &Value) -> bool {
    if let Some(blocks) = chunks.get("blocks").and_then(Value::as_array) {
        return blocks.iter().any(block_has_geometry);
    }

    if let Some(items) = chunks.as_array() {
        return items.iter().any(block_has_geometry);
    }

    false
}

fn block_has_geometry(block: &Value) -> bool {
    let has_bbox =
        block.get("bbox").and_then(Value::as_array).map(|bbox| bbox.len() >= 4).unwrap_or(false);
    let has_polygon = block
        .get("polygon")
        .and_then(Value::as_array)
        .map(|polygon| polygon.len() >= 4)
        .unwrap_or(false);

    has_bbox || has_polygon
}
