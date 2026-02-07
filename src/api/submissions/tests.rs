use super::helpers::sanitized_filename;
use axum::http::{Method, StatusCode};
use serde_json::json;
use time::{format_description::well_known::Rfc3339, Duration, OffsetDateTime, PrimitiveDateTime};
use tower::ServiceExt;
use uuid::Uuid;

use crate::db::types::{SubmissionStatus, UserRole};
use crate::test_support;

#[test]
fn sanitized_filename_filters_disallowed_chars() {
    let input = "report (final)!.png";
    let sanitized = sanitized_filename(input);
    assert_eq!(sanitized, "reportfinal.png");
}

#[test]
fn sanitized_filename_falls_back_on_empty() {
    let input = "###";
    let sanitized = sanitized_filename(input);
    assert_eq!(sanitized, "upload");
}

fn exam_payload() -> serde_json::Value {
    let now = OffsetDateTime::now_utc().replace_nanosecond(0).expect("nanoseconds");
    let start_time = (now - Duration::hours(1)).format(&Rfc3339).unwrap();
    let end_time = (now + Duration::hours(2)).format(&Rfc3339).unwrap();

    json!({
        "title": "Autosave exam",
        "description": "Autosave flow",
        "start_time": start_time,
        "end_time": end_time,
        "duration_minutes": 60,
        "timezone": "UTC",
        "max_attempts": 1,
        "allow_breaks": false,
        "break_duration_minutes": 0,
        "auto_save_interval": 10,
        "settings": {},
        "task_types": [
            {
                "title": "Task 1",
                "description": "Auto-save task",
                "order_index": 1,
                "max_score": 10.0,
                "rubric": {"criteria": []},
                "difficulty": "easy",
                "taxonomy_tags": [],
                "formulas": [],
                "units": [],
                "validation_rules": {},
                "variants": [
                    {
                        "content": "Balance equation",
                        "parameters": {},
                        "reference_solution": null,
                        "reference_answer": null,
                        "answer_tolerance": 0.01,
                        "attachments": []
                    }
                ]
            }
        ]
    })
}

async fn create_published_exam(app: axum::Router, token: &str) -> String {
    let response = app
        .clone()
        .oneshot(test_support::json_request(
            Method::POST,
            "/api/v1/exams",
            Some(token),
            Some(exam_payload()),
        ))
        .await
        .expect("create exam");

    let status = response.status();
    let created = test_support::read_json(response).await;
    assert_eq!(status, StatusCode::CREATED, "response: {created}");
    let exam_id = created["id"].as_str().expect("exam id").to_string();

    let response = app
        .oneshot(test_support::json_request(
            Method::POST,
            &format!("/api/v1/exams/{exam_id}/publish"),
            Some(token),
            None,
        ))
        .await
        .expect("publish exam");

    let status = response.status();
    let published = test_support::read_json(response).await;
    assert_eq!(status, StatusCode::OK, "response: {published}");
    exam_id
}

async fn signup_student(
    app: axum::Router,
    isu: &str,
    full_name: &str,
    password: &str,
) -> (String, String) {
    let payload = json!({
        "isu": isu,
        "full_name": full_name,
        "password": password,
        "pd_consent": true
    });

    let response = app
        .oneshot(test_support::json_request(
            Method::POST,
            "/api/v1/auth/signup",
            None,
            Some(payload),
        ))
        .await
        .expect("signup");

    let status = response.status();
    let body = test_support::read_json(response).await;
    assert_eq!(status, StatusCode::CREATED, "response: {body}");

    let token = body["access_token"].as_str().expect("token").to_string();
    let user_id = body["user"]["id"].as_str().expect("user id").to_string();

    (token, user_id)
}

async fn login_student(app: axum::Router, isu: &str, password: &str) -> String {
    let payload = json!({
        "isu": isu,
        "password": password
    });

    let response = app
        .oneshot(test_support::json_request(
            Method::POST,
            "/api/v1/auth/login",
            None,
            Some(payload),
        ))
        .await
        .expect("login");

    let status = response.status();
    let body = test_support::read_json(response).await;
    assert_eq!(status, StatusCode::OK, "response: {body}");
    body["access_token"].as_str().expect("token").to_string()
}

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

#[tokio::test]
async fn override_score_rejects_values_above_max_score() {
    let ctx = test_support::setup_test_context().await;

    let teacher = test_support::insert_user(
        ctx.state.db(),
        "000022",
        "Teacher User",
        UserRole::Teacher,
        "teacher-pass",
    )
    .await;
    let student = test_support::insert_user(
        ctx.state.db(),
        "000023",
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
        .clone()
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
    let submission_id = submission["id"].as_str().expect("submission id");

    let response = ctx
        .app
        .oneshot(test_support::json_request(
            Method::POST,
            &format!("/api/v1/submissions/{submission_id}/override-score"),
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
async fn view_url_returns_presigned_url() {
    let ctx = test_support::setup_test_context_with_storage().await;

    let teacher = test_support::insert_user(
        ctx.state.db(),
        "000030",
        "Teacher User",
        UserRole::Teacher,
        "teacher-pass",
    )
    .await;
    let teacher_token = test_support::bearer_token(&teacher.id, ctx.state.settings());

    let (student_token, student_id) =
        signup_student(ctx.app.clone(), "000031", "Student User", "student-pass").await;

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
        .clone()
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
    let submission_id = submission["id"].as_str().expect("submission id");

    let image_id = Uuid::new_v4().to_string();
    let file_path = format!("submissions/{session_id}/image.png");
    let now_offset = OffsetDateTime::now_utc();
    let now = PrimitiveDateTime::new(now_offset.date(), now_offset.time());

    sqlx::query(
        "INSERT INTO submission_images (
            id, submission_id, filename, file_path, file_size, mime_type,
            order_index, is_processed, uploaded_at
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
    )
    .bind(&image_id)
    .bind(submission_id)
    .bind("image.png")
    .bind(&file_path)
    .bind(1024_i64)
    .bind("image/png")
    .bind(0_i32)
    .bind(false)
    .bind(now)
    .execute(ctx.state.db())
    .await
    .expect("insert image");

    let response = ctx
        .app
        .oneshot(test_support::json_request(
            Method::GET,
            &format!("/api/v1/submissions/images/{image_id}/view-url"),
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

    let owner: Option<String> =
        sqlx::query_scalar("SELECT student_id FROM submissions WHERE id = $1")
            .bind(submission_id)
            .fetch_optional(ctx.state.db())
            .await
            .expect("owner");
    assert_eq!(owner.as_deref(), Some(student_id.as_str()));
}

#[tokio::test]
async fn full_flow_signup_login_submit_and_approve() {
    let ctx = test_support::setup_test_context_with_storage().await;

    let teacher = test_support::insert_user(
        ctx.state.db(),
        "000040",
        "Teacher User",
        UserRole::Teacher,
        "teacher-pass",
    )
    .await;
    let teacher_token = test_support::bearer_token(&teacher.id, ctx.state.settings());

    let (student_token, _student_id) =
        signup_student(ctx.app.clone(), "000041", "Student User", "student-pass").await;
    let login_token = login_student(ctx.app.clone(), "000041", "student-pass").await;
    assert!(!login_token.is_empty());

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
        .clone()
        .oneshot(test_support::json_request(
            Method::POST,
            &format!("/api/v1/submissions/sessions/{session_id}/presigned-upload-url?filename=work.png&content_type=image/png"),
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
            &format!("/api/v1/submissions/sessions/{session_id}/submit"),
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
            &format!("/api/v1/submissions/{submission_id}/regrade"),
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
            &format!("/api/v1/submissions/{submission_id}/override-score"),
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
            &format!("/api/v1/submissions/{submission_id}"),
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
