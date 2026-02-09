use anyhow::{Context, Result};
use time::OffsetDateTime;

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

pub(crate) async fn retry_failed_submissions(state: &AppState) -> Result<()> {
    let submissions = repositories::submissions::list_flagged_for_retry(state.db(), 3)
        .await
        .context("Failed to fetch failed submissions")?;

    let mut retried = 0;
    let now = now_primitive();

    for (submission_id, course_id) in submissions {
        let updated =
            repositories::submissions::requeue_failed(state.db(), &course_id, &submission_id, now)
                .await
                .context("Failed to requeue failed submission")?;

        if updated {
            retried += 1;
        }
    }

    tracing::info!(retried_submissions = retried, "Retried failed submissions");
    metrics::counter!("submissions_retried_total").increment(retried as u64);

    Ok(())
}
