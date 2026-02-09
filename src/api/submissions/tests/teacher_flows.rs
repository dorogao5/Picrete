use super::{create_published_exam, insert_submission_with_one_image, signup_student};
use crate::db::types::{CourseRole, SubmissionStatus};
use crate::test_support;
use axum::http::{Method, StatusCode};
use serde_json::json;
use tower::ServiceExt;

#[tokio::test]
async fn override_score_rejects_values_above_max_score() {
    let ctx = test_support::setup_test_context().await;

    let teacher =
        test_support::insert_user(ctx.state.db(), "teacher022", "Teacher User", "teacher-pass")
            .await;
    let student =
        test_support::insert_user(ctx.state.db(), "student023", "Student User", "student-pass")
            .await;
    let course = test_support::create_course_with_teacher(
        ctx.state.db(),
        "override-101",
        "Override 101",
        &teacher.id,
    )
    .await;
    test_support::add_course_role(ctx.state.db(), &course.id, &student.id, CourseRole::Student)
        .await;

    let teacher_token = test_support::bearer_token(&teacher.id, ctx.state.settings());
    let student_token = test_support::bearer_token(&student.id, ctx.state.settings());
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
        .oneshot(test_support::json_request(
            Method::POST,
            &format!("/api/v1/courses/{}/submissions/{submission_id}/override-score", course.id),
            Some(&teacher_token),
            Some(json!({
                "final_score": 1000.0,
                "teacher_comments": "manual override"
            })),
        ))
        .await
        .expect("override score");

    let status = response.status();
    let body = test_support::read_json(response).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "response: {body}");
    assert!(body["detail"].as_str().unwrap_or("").contains("cannot exceed max_score"));
}

#[tokio::test]
async fn teacher_cannot_read_grading_status_for_other_teachers_submission() {
    let ctx = test_support::setup_test_context().await;

    let owner_teacher =
        test_support::insert_user(ctx.state.db(), "teacher071", "Owner Teacher", "teacher-pass")
            .await;
    let intruder_teacher =
        test_support::insert_user(ctx.state.db(), "teacher072", "Intruder Teacher", "teacher-pass")
            .await;
    let student =
        test_support::insert_user(ctx.state.db(), "student073", "Student User", "student-pass")
            .await;
    let course = test_support::create_course_with_teacher(
        ctx.state.db(),
        "grading-101",
        "Grading 101",
        &owner_teacher.id,
    )
    .await;
    test_support::add_course_role(ctx.state.db(), &course.id, &student.id, CourseRole::Student)
        .await;

    let owner_token = test_support::bearer_token(&owner_teacher.id, ctx.state.settings());
    let intruder_token = test_support::bearer_token(&intruder_teacher.id, ctx.state.settings());
    let student_token = test_support::bearer_token(&student.id, ctx.state.settings());

    let exam_id = create_published_exam(ctx.app.clone(), &owner_token, &course.id).await;

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
    let session_id = session["id"].as_str().expect("session id").to_string();

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
    let submission_id = submission["id"].as_str().expect("submission id").to_string();

    let response = ctx
        .app
        .oneshot(test_support::json_request(
            Method::GET,
            &format!("/api/v1/courses/{}/submissions/grading-status/{submission_id}", course.id),
            Some(&intruder_token),
            None,
        ))
        .await
        .expect("grading status");
    let status = response.status();
    let body = test_support::read_json(response).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "response: {body}");
}

#[tokio::test]
async fn view_url_returns_presigned_url() {
    let ctx = test_support::setup_test_context_with_storage().await;

    let teacher =
        test_support::insert_user(ctx.state.db(), "teacher030", "Teacher User", "teacher-pass")
            .await;
    let course = test_support::create_course_with_teacher(
        ctx.state.db(),
        "view-url-101",
        "View URL 101",
        &teacher.id,
    )
    .await;
    let student_invite =
        test_support::create_active_invite_code(ctx.state.db(), &course, CourseRole::Student).await;
    let teacher_token = test_support::bearer_token(&teacher.id, ctx.state.settings());

    let (student_token, _student_id) = signup_student(
        ctx.app.clone(),
        "student031",
        "Student User",
        "student-pass",
        Some(&student_invite),
    )
    .await;

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
    let student_id = session["student_id"].as_str().expect("student_id in session");

    let (_submission_id, image_id) = insert_submission_with_one_image(
        ctx.state.db(),
        &course.id,
        session_id,
        student_id,
        &exam_id,
    )
    .await;

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
        .oneshot(test_support::json_request(
            Method::GET,
            &format!("/api/v1/courses/{}/submissions/images/{image_id}/view-url", course.id),
            Some(&student_token),
            None,
        ))
        .await
        .expect("view url");

    let status = response.status();
    let body = test_support::read_json(response).await;
    assert_eq!(status, StatusCode::OK, "response: {body}");
    assert!(body["view_url"].as_str().unwrap_or("").contains("image.png"));
    assert_eq!(body["mime_type"], "image/png");
    assert_eq!(body["filename"], "image.png");

    let owner: Option<String> = sqlx::query_scalar(
        "SELECT student_id
         FROM submissions
         WHERE course_id = $1 AND id = $2 AND status = $3",
    )
    .bind(&course.id)
    .bind(submission_id)
    .bind(SubmissionStatus::Uploaded)
    .fetch_optional(ctx.state.db())
    .await
    .expect("owner");
    assert_eq!(owner.as_deref(), Some(student_id));
}
