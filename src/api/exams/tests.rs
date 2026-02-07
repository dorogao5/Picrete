use axum::http::{Method, StatusCode};
use serde_json::json;
use time::{format_description::well_known::Rfc3339, Duration, OffsetDateTime};
use tower::ServiceExt;

use crate::db::types::UserRole;
use crate::test_support;

fn exam_payload() -> serde_json::Value {
    let now = OffsetDateTime::now_utc().replace_nanosecond(0).expect("nanoseconds");
    let start_time = (now - Duration::hours(1)).format(&Rfc3339).unwrap();
    let end_time = (now + Duration::hours(2)).format(&Rfc3339).unwrap();

    json!({
        "title": "Chemistry midterm",
        "description": "Unit test exam",
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
                "description": "Solve the equation",
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
                        "content": "H2 + O2 -> ?",
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

#[tokio::test]
async fn teacher_can_create_publish_and_list_exam() {
    let ctx = test_support::setup_test_context().await;

    let teacher =
        test_support::insert_user(ctx.state.db(), "000002", "Teacher User", UserRole::Teacher, "teacher-pass")
            .await;
    let token = test_support::bearer_token(&teacher.id, ctx.state.settings());

    let response = ctx
        .app
        .clone()
        .oneshot(test_support::json_request(
            Method::POST,
            "/api/v1/exams",
            Some(&token),
            Some(exam_payload()),
        ))
        .await
        .expect("create exam");

    let status = response.status();
    let created = test_support::read_json(response).await;
    assert_eq!(status, StatusCode::CREATED, "response: {created}");
    let exam_id = created["id"].as_str().expect("exam id").to_string();
    assert_eq!(created["status"], "draft");
    assert_eq!(created["task_types"].as_array().unwrap().len(), 1);

    let response = ctx
        .app
        .clone()
        .oneshot(test_support::json_request(
            Method::POST,
            &format!("/api/v1/exams/{exam_id}/publish"),
            Some(&token),
            None,
        ))
        .await
        .expect("publish exam");

    let status = response.status();
    let published = test_support::read_json(response).await;
    assert_eq!(status, StatusCode::OK, "response: {published}");
    assert_eq!(published["status"], "published");

    let response = ctx
        .app
        .oneshot(test_support::json_request(
            Method::GET,
            "/api/v1/exams?status=published",
            Some(&token),
            None,
        ))
        .await
        .expect("list exams");

    let status = response.status();
    let list = test_support::read_json(response).await;
    assert_eq!(status, StatusCode::OK, "response: {list}");
    let items = list.as_array().expect("exam list");
    assert!(items.iter().any(|item| item["id"] == exam_id));
}

#[tokio::test]
async fn teacher_cannot_access_other_teachers_exam_management_endpoints() {
    let ctx = test_support::setup_test_context().await;

    let owner =
        test_support::insert_user(ctx.state.db(), "000102", "Owner Teacher", UserRole::Teacher, "teacher-pass")
            .await;
    let intruder = test_support::insert_user(
        ctx.state.db(),
        "000103",
        "Intruder Teacher",
        UserRole::Teacher,
        "teacher-pass",
    )
    .await;

    let owner_token = test_support::bearer_token(&owner.id, ctx.state.settings());
    let intruder_token = test_support::bearer_token(&intruder.id, ctx.state.settings());

    let response = ctx
        .app
        .clone()
        .oneshot(test_support::json_request(
            Method::POST,
            "/api/v1/exams",
            Some(&owner_token),
            Some(exam_payload()),
        ))
        .await
        .expect("create exam");
    let status = response.status();
    let created = test_support::read_json(response).await;
    assert_eq!(status, StatusCode::CREATED, "response: {created}");
    let exam_id = created["id"].as_str().expect("exam id");

    let task_type_payload = json!({
        "title": "Task 2",
        "description": "Unauthorized add",
        "order_index": 2,
        "max_score": 5.0,
        "rubric": {"criteria": []},
        "difficulty": "easy",
        "taxonomy_tags": [],
        "formulas": [],
        "units": [],
        "validation_rules": {},
        "variants": [
            {
                "content": "Unauthorized variant",
                "parameters": {},
                "reference_solution": null,
                "reference_answer": null,
                "answer_tolerance": 0.01,
                "attachments": []
            }
        ]
    });

    let response = ctx
        .app
        .clone()
        .oneshot(test_support::json_request(
            Method::POST,
            &format!("/api/v1/exams/{exam_id}/task-types"),
            Some(&intruder_token),
            Some(task_type_payload),
        ))
        .await
        .expect("add task type as intruder");
    let status = response.status();
    let body = test_support::read_json(response).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "response: {body}");

    let response = ctx
        .app
        .oneshot(test_support::json_request(
            Method::GET,
            &format!("/api/v1/exams/{exam_id}/submissions"),
            Some(&intruder_token),
            None,
        ))
        .await
        .expect("list submissions as intruder");
    let status = response.status();
    let body = test_support::read_json(response).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "response: {body}");
}
