use axum::http::{Method, StatusCode};
use serde_json::json;
use time::{format_description::well_known::Rfc3339, Duration, OffsetDateTime, PrimitiveDateTime};
use tower::ServiceExt;
use uuid::Uuid;

use crate::db::types::SubmissionStatus;
use crate::repositories;
use crate::test_support;

mod filenames;
mod full_flow;
mod student_flows;
mod teacher_flows;

/// Inserts a submission and one image for the session so submit passes the "at least one image" check.
/// Returns (submission_id, image_id).
pub(super) async fn insert_submission_with_one_image(
    pool: &sqlx::PgPool,
    course_id: &str,
    session_id: &str,
    student_id: &str,
    exam_id: &str,
) -> (String, String) {
    let max_score =
        repositories::exams::max_score_for_exam(pool, course_id, exam_id).await.expect("max_score");
    let now =
        PrimitiveDateTime::new(OffsetDateTime::now_utc().date(), OffsetDateTime::now_utc().time());
    let submission_id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO submissions (
            id, course_id, session_id, student_id, submitted_at, status, max_score, created_at, updated_at
         ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
    )
    .bind(&submission_id)
    .bind(course_id)
    .bind(session_id)
    .bind(student_id)
    .bind(now)
    .bind(SubmissionStatus::Uploaded)
    .bind(max_score)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .expect("insert submission");

    let image_id = Uuid::new_v4().to_string();
    let file_path = format!("submissions/{session_id}/{image_id}_image.png");
    sqlx::query(
        "INSERT INTO submission_images (
            id, course_id, submission_id, filename, file_path, file_size, mime_type, order_index, is_processed, uploaded_at
         ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
    )
    .bind(&image_id)
    .bind(course_id)
    .bind(&submission_id)
    .bind("image.png")
    .bind(&file_path)
    .bind(1024_i64)
    .bind("image/png")
    .bind(0_i32)
    .bind(false)
    .bind(now)
    .execute(pool)
    .await
    .expect("insert image");

    (submission_id, image_id)
}

pub(super) fn exam_payload() -> serde_json::Value {
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

pub(super) async fn create_published_exam(
    app: axum::Router,
    token: &str,
    course_id: &str,
) -> String {
    let response = app
        .clone()
        .oneshot(test_support::json_request(
            Method::POST,
            &format!("/api/v1/courses/{course_id}/exams"),
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
            &format!("/api/v1/courses/{course_id}/exams/{exam_id}/publish"),
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

pub(super) async fn signup_student(
    app: axum::Router,
    username: &str,
    full_name: &str,
    password: &str,
    invite_code: Option<&str>,
) -> (String, String) {
    let mut payload = json!({
        "username": username,
        "full_name": full_name,
        "password": password,
        "pd_consent": true
    });
    if let Some(invite_code) = invite_code {
        payload["invite_code"] = json!(invite_code);
        payload["identity_payload"] = json!({});
    }

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

pub(super) async fn login_student(app: axum::Router, username: &str, password: &str) -> String {
    let payload = json!({
        "username": username,
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
