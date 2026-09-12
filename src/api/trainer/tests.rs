use axum::http::{Method, StatusCode};
use serde_json::json;
use tower::ServiceExt;
use uuid::Uuid;

use crate::core::time::primitive_now_utc;
use crate::db::types::CourseRole;
use crate::repositories;
use crate::test_support;

#[test]
fn generated_set_accepts_only_nonempty_bounded_verified_subset() {
    for actual in [1, 3, 5] {
        assert!(super::handlers::validate_generated_count(actual, 5).is_ok());
    }
    for (actual, requested) in [(0, 5), (6, 5), (1, 0), (1, -1)] {
        assert!(super::handlers::validate_generated_count(actual, requested).is_err());
    }
}

#[tokio::test]
async fn physical_generation_without_studio_config_never_falls_back_to_bank() {
    let mut ctx = test_support::setup_test_context().await;
    let admin = test_support::insert_platform_admin(
        ctx.state.db(),
        "studio_config_admin",
        "Config test",
        "test-password",
    )
    .await;
    let course = test_support::create_course_with_teacher(
        ctx.state.db(),
        "studio-config-course",
        "Physical chemistry",
        &admin.id,
    )
    .await;
    // Admin bypasses the unlock gate, isolating the missing-config failure.
    // A matching ready bank item makes the old fallback return 201, not 422.
    sqlx::query("INSERT INTO task_bank_sources(id,code,title,version) VALUES('config-bank','studio_fizicheskaya_himiya','Physical','1')")
        .execute(ctx.state.db()).await.unwrap();
    sqlx::query(
        "INSERT INTO course_task_bank_sources(course_id,source_id) VALUES($1,'config-bank')",
    )
    .bind(&course.id)
    .execute(ctx.state.db())
    .await
    .unwrap();
    sqlx::query("INSERT INTO task_bank_items(id,source_id,number,paragraph,topic,text,answer,has_answer,solution,difficulty) VALUES('config-task','config-bank','1','1','Кинетика','Ready task','42',true,'Reference solution','easy')")
        .execute(ctx.state.db()).await.unwrap();
    sqlx::query("INSERT INTO course_ai_assistants(course_id,studio_assistant_id,name,discipline,snapshot_version,snapshot,enabled) VALUES($1,'config-assistant','Tutor','Physical chemistry','1','{}',true)")
        .bind(&course.id).execute(ctx.state.db()).await.unwrap();

    let prior_url = std::env::var("STUDIO_API_URL").ok();
    let prior_token = std::env::var("STUDIO_INTEGRATION_TOKEN").ok();
    for (url, token) in [("", "test-token"), ("   ", "test-token"), ("http://127.0.0.1:9", "")] {
        // Empty env values override any dotenv file; no real Studio is contacted.
        std::env::set_var("STUDIO_API_URL", url);
        std::env::set_var("STUDIO_INTEGRATION_TOKEN", token);
        let settings = crate::core::config::Settings::load();
        for (key, prior) in
            [("STUDIO_API_URL", &prior_url), ("STUDIO_INTEGRATION_TOKEN", &prior_token)]
        {
            match prior {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
        ctx.state = crate::core::state::AppState::new(
            settings.unwrap(),
            ctx.state.db().clone(),
            ctx.state.redis().clone(),
            None,
        );
        ctx.app = crate::api::router::router(ctx.state.clone());
        let bearer = test_support::bearer_token(&admin.id, ctx.state.settings());
        let response = ctx.app.clone().oneshot(test_support::json_request(
            Method::POST,
            &format!("/api/v1/courses/{}/trainer/sets/generate", course.id),
            Some(&bearer),
            Some(json!({"source":"studio_fizicheskaya_himiya","count":1,"filters":{"topic":"Кинетика","difficulty":"easy","has_solution":true,"has_answer":true}})),
        )).await.unwrap();
        let status = response.status();
        let body = test_support::read_json(response).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "missing Studio config: {body}");
        assert!(body["detail"].as_str().unwrap_or("").contains("Studio не настроена"), "{body}");
        let sets: i64 = sqlx::query_scalar("SELECT count(*) FROM trainer_sets WHERE course_id=$1")
            .bind(&course.id)
            .fetch_one(ctx.state.db())
            .await
            .unwrap();
        assert_eq!(sets, 0, "Missing Studio config must not create a bank-backed set");
    }
}

#[tokio::test]
async fn private_trainer_set_requires_explicit_practice_reveal() {
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
            solution: None,
            task_type: None,
            difficulty: None,
            volume: None,
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
    assert!(body["items"][0]["answer"].is_null());
}
