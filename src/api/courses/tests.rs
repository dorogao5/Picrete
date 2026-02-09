use axum::http::{Method, StatusCode};
use serde_json::json;
use tower::ServiceExt;

use crate::repositories;
use crate::test_support;

#[tokio::test]
async fn admin_can_delete_course() {
    let ctx = test_support::setup_test_context().await;

    let admin =
        test_support::insert_platform_admin(ctx.state.db(), "courseadmin01", "Admin", "admin-pass")
            .await;
    let token = test_support::bearer_token(&admin.id, ctx.state.settings());

    let response = ctx
        .app
        .clone()
        .oneshot(test_support::json_request(
            Method::POST,
            "/api/v1/courses",
            Some(&token),
            Some(json!({
                "slug": "delete-course-1",
                "title": "Delete Course 1",
                "organization": "Picrete"
            })),
        ))
        .await
        .expect("create course");

    let status = response.status();
    let created = test_support::read_json(response).await;
    assert_eq!(status, StatusCode::CREATED, "response: {created}");
    let course_id = created["id"].as_str().expect("course id").to_string();

    let response = ctx
        .app
        .clone()
        .oneshot(test_support::json_request(
            Method::DELETE,
            &format!("/api/v1/courses/{course_id}"),
            Some(&token),
            None,
        ))
        .await
        .expect("delete course");
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let found = repositories::courses::find_by_id(ctx.state.db(), &course_id)
        .await
        .expect("find course after deletion");
    assert!(found.is_none());
}

#[tokio::test]
async fn non_admin_cannot_delete_course() {
    let ctx = test_support::setup_test_context().await;

    let admin =
        test_support::insert_platform_admin(ctx.state.db(), "courseadmin02", "Admin", "admin-pass")
            .await;
    let course = test_support::create_course_with_teacher(
        ctx.state.db(),
        "delete-course-2",
        "Delete Course 2",
        &admin.id,
    )
    .await;

    let teacher =
        test_support::insert_user(ctx.state.db(), "teacher02", "Teacher", "teacher-pass").await;
    test_support::add_course_role(
        ctx.state.db(),
        &course.id,
        &teacher.id,
        crate::db::types::CourseRole::Teacher,
    )
    .await;
    let teacher_token = test_support::bearer_token(&teacher.id, ctx.state.settings());

    let response = ctx
        .app
        .oneshot(test_support::json_request(
            Method::DELETE,
            &format!("/api/v1/courses/{}", course.id),
            Some(&teacher_token),
            None,
        ))
        .await
        .expect("delete course as non-admin");

    let status = response.status();
    let body = test_support::read_json(response).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "response: {body}");
}
