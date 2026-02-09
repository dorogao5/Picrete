use super::{create_published_exam, login_student, signup_student};
use crate::db::types::CourseRole;
use crate::test_support;
use axum::http::{Method, StatusCode};
use serde_json::json;
use tower::ServiceExt;

#[tokio::test]
async fn full_flow_signup_login_submit_and_approve() {
    let ctx = test_support::setup_test_context_with_storage().await;

    let teacher =
        test_support::insert_user(ctx.state.db(), "teacher040", "Teacher User", "teacher-pass")
            .await;
    let course = test_support::create_course_with_teacher(
        ctx.state.db(),
        "full-flow-101",
        "Full Flow 101",
        &teacher.id,
    )
    .await;
    let student_invite =
        test_support::create_active_invite_code(ctx.state.db(), &course, CourseRole::Student).await;
    let teacher_token = test_support::bearer_token(&teacher.id, ctx.state.settings());

    let (student_token, _student_id) = signup_student(
        ctx.app.clone(),
        "student041",
        "Student User",
        "student-pass",
        Some(&student_invite),
    )
    .await;
    let login_token = login_student(ctx.app.clone(), "student041", "student-pass").await;
    assert!(!login_token.is_empty());

    let exam_id = create_published_exam(ctx.app.clone(), &teacher_token, &course.id).await;

    let response = ctx
        .app
        .clone()
        .oneshot(test_support::json_request(
            Method::POST,
            &format!("/api/v1/courses/{}/submissions/exams/{exam_id}/enter", course.id),
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
        .clone()
        .oneshot(test_support::json_request(
            Method::POST,
            &format!(
                "/api/v1/courses/{}/submissions/sessions/{session_id}/presigned-upload-url?filename=work.png&content_type=image/png",
                course.id
            ),
            Some(&student_token),
            None,
        ))
        .await
        .expect("presign url");

    let status = response.status();
    let presign = test_support::read_json(response).await;
    assert_eq!(status, StatusCode::OK, "response: {presign}");
    assert!(presign["upload_url"].as_str().unwrap_or("").contains("work.png"));

    let response = ctx
        .app
        .clone()
        .oneshot(test_support::json_request(
            Method::POST,
            &format!("/api/v1/courses/{}/submissions/sessions/{session_id}/submit", course.id),
            Some(&student_token),
            None,
        ))
        .await
        .expect("submit exam");

    let status = response.status();
    let submission = test_support::read_json(response).await;
    assert_eq!(status, StatusCode::OK, "response: {submission}");
    let submission_id = submission["id"].as_str().expect("submission id");

    let response = ctx
        .app
        .clone()
        .oneshot(test_support::json_request(
            Method::POST,
            &format!("/api/v1/courses/{}/submissions/{submission_id}/regrade", course.id),
            Some(&teacher_token),
            None,
        ))
        .await
        .expect("regrade");

    let status = response.status();
    let regrade = test_support::read_json(response).await;
    assert_eq!(status, StatusCode::OK, "response: {regrade}");
    assert_eq!(regrade["status"], "processing");

    // Use override-score instead of approve (approve requires ai_score which is not
    // available in tests since AI grading doesn't run)
    let response = ctx
        .app
        .clone()
        .oneshot(test_support::json_request(
            Method::POST,
            &format!("/api/v1/courses/{}/submissions/{submission_id}/override-score", course.id),
            Some(&teacher_token),
            Some(json!({"final_score": 8.5, "teacher_comments": "Looks good"})),
        ))
        .await
        .expect("override score");

    let status = response.status();
    let override_resp = test_support::read_json(response).await;
    assert_eq!(status, StatusCode::OK, "response: {override_resp}");

    let response = ctx
        .app
        .oneshot(test_support::json_request(
            Method::GET,
            &format!("/api/v1/courses/{}/submissions/{submission_id}", course.id),
            Some(&teacher_token),
            None,
        ))
        .await
        .expect("get submission");

    let status = response.status();
    let fetched = test_support::read_json(response).await;
    assert_eq!(status, StatusCode::OK, "response: {fetched}");
    assert_eq!(fetched["status"], "approved");
    assert_eq!(fetched["final_score"], 8.5);
}
