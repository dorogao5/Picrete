use super::{create_published_exam, exam_payload};
use crate::db::types::{CourseRole, SubmissionStatus};
use crate::test_support;
use axum::http::{Method, StatusCode};
use serde_json::json;
use tower::ServiceExt;

#[tokio::test]
async fn student_auto_save_is_rate_limited() {
    let ctx = test_support::setup_test_context().await;

    let teacher =
        test_support::insert_user(ctx.state.db(), "teacher010", "Teacher User", "teacher-pass")
            .await;
    let student =
        test_support::insert_user(ctx.state.db(), "student011", "Student User", "student-pass")
            .await;
    let course = test_support::create_course_with_teacher(
        ctx.state.db(),
        "auto-save-101",
        "Auto Save 101",
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

    let payload = json!({ "draft": { "q1": "answer" } });
    let response = ctx
        .app
        .clone()
        .oneshot(test_support::json_request(
            Method::POST,
            &format!("/api/v1/courses/{}/submissions/sessions/{session_id}/auto-save", course.id),
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
            &format!("/api/v1/courses/{}/submissions/sessions/{session_id}/auto-save", course.id),
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

    let teacher =
        test_support::insert_user(ctx.state.db(), "teacher020", "Teacher User", "teacher-pass")
            .await;
    let student =
        test_support::insert_user(ctx.state.db(), "student021", "Student User", "student-pass")
            .await;
    let course = test_support::create_course_with_teacher(
        ctx.state.db(),
        "submit-101",
        "Submit 101",
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
    assert_eq!(submission["session_id"], session_id);
    assert_eq!(submission["status"], "uploaded");
}

#[tokio::test]
async fn uploaded_images_are_persistent_and_deletable() {
    let ctx = test_support::setup_test_context().await;

    let teacher =
        test_support::insert_user(ctx.state.db(), "teacher041", "Teacher User", "teacher-pass")
            .await;
    let student =
        test_support::insert_user(ctx.state.db(), "student042", "Student User", "student-pass")
            .await;
    let course = test_support::create_course_with_teacher(
        ctx.state.db(),
        "images-101",
        "Images 101",
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
    let session = test_support::read_json(response).await;
    let session_id = session["id"].as_str().expect("session id");

    let (submission_id, _) = super::insert_submission_with_one_image(
        ctx.state.db(),
        &course.id,
        session_id,
        &student.id,
        &exam_id,
    )
    .await;
    let image_id_2 = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO submission_images (
            id, course_id, submission_id, filename, file_path, file_size, mime_type, order_index, is_processed, uploaded_at
         ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
    )
    .bind(&image_id_2)
    .bind(&course.id)
    .bind(&submission_id)
    .bind("page2.png")
    .bind(format!("submissions/{session_id}/{image_id_2}_page2.png"))
    .bind(2048_i64)
    .bind("image/png")
    .bind(1_i32)
    .bind(false)
    .bind(crate::core::time::primitive_now_utc())
    .execute(ctx.state.db())
    .await
    .expect("insert second image");

    let list_uri =
        format!("/api/v1/courses/{}/submissions/sessions/{session_id}/images", course.id);
    let listed = ctx
        .app
        .clone()
        .oneshot(test_support::json_request(Method::GET, &list_uri, Some(&student_token), None))
        .await
        .expect("list session images");
    assert_eq!(listed.status(), StatusCode::OK);
    let payload = test_support::read_json(listed).await;
    let items = payload["items"].as_array().expect("items array");
    assert_eq!(items.len(), 2, "response: {payload}");
    assert_eq!(items[0]["order_index"], 0);
    assert_eq!(items[1]["order_index"], 1);
    assert_eq!(items[0]["upload_source"], "web");

    let image_id = items[0]["id"].as_str().expect("image id");
    let delete_uri = format!(
        "/api/v1/courses/{}/submissions/sessions/{session_id}/images/{image_id}",
        course.id
    );
    let deleted = ctx
        .app
        .clone()
        .oneshot(test_support::json_request(
            Method::DELETE,
            &delete_uri,
            Some(&student_token),
            None,
        ))
        .await
        .expect("delete image");
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);

    let listed_after = ctx
        .app
        .oneshot(test_support::json_request(Method::GET, &list_uri, Some(&student_token), None))
        .await
        .expect("list images after delete");
    assert_eq!(listed_after.status(), StatusCode::OK);
    let after_payload = test_support::read_json(listed_after).await;
    let after_items = after_payload["items"].as_array().expect("items array");
    assert_eq!(after_items.len(), 1, "response: {after_payload}");
}

#[tokio::test]
async fn submit_exam_does_not_downgrade_processing_submission() {
    let ctx = test_support::setup_test_context().await;

    let teacher =
        test_support::insert_user(ctx.state.db(), "teacher024", "Teacher User", "teacher-pass")
            .await;
    let student =
        test_support::insert_user(ctx.state.db(), "student025", "Student User", "student-pass")
            .await;
    let course = test_support::create_course_with_teacher(
        ctx.state.db(),
        "processing-101",
        "Processing 101",
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
        .expect("first submit");
    let status = response.status();
    let first_submission = test_support::read_json(response).await;
    assert_eq!(status, StatusCode::OK, "response: {first_submission}");

    sqlx::query("UPDATE submissions SET status = $1 WHERE course_id = $2 AND session_id = $3")
        .bind(SubmissionStatus::Processing)
        .bind(&course.id)
        .bind(&session_id)
        .execute(ctx.state.db())
        .await
        .expect("mark processing");

    let response = ctx
        .app
        .oneshot(test_support::json_request(
            Method::POST,
            &format!("/api/v1/courses/{}/submissions/sessions/{session_id}/submit", course.id),
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
async fn homework_variant_response_has_no_timer() {
    let ctx = test_support::setup_test_context().await;

    let teacher =
        test_support::insert_user(ctx.state.db(), "teacher026", "Teacher User", "teacher-pass")
            .await;
    let student =
        test_support::insert_user(ctx.state.db(), "student027", "Student User", "student-pass")
            .await;
    let course = test_support::create_course_with_teacher(
        ctx.state.db(),
        "homework-variant-101",
        "Homework Variant 101",
        &teacher.id,
    )
    .await;
    test_support::add_course_role(ctx.state.db(), &course.id, &student.id, CourseRole::Student)
        .await;

    let teacher_token = test_support::bearer_token(&teacher.id, ctx.state.settings());
    let student_token = test_support::bearer_token(&student.id, ctx.state.settings());

    let mut payload = exam_payload();
    payload["kind"] = json!("homework");
    payload["duration_minutes"] = serde_json::Value::Null;

    let response = ctx
        .app
        .clone()
        .oneshot(test_support::json_request(
            Method::POST,
            &format!("/api/v1/courses/{}/exams", course.id),
            Some(&teacher_token),
            Some(payload),
        ))
        .await
        .expect("create homework");
    let status = response.status();
    let created = test_support::read_json(response).await;
    assert_eq!(status, StatusCode::CREATED, "response: {created}");
    let exam_id = created["id"].as_str().expect("exam id").to_string();

    let response = ctx
        .app
        .clone()
        .oneshot(test_support::json_request(
            Method::POST,
            &format!("/api/v1/courses/{}/exams/{exam_id}/publish", course.id),
            Some(&teacher_token),
            None,
        ))
        .await
        .expect("publish homework");
    let status = response.status();
    let published = test_support::read_json(response).await;
    assert_eq!(status, StatusCode::OK, "response: {published}");

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
        .expect("enter homework");
    let status = response.status();
    let session = test_support::read_json(response).await;
    assert_eq!(status, StatusCode::OK, "response: {session}");
    let session_id = session["id"].as_str().expect("session id");

    let response = ctx
        .app
        .oneshot(test_support::json_request(
            Method::GET,
            &format!("/api/v1/courses/{}/submissions/sessions/{session_id}/variant", course.id),
            Some(&student_token),
            None,
        ))
        .await
        .expect("get homework variant");
    let status = response.status();
    let variant = test_support::read_json(response).await;
    assert_eq!(status, StatusCode::OK, "response: {variant}");
    assert_eq!(variant["work_kind"], "homework");
    assert!(variant["time_remaining"].is_null(), "response: {variant}");
    assert!(variant["hard_deadline"].is_string(), "response: {variant}");
}
