use anyhow::{anyhow, Context, Result};
use uuid::Uuid;

use crate::core::state::AppState;
use crate::core::time::primitive_now_utc;
use crate::db::models::{ExamSession, Submission};
use crate::schemas::submission::{
    SubmissionImageResponse, SubmissionNextStep, SubmissionScoreResponse,
};
use crate::services::work_processing::WorkProcessingSettings;

#[derive(Debug, Clone, Copy)]
pub(crate) enum FinalizeMode {
    ManualSubmit,
    AutoDeadline,
}

#[derive(Debug)]
pub(crate) struct FinalizeSubmissionResult {
    pub(crate) submission: Submission,
    pub(crate) images: Vec<SubmissionImageResponse>,
    pub(crate) scores: Vec<SubmissionScoreResponse>,
    pub(crate) next_step: SubmissionNextStep,
}

pub(crate) async fn finalize_submission(
    state: &AppState,
    session: &ExamSession,
    mode: FinalizeMode,
    submitted_at: time::PrimitiveDateTime,
) -> Result<FinalizeSubmissionResult> {
    let now = primitive_now_utc();

    let max_score = crate::repositories::exams::max_score_for_exam(
        state.db(),
        &session.course_id,
        &session.exam_id,
    )
    .await
    .context("Failed to fetch max score")?;

    let submission_id = Uuid::new_v4().to_string();
    crate::repositories::submissions::create_if_absent(
        state.db(),
        &submission_id,
        &session.course_id,
        &session.id,
        &session.student_id,
        crate::db::types::SubmissionStatus::Uploaded,
        max_score,
        submitted_at,
        now,
    )
    .await
    .context("Failed to create submission")?;

    let exam =
        crate::repositories::exams::find_by_id(state.db(), &session.course_id, &session.exam_id)
            .await
            .context("Failed to fetch exam")?
            .ok_or_else(|| anyhow!("Exam not found"))?;
    let processing = WorkProcessingSettings::from_exam_settings_strict(&exam.settings.0)
        .map_err(|e| anyhow!("Invalid processing settings: {e}"))?;

    match mode {
        FinalizeMode::ManualSubmit => {
            crate::repositories::sessions::submit(
                state.db(),
                &session.course_id,
                &session.id,
                submitted_at,
            )
            .await
            .context("Failed to set session submitted status")?;
        }
        FinalizeMode::AutoDeadline => {
            crate::repositories::sessions::expire_with_deadline(
                state.db(),
                &session.course_id,
                &session.id,
                submitted_at,
                now,
            )
            .await
            .context("Failed to expire session with deadline")?;
        }
    }

    crate::repositories::submissions::configure_pipeline_after_submit(
        state.db(),
        &session.course_id,
        &session.id,
        processing.ocr_enabled,
        now,
    )
    .await
    .context("Failed to configure submission pipeline")?;

    let submission = crate::repositories::submissions::find_by_session(
        state.db(),
        &session.course_id,
        &session.id,
    )
    .await
    .context("Failed to fetch finalized submission")?
    .ok_or_else(|| anyhow!("Submission missing after finalize"))?;

    let images = crate::api::submissions::helpers::fetch_images(
        state.db(),
        &session.course_id,
        &submission.id,
    )
    .await
    .map_err(|e| anyhow!("Failed to fetch submission images: {e:?}"))?;
    let scores = crate::api::submissions::helpers::fetch_scores(
        state.db(),
        &session.course_id,
        &submission.id,
    )
    .await
    .map_err(|e| anyhow!("Failed to fetch submission scores: {e:?}"))?;

    let next_step = if processing.ocr_enabled {
        SubmissionNextStep::OcrReview
    } else {
        SubmissionNextStep::Result
    };

    Ok(FinalizeSubmissionResult { submission, images, scores, next_step })
}
