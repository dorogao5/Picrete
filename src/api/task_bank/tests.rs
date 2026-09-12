use axum::http::{header, Method, StatusCode};
use tower::ServiceExt;

use crate::core::time::primitive_now_utc;
use crate::db::types::CourseRole;
use crate::repositories;
use crate::test_support;

#[tokio::test]
async fn dynamic_bank_numbers_do_not_overflow_listing_or_search() {
    let ctx = test_support::setup_test_context().await;
    let teacher =
        test_support::insert_user(ctx.state.db(), "dynamic_teacher", "Teacher", "password").await;
    let course = test_support::create_course_with_teacher(
        ctx.state.db(),
        "dynamic-bank",
        "Physical chemistry",
        &teacher.id,
    )
    .await;
    sqlx::query("INSERT INTO task_bank_sources(id,code,title,version) VALUES('dynamic-source','studio_fizicheskaya_himiya_dynamic','Generated','1')")
        .execute(ctx.state.db()).await.unwrap();
    sqlx::query(
        "INSERT INTO course_task_bank_sources(course_id,source_id) VALUES($1,'dynamic-source')",
    )
    .bind(&course.id)
    .execute(ctx.state.db())
    .await
    .unwrap();
    // Include the live format, a number beyond bigint, and ordinary chapter
    // numbering so a fix cannot silently replace natural ordering with lexical.
    let numbers = [
        "2.10",
        "2.2",
        "student-5c22316923c9-1-1",
        "student-999999999999999999999999-1-1",
        "2.999999999999999999999999",
    ];
    for number in numbers {
        sqlx::query("INSERT INTO task_bank_items(id,source_id,number,paragraph,topic,text,solution,answer,has_answer) VALUES($1,'dynamic-source',$1,'1','Arrhenius','Generated task','Reference solution','42',true)")
            .bind(number).execute(ctx.state.db()).await.unwrap();
    }
    let token = test_support::bearer_token(&teacher.id, ctx.state.settings());
    let base = format!(
        "/api/v1/courses/{}/task-bank/items?source=studio_fizicheskaya_himiya_dynamic&limit=100",
        course.id
    );
    for query in ["", "&q=student-5c22316923c9-1-1"] {
        let response = ctx
            .app
            .clone()
            .oneshot(test_support::json_request(
                Method::GET,
                &format!("{base}{query}"),
                Some(&token),
                None,
            ))
            .await
            .unwrap();
        let status = response.status();
        let body = test_support::read_json(response).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        if query.is_empty() {
            assert_eq!(body["total_count"], 5);
            assert_eq!(body["items"][0]["number"], "2.2");
            assert_eq!(body["items"][1]["number"], "2.10");
        } else {
            assert_eq!(body["total_count"], 1);
            assert_eq!(body["items"][0]["number"], "student-5c22316923c9-1-1");
            assert_eq!(body["items"][0]["solution"], "Reference solution");
        }
    }
}

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
            solution: Some("A full reference solution"),
            task_type: Some("расчетное"),
            difficulty: Some("сложная"),
            volume: Some("длинное"),
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
            assert_eq!(body["items"][0]["has_solution"], true);
            assert_eq!(
                body["items"][0]["solution"].as_str(),
                expected_answer.map(|_| "A full reference solution")
            );
        }
    }
}

#[tokio::test]
async fn bank_filters_snapshot_and_legacy_reimport_preserve_solutions() {
    use serde_json::json;
    let ctx = test_support::setup_test_context().await;
    let teacher =
        test_support::insert_user(ctx.state.db(), "filter_teacher", "Teacher", "password").await;
    let course = test_support::create_course_with_teacher(
        ctx.state.db(),
        "bank-filters",
        "Chemistry",
        &teacher.id,
    )
    .await;
    let token = test_support::bearer_token(&teacher.id, ctx.state.settings());
    let now = primitive_now_utc();
    let source = repositories::task_bank::upsert_source(
        ctx.state.db(),
        repositories::task_bank::UpsertSource {
            id: "filter-source",
            code: "filters",
            title: "Filters",
            version: "v2",
            is_active: true,
            now,
        },
    )
    .await
    .unwrap();
    sqlx::query("INSERT INTO course_task_bank_sources (course_id, source_id) VALUES ($1, $2)")
        .bind(&course.id)
        .bind(&source.id)
        .execute(ctx.state.db())
        .await
        .unwrap();
    for (number, solution) in [("7.1", Some("Detailed solution with derivation")), ("7.2", None)] {
        repositories::task_bank::upsert_item(
            ctx.state.db(),
            repositories::task_bank::UpsertItem {
                id: number,
                source_id: &source.id,
                number,
                paragraph: "7",
                topic: "Thermodynamics",
                text: "Calculate enthalpy",
                answer: None,
                has_answer: false,
                solution,
                task_type: solution.map(|_| "расчетное"),
                difficulty: solution.map(|_| "сложная"),
                volume: solution.map(|_| "длинное"),
                metadata: json!({}),
                now,
            },
        )
        .await
        .unwrap();
    }
    // Old JSON import must not erase newly enriched solution or classification.
    repositories::task_bank::upsert_item(
        ctx.state.db(),
        repositories::task_bank::UpsertItem {
            id: "7.1",
            source_id: &source.id,
            number: "7.1",
            paragraph: "7",
            topic: "Thermodynamics",
            text: "Calculate enthalpy",
            answer: None,
            has_answer: false,
            solution: None,
            task_type: None,
            difficulty: None,
            volume: None,
            metadata: json!({}),
            now,
        },
    )
    .await
    .unwrap();
    for (query, count, length) in [
        ("has_solution=true&has_answer=false&q=7.1", 1, 1),
        ("has_solution=false", 1, 1), ("has_solution=true&q=nonexistent", 0, 0),
        ("q=%25", 0, 0), ("has_solution=true&skip=999", 1, 0),
        ("source=another", 0, 0),
        ("task_type=%D1%80%D0%B0%D1%81%D1%87%D0%B5%D1%82%D0%BD%D0%BE%D0%B5&difficulty=%D1%81%D0%BB%D0%BE%D0%B6%D0%BD%D0%B0%D1%8F&volume=%D0%B4%D0%BB%D0%B8%D0%BD%D0%BD%D0%BE%D0%B5", 1, 1),
    ] {
        let response = ctx.app.clone().oneshot(test_support::json_request(Method::GET,
            &format!("/api/v1/courses/{}/task-bank/items?{query}", course.id), Some(&token), None)).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = test_support::read_json(response).await;
        assert_eq!(body["total_count"], count, "{query}: {body}");
        assert_eq!(body["items"].as_array().unwrap().len(), length, "{query}");
    }
    let response = ctx
        .app
        .clone()
        .oneshot(test_support::json_request(
            Method::GET,
            &format!("/api/v1/courses/{}/task-bank/facets?source=filters", course.id),
            Some(&token),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let facets = test_support::read_json(response).await;
    assert_eq!(facets["task_types"], json!(["расчетное"]));
    let response = ctx.app.clone().oneshot(test_support::json_request(Method::POST,
        &format!("/api/v1/courses/{}/exams", course.id), Some(&token), Some(json!({
            "title": "Bank snapshot", "kind": "homework", "start_time": "2026-09-01T00:00:00Z", "end_time": "2027-09-01T00:00:00Z", "timezone": "UTC", "max_attempts": 1, "task_types": []
        })))).await.unwrap();
    let status = response.status();
    let exam = test_support::read_json(response).await;
    assert_eq!(status, StatusCode::CREATED, "{exam}");
    let exam_id = exam["id"].as_str().unwrap();
    let response = ctx
        .app
        .clone()
        .oneshot(test_support::json_request(
            Method::POST,
            &format!("/api/v1/courses/{}/exams/{exam_id}/task-types/from-bank", course.id),
            Some(&token),
            Some(json!({"bank_item_ids": ["7.1"]})),
        ))
        .await
        .unwrap();
    let status = response.status();
    let body = test_support::read_json(response).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let response = ctx
        .app
        .clone()
        .oneshot(test_support::json_request(
            Method::GET,
            &format!("/api/v1/courses/{}/exams/{exam_id}", course.id),
            Some(&token),
            None,
        ))
        .await
        .unwrap();
    let exam = test_support::read_json(response).await;
    assert_eq!(exam["task_types"][0]["difficulty"], "hard");
    assert_eq!(
        exam["task_types"][0]["variants"][0]["reference_solution"],
        "Detailed solution with derivation"
    );
    assert!(exam["task_types"][0]["variants"][0]["reference_answer"].is_null());
    // Random generation must use exactly the same extended filters.
    let filters = crate::schemas::trainer::TrainerFilters {
        has_solution: Some(true),
        q: Some("enthalpy".into()),
        ..Default::default()
    };
    let params = repositories::task_bank::FilterParams {
        source_id: source.id,
        paragraph: None,
        topic: None,
        has_answer: None,
        filters,
    };
    assert_eq!(
        repositories::task_bank::count_items_by_filters(ctx.state.db(), &params).await.unwrap(),
        1
    );
    assert_eq!(
        repositories::task_bank::list_item_ids_by_filters(ctx.state.db(), &params, 100)
            .await
            .unwrap(),
        vec!["7.1"]
    );
}
