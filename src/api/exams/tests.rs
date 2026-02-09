use axum::http::{Method, StatusCode};
use serde_json::json;
use time::{format_description::well_known::Rfc3339, Duration, OffsetDateTime};
use tower::ServiceExt;

use crate::db::types::CourseRole;
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
        test_support::insert_user(ctx.state.db(), "teacher002", "Teacher User", "teacher-pass")
            .await;
    let course = test_support::create_course_with_teacher(
        ctx.state.db(),
        "chem-101",
        "Chemistry 101",
        &teacher.id,
    )
    .await;
    let token = test_support::bearer_token(&teacher.id, ctx.state.settings());

    let response = ctx
        .app
        .clone()
        .oneshot(test_support::json_request(
            Method::POST,
            &format!("/api/v1/courses/{}/exams", course.id),
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
            &format!("/api/v1/courses/{}/exams/{exam_id}/publish", course.id),
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
            &format!("/api/v1/courses/{}/exams?status=published", course.id),
            Some(&token),
            None,
        ))
        .await
        .expect("list exams");

    let status = response.status();
    let list = test_support::read_json(response).await;
    assert_eq!(status, StatusCode::OK, "response: {list}");
    let items = list["items"].as_array().expect("exam list");
    assert!(items.iter().any(|item| item["id"] == exam_id));
}

#[tokio::test]
async fn teacher_cannot_access_other_teachers_exam_management_endpoints() {
    let ctx = test_support::setup_test_context().await;

    let owner =
        test_support::insert_user(ctx.state.db(), "teacher102", "Owner Teacher", "teacher-pass")
            .await;
    let collaborator = test_support::insert_user(
        ctx.state.db(),
        "teacher103",
        "Collaborator Teacher",
        "teacher-pass",
    )
    .await;
    let outsider =
        test_support::insert_user(ctx.state.db(), "teacher104", "Outsider Teacher", "teacher-pass")
            .await;
    let course = test_support::create_course_with_teacher(
        ctx.state.db(),
        "exam-access-101",
        "Exam Access 101",
        &owner.id,
    )
    .await;
    test_support::add_course_role(
        ctx.state.db(),
        &course.id,
        &collaborator.id,
        CourseRole::Teacher,
    )
    .await;

    let owner_token = test_support::bearer_token(&owner.id, ctx.state.settings());
    let collaborator_token = test_support::bearer_token(&collaborator.id, ctx.state.settings());
    let outsider_token = test_support::bearer_token(&outsider.id, ctx.state.settings());

    let response = ctx
        .app
        .clone()
        .oneshot(test_support::json_request(
            Method::POST,
            &format!("/api/v1/courses/{}/exams", course.id),
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
            &format!("/api/v1/courses/{}/exams/{exam_id}/task-types", course.id),
            Some(&collaborator_token),
            Some(task_type_payload),
        ))
        .await
        .expect("add task type as collaborator");
    let status = response.status();
    let body = test_support::read_json(response).await;
    assert_eq!(status, StatusCode::CREATED, "response: {body}");

    let response = ctx
        .app
        .clone()
        .oneshot(test_support::json_request(
            Method::GET,
            &format!("/api/v1/courses/{}/exams/{exam_id}/submissions", course.id),
            Some(&collaborator_token),
            None,
        ))
        .await
        .expect("list submissions as collaborator");
    let status = response.status();
    let body = test_support::read_json(response).await;
    assert_eq!(status, StatusCode::OK, "response: {body}");

    let response = ctx
        .app
        .clone()
        .oneshot(test_support::json_request(
            Method::GET,
            &format!("/api/v1/courses/{}/exams/{exam_id}/submissions", course.id),
            Some(&outsider_token),
            None,
        ))
        .await
        .expect("list submissions as outsider");
    let status = response.status();
    let body = test_support::read_json(response).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "response: {body}");
}
