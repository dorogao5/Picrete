use anyhow::{Context, Result};
use time::{Duration, OffsetDateTime};

use crate::core::state::AppState;
use crate::core::time::primitive_now_utc as now_primitive;
use crate::repositories;

pub(crate) async fn process_completed_exams(state: &AppState) -> Result<()> {
    let now = now_primitive();

    let exams = repositories::exams::list_ready_to_complete(state.db(), now)
        .await
        .context("Failed to fetch completed exams")?;

    if exams.is_empty() {
        return Ok(());
    }

    let mut queued = 0;

    for exam in &exams {
        let queued_ids = repositories::submissions::queue_uploaded_for_processing_by_exam(
            state.db(),
            &exam.course_id,
            &exam.id,
            now,
        )
        .await
        .context("Failed to queue submissions for processing")?;
        queued += queued_ids.len();

        repositories::exams::mark_completed(state.db(), &exam.course_id, &exam.id, now)
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
    let sessions = repositories::sessions::list_active_with_exam_end(state.db())
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

        repositories::sessions::expire_with_deadline(
            state.db(),
            &session.course_id,
            &session.id,
            hard_deadline,
            now_primitive(),
        )
        .await
        .context("Failed to expire session")?;

        closed += 1;
    }

    tracing::info!(closed_sessions = closed, "Closed expired sessions");
    metrics::counter!("expired_sessions_closed_total").increment(closed as u64);

    Ok(())
}

pub(crate) async fn retry_failed_ocr_submissions(state: &AppState) -> Result<()> {
    let max_retry_count = state.settings().datalab().max_submit_retries as i32;
    let submissions =
        repositories::submissions::list_failed_ocr_for_retry(state.db(), max_retry_count)
            .await
            .context("Failed to fetch OCR-failed submissions")?;

    let mut retried = 0;
    let now = now_primitive();

    for (submission_id, course_id) in submissions {
        repositories::images::reset_ocr_by_submission(state.db(), &course_id, &submission_id)
            .await
            .context("Failed to reset image OCR state")?;
        repositories::ocr_reviews::clear_reviews_by_submission(
            state.db(),
            &course_id,
            &submission_id,
        )
        .await
        .context("Failed to clear OCR reviews before retry")?;

        let updated = repositories::submissions::requeue_failed_ocr(
            state.db(),
            &course_id,
            &submission_id,
            now,
        )
        .await
        .context("Failed to requeue OCR-failed submission")?;

        if updated {
            retried += 1;
        }
    }

    tracing::info!(retried_submissions = retried, "Retried failed OCR submissions");
    metrics::counter!("ocr_submissions_retried_total").increment(retried as u64);

    Ok(())
}

pub(crate) async fn recover_stale_processing_submissions(state: &AppState) -> Result<()> {
    let now = now_primitive();

    let ocr_timeout_seconds = state
        .settings()
        .datalab()
        .timeout_seconds
        .saturating_add(
            state
                .settings()
                .datalab()
                .timeout_seconds
                .saturating_mul(state.settings().datalab().max_poll_attempts as u64),
        )
        .saturating_add(
            state
                .settings()
                .datalab()
                .poll_interval_seconds
                .saturating_mul(state.settings().datalab().max_poll_attempts as u64),
        )
        .saturating_add(120);
    let llm_timeout_seconds = state.settings().ai().ai_request_timeout.saturating_add(120);

    let stale_ocr_before = now - seconds_as_duration(ocr_timeout_seconds);
    let stale_llm_before = now - seconds_as_duration(llm_timeout_seconds);

    let stale_ocr =
        repositories::submissions::list_stale_ocr_processing(state.db(), stale_ocr_before)
            .await
            .context("Failed to list stale OCR processing submissions")?;
    let stale_llm =
        repositories::submissions::list_stale_llm_processing(state.db(), stale_llm_before)
            .await
            .context("Failed to list stale LLM processing submissions")?;

    let mut recovered_ocr = 0;
    let mut recovered_llm = 0;

    for (submission_id, course_id) in stale_ocr {
        let reason = "OCR processing timed out while waiting for worker completion";
        repositories::images::mark_stale_processing_failed_by_submission(
            state.db(),
            &course_id,
            &submission_id,
            reason,
            now,
        )
        .await
        .context("Failed to mark stale OCR image state")?;

        repositories::submissions::mark_ocr_failed(
            state.db(),
            &course_id,
            &submission_id,
            reason,
            now,
        )
        .await
        .context("Failed to mark stale OCR submission as failed")?;
        recovered_ocr += 1;
    }

    for (submission_id, course_id) in stale_llm {
        let reason = "LLM precheck processing timed out while waiting for worker completion";
        repositories::submissions::mark_llm_precheck_failed(
            state.db(),
            &course_id,
            &submission_id,
            reason,
            now,
        )
        .await
        .context("Failed to mark stale LLM submission as failed")?;
        recovered_llm += 1;
    }

    if recovered_ocr > 0 || recovered_llm > 0 {
        tracing::warn!(recovered_ocr, recovered_llm, "Recovered stale processing submissions");
    }

    metrics::counter!("ocr_processing_stale_recovered_total").increment(recovered_ocr as u64);
    metrics::counter!("llm_processing_stale_recovered_total").increment(recovered_llm as u64);

    Ok(())
}

fn seconds_as_duration(seconds: u64) -> Duration {
    Duration::seconds(seconds.min(i64::MAX as u64) as i64)
}
