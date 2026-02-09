use sqlx::types::Json;
use sqlx::PgPool;
use time::PrimitiveDateTime;

use crate::db::types::SubmissionStatus;

use super::types::PreliminaryUpdate;

pub(crate) async fn claim_next_for_processing(
    pool: &PgPool,
    now: PrimitiveDateTime,
) -> Result<Option<(String, String)>, sqlx::Error> {
    sqlx::query_as::<_, (String, String)>(
        "WITH candidate AS (
            SELECT s.id, s.course_id
            FROM submissions s
            JOIN exam_sessions es
              ON es.course_id = s.course_id
             AND es.id = s.session_id
            WHERE s.status IN ($1, $2)
              AND s.ai_request_started_at IS NULL
              AND es.submitted_at IS NOT NULL
            ORDER BY CASE WHEN s.status = $1 THEN 0 ELSE 1 END,
                     COALESCE(s.ai_retry_count, 0),
                     s.created_at
            FOR UPDATE OF s SKIP LOCKED
            LIMIT 1
        )
        UPDATE submissions
        SET status = $3,
            ai_request_started_at = $4,
            ai_error = NULL
        FROM candidate
        WHERE submissions.id = candidate.id
          AND submissions.course_id = candidate.course_id
        RETURNING submissions.id, submissions.course_id",
    )
    .bind(SubmissionStatus::Uploaded)
    .bind(SubmissionStatus::Processing)
    .bind(SubmissionStatus::Processing)
    .bind(now)
    .fetch_optional(pool)
    .await
}

pub(crate) async fn mark_preliminary(
    pool: &PgPool,
    course_id: &str,
    submission_id: &str,
    params: PreliminaryUpdate,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE submissions
         SET status = $1,
             ai_score = $2,
             ai_analysis = $3,
             ai_comments = $4,
             ai_processed_at = $5,
             ai_request_completed_at = $6,
             ai_request_duration_seconds = $7,
             ai_error = NULL,
             is_flagged = FALSE,
             flag_reasons = $8,
             updated_at = $9
         WHERE course_id = $10 AND id = $11",
    )
    .bind(SubmissionStatus::Preliminary)
    .bind(params.ai_score)
    .bind(Json(params.ai_analysis))
    .bind(params.ai_comments)
    .bind(params.completed_at)
    .bind(params.completed_at)
    .bind(params.duration_seconds)
    .bind(Json(Vec::<String>::new()))
    .bind(params.completed_at)
    .bind(course_id)
    .bind(submission_id)
    .execute(pool)
    .await?;

    Ok(())
}

pub(crate) async fn queue_uploaded_for_processing_by_exam(
    pool: &PgPool,
    course_id: &str,
    exam_id: &str,
    now: PrimitiveDateTime,
) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar::<_, String>(
        "UPDATE submissions s
         SET status = $1,
             ai_request_started_at = NULL,
             updated_at = $2
         FROM exam_sessions es
         WHERE es.course_id = s.course_id
           AND es.id = s.session_id
           AND s.course_id = $3
           AND es.exam_id = $4
           AND s.status = $5
           AND s.ai_request_started_at IS NULL
         RETURNING s.id",
    )
    .bind(SubmissionStatus::Processing)
    .bind(now)
    .bind(course_id)
    .bind(exam_id)
    .bind(SubmissionStatus::Uploaded)
    .fetch_all(pool)
    .await
}

pub(crate) async fn requeue_failed(
    pool: &PgPool,
    course_id: &str,
    submission_id: &str,
    now: PrimitiveDateTime,
) -> Result<bool, sqlx::Error> {
    let updated = sqlx::query(
        "UPDATE submissions
         SET status = $1,
             ai_retry_count = COALESCE(ai_retry_count, 0) + 1,
             ai_error = NULL,
             ai_request_started_at = NULL,
             updated_at = $2
         WHERE course_id = $3 AND id = $4",
    )
    .bind(SubmissionStatus::Processing)
    .bind(now)
    .bind(course_id)
    .bind(submission_id)
    .execute(pool)
    .await?;

    Ok(updated.rows_affected() > 0)
}

pub(crate) async fn flag(
    pool: &PgPool,
    course_id: &str,
    submission_id: &str,
    reason: &str,
    flag_reasons: Vec<String>,
    now: PrimitiveDateTime,
    increment_retry: bool,
) -> Result<(), sqlx::Error> {
    if increment_retry {
        sqlx::query(
            "UPDATE submissions
             SET status = $1,
                 ai_error = $2,
                 is_flagged = TRUE,
                 flag_reasons = $3,
                 ai_request_completed_at = $4,
                 ai_request_duration_seconds = $5,
                 ai_retry_count = COALESCE(ai_retry_count,0) + 1,
                 updated_at = $6
             WHERE course_id = $7 AND id = $8",
        )
        .bind(SubmissionStatus::Flagged)
        .bind(reason)
        .bind(Json(flag_reasons))
        .bind(now)
        .bind(0.0)
        .bind(now)
        .bind(course_id)
        .bind(submission_id)
        .execute(pool)
        .await?;
    } else {
        sqlx::query(
            "UPDATE submissions
             SET status = $1,
                 ai_error = $2,
                 is_flagged = TRUE,
                 flag_reasons = $3,
                 ai_request_completed_at = $4,
                 ai_request_duration_seconds = $5,
                 updated_at = $6
             WHERE course_id = $7 AND id = $8",
        )
        .bind(SubmissionStatus::Flagged)
        .bind(reason)
        .bind(Json(flag_reasons))
        .bind(now)
        .bind(0.0)
        .bind(now)
        .bind(course_id)
        .bind(submission_id)
        .execute(pool)
        .await?;
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn create_if_absent(
    pool: &PgPool,
    id: &str,
    course_id: &str,
    session_id: &str,
    student_id: &str,
    status: SubmissionStatus,
    max_score: f64,
    submitted_at: PrimitiveDateTime,
    now: PrimitiveDateTime,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO submissions (id, course_id, session_id, student_id, status, max_score, submitted_at, created_at, updated_at)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
         ON CONFLICT (session_id) DO NOTHING",
    )
    .bind(id)
    .bind(course_id)
    .bind(session_id)
    .bind(student_id)
    .bind(status)
    .bind(max_score)
    .bind(submitted_at)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn update_status_by_session(
    pool: &PgPool,
    course_id: &str,
    session_id: &str,
    status: SubmissionStatus,
    submitted_at: PrimitiveDateTime,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE submissions
         SET status = $1, submitted_at = $2, updated_at = $2
         WHERE course_id = $3
           AND session_id = $4
           AND status NOT IN ($5, $6, $7, $8, $9)",
    )
    .bind(status)
    .bind(submitted_at)
    .bind(course_id)
    .bind(session_id)
    .bind(SubmissionStatus::Processing)
    .bind(SubmissionStatus::Preliminary)
    .bind(SubmissionStatus::Approved)
    .bind(SubmissionStatus::Flagged)
    .bind(SubmissionStatus::Rejected)
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn approve(
    pool: &PgPool,
    course_id: &str,
    id: &str,
    ai_score: Option<f64>,
    teacher_comments: Option<String>,
    reviewed_by: String,
    now: PrimitiveDateTime,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE submissions
         SET status = $1,
             final_score = $2,
             teacher_comments = $3,
             reviewed_by = $4,
             reviewed_at = $5
         WHERE course_id = $6 AND id = $7",
    )
    .bind(SubmissionStatus::Approved)
    .bind(ai_score)
    .bind(teacher_comments)
    .bind(reviewed_by)
    .bind(now)
    .bind(course_id)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn override_score(
    pool: &PgPool,
    course_id: &str,
    id: &str,
    final_score: f64,
    teacher_comments: String,
    reviewed_by: String,
    now: PrimitiveDateTime,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE submissions
         SET final_score = $1,
             teacher_comments = $2,
             status = $3,
             reviewed_by = $4,
             reviewed_at = $5
         WHERE course_id = $6 AND id = $7",
    )
    .bind(final_score)
    .bind(teacher_comments)
    .bind(SubmissionStatus::Approved)
    .bind(reviewed_by)
    .bind(now)
    .bind(course_id)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn queue_regrade(
    pool: &PgPool,
    course_id: &str,
    id: &str,
    now: PrimitiveDateTime,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE submissions
         SET status = $1,
             ai_retry_count = COALESCE(ai_retry_count,0) + 1,
             ai_error = NULL,
             ai_request_started_at = NULL,
             ai_request_completed_at = NULL,
             ai_request_duration_seconds = NULL,
             ai_processed_at = NULL,
             is_flagged = FALSE,
             flag_reasons = $2,
             updated_at = $3
         WHERE course_id = $4 AND id = $5",
    )
    .bind(SubmissionStatus::Processing)
    .bind(sqlx::types::Json(Vec::<String>::new()))
    .bind(now)
    .bind(course_id)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use time::Duration;
    use uuid::Uuid;

    use super::claim_next_for_processing;
    use crate::core::time::primitive_now_utc;
    use crate::db::types::{CourseRole, ExamStatus, SessionStatus, SubmissionStatus};
    use crate::repositories;
    use crate::test_support;

    #[tokio::test]
    async fn claim_next_for_processing_skips_active_unsubmitted_sessions() {
        let ctx = test_support::setup_test_context().await;
        let db = ctx.state.db();

        let teacher =
            test_support::insert_user(db, "teacher_claim", "Teacher", "Password123").await;
        let student =
            test_support::insert_user(db, "student_claim", "Student", "Password123").await;
        let course = test_support::create_course_with_teacher(
            db,
            "claim-course",
            "Claim Course",
            &teacher.id,
        )
        .await;
        test_support::add_course_role(db, &course.id, &student.id, CourseRole::Student).await;

        let now = primitive_now_utc();
        let exam_id = Uuid::new_v4().to_string();
        repositories::exams::create(
            db,
            repositories::exams::CreateExam {
                id: &exam_id,
                course_id: &course.id,
                title: "Claim Exam",
                description: None,
                start_time: now - Duration::hours(1),
                end_time: now + Duration::hours(1),
                duration_minutes: 60,
                timezone: "UTC",
                max_attempts: 1,
                allow_breaks: false,
                break_duration_minutes: 0,
                auto_save_interval: 30,
                status: ExamStatus::Published,
                created_by: &teacher.id,
                created_at: now,
                updated_at: now,
                settings: serde_json::json!({}),
            },
        )
        .await
        .expect("create exam");

        let session_id = Uuid::new_v4().to_string();
        repositories::sessions::create(
            db,
            repositories::sessions::CreateSession {
                id: &session_id,
                course_id: &course.id,
                exam_id: &exam_id,
                student_id: &student.id,
                variant_seed: 1,
                variant_assignments: serde_json::json!({}),
                started_at: now,
                expires_at: now + Duration::minutes(60),
                status: SessionStatus::Active,
                attempt_number: 1,
                created_at: now,
                updated_at: now,
            },
        )
        .await
        .expect("create session");

        let submission_id = Uuid::new_v4().to_string();
        repositories::submissions::create_if_absent(
            db,
            &submission_id,
            &course.id,
            &session_id,
            &student.id,
            SubmissionStatus::Uploaded,
            100.0,
            now,
            now,
        )
        .await
        .expect("create submission");

        let claimed_before_submit =
            claim_next_for_processing(db, now).await.expect("claim before submit");
        assert!(claimed_before_submit.is_none());

        repositories::sessions::submit(db, &course.id, &session_id, now)
            .await
            .expect("submit session");

        let claimed_after_submit =
            claim_next_for_processing(db, now).await.expect("claim after submit");
        assert_eq!(claimed_after_submit, Some((submission_id, course.id)));
    }
}
