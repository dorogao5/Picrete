use axum::http::{header, Method, StatusCode};
use tower::ServiceExt;

use crate::core::time::primitive_now_utc;
use crate::db::types::CourseRole;
use crate::repositories;
use crate::test_support;

#[tokio::test]
async fn task_bank_answer_authorization_matrix_is_fail_closed() {
    let ctx = test_support::setup_test_context().await;
    let teacher =
        test_support::insert_user(ctx.state.db(), "bank_teacher", "Teacher", "teacher-pass").await;
    let student =
        test_support::insert_user(ctx.state.db(), "bank_student", "Student", "student-pass").await;
    let outsider =
        test_support::insert_user(ctx.state.db(), "bank_outsider", "Outsider", "outsider-pass")
            .await;
    let admin = test_support::insert_platform_admin(
        ctx.state.db(),
        "bank_admin",
        "Administrator",
        "admin-pass",
    )
    .await;
    let course = test_support::create_course_with_teacher(
        ctx.state.db(),
        "bank-auth-course",
        "General Chemistry",
        &teacher.id,
    )
    .await;
    test_support::add_course_role(ctx.state.db(), &course.id, &student.id, CourseRole::Student)
        .await;

    let now = primitive_now_utc();
    let source = repositories::task_bank::upsert_source(
        ctx.state.db(),
        repositories::task_bank::UpsertSource {
            id: "source-auth-matrix",
            code: "auth-matrix",
            title: "Authorization matrix",
            version: "v1",
            is_active: true,
            now,
        },
    )
    .await
    .expect("insert source");
    sqlx::query("INSERT INTO course_task_bank_sources (course_id, source_id) VALUES ($1, $2)")
        .bind(&course.id)
        .bind(&source.id)
        .execute(ctx.state.db())
        .await
        .expect("map source to course");
    repositories::task_bank::upsert_item(
        ctx.state.db(),
        repositories::task_bank::UpsertItem {
            id: "item-auth-matrix",
            source_id: &source.id,
            number: "1.1",
            paragraph: "1",
            topic: "Security",
            text: "What is the protected answer?",
            answer: Some("server-side-key"),
            has_answer: true,
            metadata: serde_json::json!({}),
            now,
        },
    )
    .await
    .expect("insert item");

    let uri = format!("/api/v1/courses/{}/task-bank/items", course.id);
    let cases = [
        (None, StatusCode::UNAUTHORIZED, None),
        (
            Some(test_support::bearer_token(&outsider.id, ctx.state.settings())),
            StatusCode::FORBIDDEN,
            None,
        ),
        (Some(test_support::bearer_token(&student.id, ctx.state.settings())), StatusCode::OK, None),
        (
            Some(test_support::bearer_token(&teacher.id, ctx.state.settings())),
            StatusCode::OK,
            Some("server-side-key"),
        ),
        (
            Some(test_support::bearer_token(&admin.id, ctx.state.settings())),
            StatusCode::OK,
            Some("server-side-key"),
        ),
    ];

    for (token, expected_status, expected_answer) in cases {
        let response = ctx
            .app
            .clone()
            .oneshot(test_support::json_request(Method::GET, &uri, token.as_deref(), None))
            .await
            .expect("task bank response");
        assert_eq!(response.status(), expected_status);
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL).and_then(|value| value.to_str().ok()),
            Some("private, no-store, max-age=0, must-revalidate")
        );
        if expected_status == StatusCode::OK {
            let body = test_support::read_json(response).await;
            let actual = body["items"][0]["answer"].as_str();
            assert_eq!(actual, expected_answer, "response: {body}");
        }
    }
}
