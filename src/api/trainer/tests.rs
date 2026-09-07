use axum::http::{Method, StatusCode};
use serde_json::json;
use tower::ServiceExt;
use uuid::Uuid;

use crate::core::time::primitive_now_utc;
use crate::db::types::CourseRole;
use crate::repositories;
use crate::test_support;

#[tokio::test]
async fn private_trainer_set_keeps_answer_for_owner_self_check() {
    let ctx = test_support::setup_test_context().await;
    let teacher =
        test_support::insert_user(ctx.state.db(), "trainer_teacher", "Teacher", "teacher-pass")
            .await;
    let student =
        test_support::insert_user(ctx.state.db(), "trainer_student", "Student", "student-pass")
            .await;
    let course = test_support::create_course_with_teacher(
        ctx.state.db(),
        "trainer-answer-course",
        "Trainer answers",
        &teacher.id,
    )
    .await;
    test_support::add_course_role(ctx.state.db(), &course.id, &student.id, CourseRole::Student)
        .await;

    let now = primitive_now_utc();
    let source = repositories::task_bank::upsert_source(
        ctx.state.db(),
        repositories::task_bank::UpsertSource {
            id: "trainer-answer-source",
            code: "trainer-answer",
            title: "Trainer answer source",
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
    let item = repositories::task_bank::upsert_item(
        ctx.state.db(),
        repositories::task_bank::UpsertItem {
            id: "trainer-answer-item",
            source_id: &source.id,
            number: "1.1",
            paragraph: "1",
            topic: "Self-check",
            text: "Solve the training task",
            answer: Some("42 mol"),
            has_answer: true,
            metadata: json!({}),
            now,
        },
    )
    .await
    .expect("insert item");

    let set_id = Uuid::new_v4().to_string();
    let mut tx = ctx.state.db().begin().await.expect("trainer transaction");
    repositories::trainer_sets::create(
        &mut *tx,
        repositories::trainer_sets::CreateTrainerSet {
            id: &set_id,
            student_id: &student.id,
            course_id: &course.id,
            title: "Private practice",
            source_id: &source.id,
            filters: json!({"mode": "manual"}),
            now,
        },
    )
    .await
    .expect("insert trainer set");
    repositories::trainer_sets::insert_items(&mut tx, &set_id, &[item.id])
        .await
        .expect("insert trainer item");
    tx.commit().await.expect("commit trainer set");

    let token = test_support::bearer_token(&student.id, ctx.state.settings());
    let response = ctx
        .app
        .oneshot(test_support::json_request(
            Method::GET,
            &format!("/api/v1/courses/{}/trainer/sets/{set_id}", course.id),
            Some(&token),
            None,
        ))
        .await
        .expect("trainer response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = test_support::read_json(response).await;
    assert_eq!(body["items"][0]["has_answer"], true);
    assert_eq!(body["items"][0]["answer"], "42 mol");
}
