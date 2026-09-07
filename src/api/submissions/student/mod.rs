mod list;
mod ocr;
mod session;
mod submit;
mod upload;

fn feedback_is_released(
    session: &crate::db::models::ExamSession,
    exam: &crate::db::models::Exam,
) -> bool {
    session.attempt_number >= exam.max_attempts
        || crate::core::time::primitive_now_utc() >= exam.end_time
        || matches!(exam.status, crate::db::types::ExamStatus::Completed)
}

pub(super) use list::get_my_submissions;
pub(super) use ocr::{finalize_ocr_review, get_ocr_pages, review_ocr_page};
pub(super) use session::{auto_save, enter_exam, get_session_variant};
pub(super) use submit::{get_session_result, submit_exam};
pub(super) use upload::{
    delete_session_image, list_session_images, presigned_upload_url, upload_image,
};

fn redact_submission_feedback(response: &mut crate::schemas::submission::SubmissionResponse) {
    response.ai_score = None;
    response.final_score = None;
    response.ai_analysis = None;
    response.ai_comments = None;
    response.teacher_comments = None;
    response.report_summary = None;
    response.ocr_error = None;
    response.llm_error = None;
    response.scores.clear();
    response.flag_reasons.clear();
    response.reviewed_by = None;
    response.reviewed_at = None;
}
