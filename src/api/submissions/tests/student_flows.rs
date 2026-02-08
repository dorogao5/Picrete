use super::create_published_exam;
use crate::db::types::{SubmissionStatus, UserRole};
use crate::test_support;
use axum::http::{Method, StatusCode};
use serde_json::json;
use tower::ServiceExt;

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
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "response: {error}");
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
async fn submit_exam_does_not_downgrade_processing_submission() {
    let ctx = test_support::setup_test_context().await;

    let teacher = test_support::insert_user(
        ctx.state.db(),
        "000024",
        "Teacher User",
        UserRole::Teacher,
        "teacher-pass",
    )
    .await;
    let student = test_support::insert_user(
        ctx.state.db(),
        "000025",
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
    let session_id = session["id"].as_str().expect("session id").to_string();

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
        .expect("first submit");
    let status = response.status();
    let first_submission = test_support::read_json(response).await;
    assert_eq!(status, StatusCode::OK, "response: {first_submission}");

    sqlx::query("UPDATE submissions SET status = $1 WHERE session_id = $2")
        .bind(SubmissionStatus::Processing)
        .bind(&session_id)
        .execute(ctx.state.db())
        .await
        .expect("mark processing");

    let response = ctx
        .app
        .oneshot(test_support::json_request(
            Method::POST,
            &format!("/api/v1/submissions/sessions/{session_id}/submit"),
            Some(&student_token),
            None,
        ))
        .await
        .expect("second submit");
    let status = response.status();
    let second_submission = test_support::read_json(response).await;
    assert_eq!(status, StatusCode::OK, "response: {second_submission}");
    assert_eq!(second_submission["status"], "processing");
}
