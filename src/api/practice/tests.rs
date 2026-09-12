use crate::{
    db::types::CourseRole,
    test_support::{self, TestContext},
};
use axum::{
    http::{Method, StatusCode},
    Json,
};
use serde_json::{json, Value};
use tower::ServiceExt;
use uuid::Uuid;

async fn request(
    ctx: &TestContext,
    token: &str,
    method: Method,
    path: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let r = ctx
        .app
        .clone()
        .oneshot(test_support::json_request(method, path, Some(token), body))
        .await
        .unwrap();
    let status = r.status();
    (status, test_support::read_json(r).await)
}
async fn fixture(ctx: &TestContext) -> (String, String, String, String, Value) {
    let teacher = test_support::insert_user(ctx.state.db(), "author", "Author", "password").await;
    let student = test_support::insert_user(ctx.state.db(), "learner", "Learner", "password").await;
    let course = test_support::create_course_with_teacher(
        ctx.state.db(),
        "practice-course",
        "Practice",
        &teacher.id,
    )
    .await;
    test_support::add_course_role(ctx.state.db(), &course.id, &student.id, CourseRole::Student)
        .await;
    sqlx::query(
        "INSERT INTO task_bank_sources(id,code,title,version) VALUES('bank','bank','Bank','1')",
    )
    .execute(ctx.state.db())
    .await
    .unwrap();
    sqlx::query("INSERT INTO course_task_bank_sources(course_id,source_id) VALUES($1,'bank')")
        .bind(&course.id)
        .execute(ctx.state.db())
        .await
        .unwrap();
    for (id, level) in [("task-easy", "easy"), ("task-hard", "hard")] {
        sqlx::query("INSERT INTO task_bank_items(id,source_id,number,paragraph,topic,text,answer,has_answer,solution,difficulty) VALUES($1,'bank',$1,'1','Растворы','Найдите массовую долю','10%',true,'Эталон: масса вещества / масса раствора',$2)").bind(id).bind(level).execute(ctx.state.db()).await.unwrap();
    }
    let model = &ctx.state.settings().ai().assistant_model;
    let snapshot = json!({"version":"test-version","assistant":{"grading_enabled":true,"name":"Tutor","criteria":[{"name":"Метод","max_score":5}],"runtime_policy":{"policy_version":"test","tier":"decision","decision_model_id":model,"tutor_model_id":model,"allowed_uses":["grading","student_tutor"]}},"prompts":{"tutor":{"system_prompt":"Обучай химии"},"grader":{"system_prompt":"Проверяй решение"}},"reference_sheets":[]});
    sqlx::query("INSERT INTO course_ai_assistants(course_id,studio_assistant_id,name,discipline,snapshot_version,snapshot) VALUES($1,'test','Tutor','Chemistry','test-version',$2)").bind(&course.id).bind(snapshot).execute(ctx.state.db()).await.unwrap();
    let def = json!({"title":"Растворы","description":"Практика по курсу","sections":[{"id":Uuid::new_v4().to_string(),"title":"Концентрации","target":2,"items":[{"task_id":"task-easy","difficulty":"easy"},{"task_id":"task-hard","difficulty":"hard"}]}]});
    (
        course.id,
        test_support::bearer_token(&teacher.id, ctx.state.settings()),
        test_support::bearer_token(&student.id, ctx.state.settings()),
        student.id,
        def,
    )
}

#[tokio::test]
async fn physical_generation_unlock_is_distinct_scoped_and_survives_republish() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    let mut ctx = test_support::setup_test_context().await;
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let mock = axum::Router::new().route(
        "/api/internal/trainer/generate",
        axum::routing::post(move |Json(body): Json<Value>| async move {
            let call = observed.fetch_add(1, Ordering::SeqCst);
            assert_eq!(body["topic"], "Концентрации");
            assert_eq!(body["difficulty"], "hard");
            assert_eq!(body["count"], 5);
            if call < 2 {
                return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({})));
            }
            (StatusCode::OK, Json(json!({
                "source":{"code":"studio_fizicheskaya_himiya","title":"Physical","version":"test"},
                "paragraphs":[{"paragraph":"generated","topic":"Концентрации","tasks":(0..if call == 2 { 3 } else { 5 }).map(|i|json!({
                    "number":format!("generated-{call}-{i}"),"text":"Generated concentration task",
                    "solution":"Generated reference: 10 / 100 = 0.1","answer":"10%","difficulty":"hard"
                })).collect::<Vec<_>>()}]
            })))
        }),
    ).route("/chat/completions", axum::routing::post(|Json(body): Json<Value>| async move {
        let prompt = body.to_string();
        assert!(prompt.contains("Generated concentration task"), "{prompt}");
        assert!(prompt.contains("Generated reference: 10 / 100 = 0.1"), "{prompt}");
        let content = json!({"unreadable":false,"total_score":5,"max_score":5,"criteria_scores":[{"criterion_name":"Метод","score":5,"max_score":5,"comment":"Верно"}],"feedback":"Верно"}).to_string();
        Json(json!({"choices":[{"message":{"content":content}}]}))
    }));
    let server = tokio::spawn(async move { axum::serve(listener, mock).await.unwrap() });
    let prior_url = std::env::var("STUDIO_API_URL").ok();
    let prior_token = std::env::var("STUDIO_INTEGRATION_TOKEN").ok();
    let prior_ai = std::env::var("ASSISTANT_AI_BASE_URL").ok();
    std::env::set_var("STUDIO_API_URL", &url);
    std::env::set_var("STUDIO_INTEGRATION_TOKEN", "local-test");
    std::env::set_var("ASSISTANT_AI_BASE_URL", &url);
    let settings = crate::core::config::Settings::load().unwrap();
    for (key, prior) in [
        ("STUDIO_API_URL", prior_url),
        ("STUDIO_INTEGRATION_TOKEN", prior_token),
        ("ASSISTANT_AI_BASE_URL", prior_ai),
    ] {
        match prior {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }
    ctx.state = crate::core::state::AppState::new(
        settings,
        ctx.state.db().clone(),
        ctx.state.redis().clone(),
        None,
    );
    ctx.app = crate::api::router::router(ctx.state.clone());
    let (course, teacher, student, student_id, mut definition) = fixture(&ctx).await;
    let source = crate::repositories::trainer_sets::PHYSICAL_CHEMISTRY_SOURCE;
    sqlx::query("UPDATE task_bank_sources SET code=$1 WHERE id='bank'")
        .bind(source)
        .execute(ctx.state.db())
        .await
        .unwrap();
    sqlx::query("INSERT INTO task_bank_items(id,source_id,number,paragraph,topic,text,solution) VALUES('task-third','bank','3','1','Растворы','Задача','Эталон')").execute(ctx.state.db()).await.unwrap();
    definition["sections"][0]["items"]
        .as_array_mut()
        .unwrap()
        .push(json!({"task_id":"task-third","difficulty":"medium"}));
    let section = definition["sections"][0]["id"].as_str().unwrap().to_owned();
    let other_section = Uuid::new_v4().to_string();
    let mut other = definition["sections"][0].clone();
    other["id"] = json!(other_section);
    other["title"] = json!("Другая подтема");
    definition["sections"].as_array_mut().unwrap().push(other);
    let base = format!("/api/v1/courses/{course}/practice/catalog");
    let (status, created) =
        request(&ctx, &teacher, Method::POST, &base, Some(json!({"definition":definition}))).await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let id = created["id"].as_str().unwrap();
    let path = format!("{base}/{id}");
    let (status, published) = request(
        &ctx,
        &teacher,
        Method::POST,
        &format!("{path}/publish"),
        Some(json!({"definition":definition,"revision":1})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{published}");
    let endpoint = format!("/api/v1/courses/{course}/trainer/sets/generate");
    let payload = json!({"source":source,"count":5,"trainer_id":id,"section_id":section,"filters":{"topic":"Концентрации","difficulty":"hard"}});
    assert_eq!(
        request(&ctx, &student, Method::POST, &endpoint, Some(payload.clone())).await.0,
        StatusCode::FORBIDDEN
    );
    for field in ["trainer_id", "section_id"] {
        let mut missing = payload.clone();
        missing.as_object_mut().unwrap().remove(field);
        assert_eq!(
            request(&ctx, &student, Method::POST, &endpoint, Some(missing)).await.0,
            StatusCode::BAD_REQUEST
        );
    }
    // Repeated solves (including different levels/releases), previews and
    // unsolved attempts do not add distinct solved tasks to this section.
    let catalog = request(&ctx, &student, Method::GET, &path, None).await.1;
    assert_eq!(catalog["generation_progress"][&section], json!({"solved":0,"required":3}));
    let teacher_catalog = request(&ctx, &teacher, Method::GET, &path, None).await.1;
    assert_eq!(teacher_catalog["generation_unlock"][&section], true);
    assert!(teacher_catalog.get("generation_progress").is_none());
    for (index, (task, level, preview, solved, sec)) in [
        ("task-easy", "easy", false, true, section.as_str()),
        ("task-easy", "hard", false, true, section.as_str()),
        ("task-hard", "hard", false, true, section.as_str()),
        ("task-third", "medium", true, true, section.as_str()),
        ("task-third", "medium", false, false, section.as_str()),
        ("task-third", "medium", false, true, other_section.as_str()),
    ]
    .into_iter()
    .enumerate()
    {
        sqlx::query("INSERT INTO practice_attempts(id,course_id,student_id,trainer_id,release_id,section_id,difficulty,task_id,task,snapshot,title,preview,solved,helped) VALUES($1,$2,$3,$4,$5,$6,$7,$8,'{}','{}','Test',$9,$10,true)")
            .bind(Uuid::new_v4().to_string()).bind(&course).bind(&student_id).bind(id)
            .bind(Uuid::new_v4().to_string()).bind(sec).bind(level).bind(task).bind(preview).bind(solved)
            .execute(ctx.state.db()).await.unwrap();
        let catalog = request(&ctx, &student, Method::GET, &path, None).await.1;
        assert_eq!(
            catalog["generation_progress"][&section],
            json!({"solved":if index < 2 { 1 } else { 2 },"required":3})
        );
    }
    assert_eq!(
        crate::repositories::trainer_sets::generation_solved_count(
            ctx.state.db(),
            &course,
            &student_id,
            id,
            &section
        )
        .await
        .unwrap(),
        2
    );
    for (c, u, t, s) in [
        (course.as_str(), "other-student", id, section.as_str()),
        ("other-course", student_id.as_str(), id, section.as_str()),
        (course.as_str(), student_id.as_str(), "other-trainer", section.as_str()),
    ] {
        assert_eq!(
            crate::repositories::trainer_sets::generation_solved_count(ctx.state.db(), c, u, t, s)
                .await
                .unwrap(),
            0
        );
    }
    let catalog = request(&ctx, &student, Method::GET, &path, None).await.1;
    assert_eq!(catalog["generation_unlock"][&section], false);
    assert_eq!(
        request(&ctx, &student, Method::POST, &endpoint, Some(payload.clone())).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    sqlx::query("UPDATE practice_attempts SET solved=true WHERE trainer_id=$1 AND section_id=$2 AND task_id='task-third' AND NOT preview").bind(id).bind(&section).execute(ctx.state.db()).await.unwrap();
    let catalog = request(&ctx, &student, Method::GET, &path, None).await.1;
    assert_eq!(catalog["generation_unlock"][&section], true);
    assert_eq!(catalog["generation_progress"][&section], json!({"solved":3,"required":3}));
    // An unlocked section cannot pay for another topic, section or trainer.
    for (field, value) in [("section_id", other_section.as_str()), ("trainer_id", "unknown")] {
        let mut invalid = payload.clone();
        invalid[field] = json!(value);
        assert_eq!(
            request(&ctx, &student, Method::POST, &endpoint, Some(invalid)).await.0,
            StatusCode::BAD_REQUEST
        );
    }
    let mut other_payload = payload.clone();
    other_payload["section_id"] = json!(other_section);
    other_payload["filters"]["topic"] = json!("Другая подтема");
    assert_eq!(
        request(&ctx, &student, Method::POST, &endpoint, Some(other_payload)).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        request(&ctx, &student, Method::POST, &endpoint, Some(payload.clone())).await.0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "Only unlocked requests reach the local Studio mock"
    );
    definition["description"] = json!("Новая публикация");
    let (status, saved) = request(
        &ctx,
        &teacher,
        Method::PUT,
        &path,
        Some(json!({"definition":definition,"revision":2})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    let (status, republished) = request(
        &ctx,
        &teacher,
        Method::POST,
        &format!("{path}/publish"),
        Some(json!({"definition":definition,"revision":saved["revision"]})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{republished}");
    assert_ne!(republished["release_id"], published["release_id"]);
    let catalog = request(&ctx, &student, Method::GET, &path, None).await.1;
    assert_eq!(catalog["generation_unlock"][&section], true);
    assert_eq!(catalog["generation_unlock"][&other_section], false);
    assert_eq!(catalog["generation_progress"][&section], json!({"solved":3,"required":3}));
    assert_eq!(catalog["generation_progress"][&other_section], json!({"solved":1,"required":3}));
    assert_eq!(
        request(&ctx, &student, Method::POST, &endpoint, Some(payload.clone())).await.0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let (status, generated) =
        request(&ctx, &student, Method::POST, &endpoint, Some(payload.clone())).await;
    assert_eq!(status, StatusCode::CREATED, "{generated}");
    assert_eq!(generated["items"].as_array().unwrap().len(), 3);
    assert_eq!(generated["filters"]["count"], 3);
    assert_eq!(generated["filters"]["requested_count"], 5);
    assert_eq!(generated["filters"]["pending_count"], 2);
    assert_eq!(
        calls.load(Ordering::SeqCst),
        3,
        "Partial success must not trigger top-up generation"
    );
    assert_eq!(generated["filters"]["trainer_id"], id);
    assert_eq!(generated["filters"]["section_id"], section);
    let set_path =
        format!("/api/v1/courses/{course}/trainer/sets/{}", generated["id"].as_str().unwrap());
    assert_eq!(
        request(&ctx, &student, Method::GET, &set_path, None).await.1["filters"],
        generated["filters"],
        "origin survives reloading the set"
    );
    let practice_base = format!("/api/v1/courses/{course}/practice");
    let (status, started) = request(&ctx, &student, Method::POST, &format!("{practice_base}/attempts"), Some(json!({"request_id":Uuid::new_v4().to_string(),"set_id":generated["id"],"task_id":generated["items"][0]["id"]}))).await;
    assert_eq!(status, StatusCode::OK, "{started}");
    let attempt_path = format!("{practice_base}/attempts/{}", started["id"].as_str().unwrap());
    let attempt = request(&ctx, &student, Method::GET, &attempt_path, None).await.1;
    assert_eq!(attempt["set_id"], generated["id"]);
    assert_eq!(attempt["can_check"], true);
    assert_eq!(attempt["can_chat"], true);
    assert!(attempt["task"].get("solution").is_none());
    assert!(attempt["task"].get("answer").is_none());
    let (status, edit) = request(
        &ctx,
        &student,
        Method::PUT,
        &attempt_path,
        Some(json!({"draft":"10 / 100 = 0.1","revision":0})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{edit}");
    let (status, checked) = request(&ctx, &student, Method::POST, &format!("{attempt_path}/actions"), Some(json!({"request_id":Uuid::new_v4().to_string(),"kind":"check","revision":edit["revision"]}))).await;
    assert_eq!(status, StatusCode::OK, "{checked}");
    crate::services::practice::tick(&ctx.state).await.unwrap();
    let attempt = request(&ctx, &student, Method::GET, &attempt_path, None).await.1;
    assert_eq!(attempt["jobs"][0]["status"], "completed", "{attempt}");
    assert_eq!(attempt["solved"], true, "{attempt}");
    let (status, next_set) =
        request(&ctx, &student, Method::POST, &endpoint, Some(payload.clone())).await;
    assert_eq!(status, StatusCode::CREATED, "{next_set}");
    assert_ne!(next_set["id"], generated["id"]);
    assert_eq!(next_set["items"].as_array().unwrap().len(), 5);
    assert_eq!(next_set["filters"]["count"], 5);
    assert_eq!(next_set["filters"]["requested_count"], 5);
    assert_eq!(next_set["filters"]["pending_count"], 0);
    assert_eq!(next_set["filters"]["section_id"], section);
    assert_eq!(calls.load(Ordering::SeqCst), 4);
    assert_eq!(
        request(&ctx, &teacher, Method::POST, &format!("{path}/unpublish"), None).await.0,
        StatusCode::OK
    );
    assert_eq!(
        request(&ctx, &student, Method::POST, &endpoint, Some(payload)).await.0,
        StatusCode::BAD_REQUEST
    );
    // Existing non-physical generation still works without catalog IDs/solves.
    sqlx::query("UPDATE task_bank_sources SET code='bank' WHERE id='bank'")
        .execute(ctx.state.db())
        .await
        .unwrap();
    assert_eq!(
        request(&ctx, &student, Method::POST, &endpoint, Some(json!({"source":"bank","count":1})))
            .await
            .0,
        StatusCode::CREATED
    );
    assert_eq!(calls.load(Ordering::SeqCst), 4);
    server.abort();
}
#[tokio::test]
async fn author_publish_student_practice_and_isolation() {
    let ctx = test_support::setup_test_context().await;
    let (course, teacher, student, student_id, definition) = fixture(&ctx).await;
    let base = format!("/api/v1/courses/{course}/practice");
    assert_eq!(
        request(
            &ctx,
            &student,
            Method::POST,
            &format!("{base}/catalog"),
            Some(json!({"definition":definition}))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let (status, t) = request(
        &ctx,
        &teacher,
        Method::POST,
        &format!("{base}/catalog"),
        Some(json!({"definition":definition})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{t}");
    let id = t["id"].as_str().unwrap();
    assert_eq!(
        request(&ctx, &student, Method::GET, &format!("{base}/catalog"), None).await.1["items"],
        json!([])
    );
    let publish = json!({"definition":definition,"revision":1});
    let (s, v) = request(
        &ctx,
        &teacher,
        Method::POST,
        &format!("{base}/catalog/{id}/publish"),
        Some(publish),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let catalog = request(&ctx, &student, Method::GET, &format!("{base}/catalog"), None).await.1;
    assert_eq!(catalog["items"][0]["definition"]["title"], "Растворы");
    let start = json!({"request_id":Uuid::new_v4().to_string(),"trainer_id":id,"section_id":definition["sections"][0]["id"],"difficulty":"easy"});
    let (s, a) =
        request(&ctx, &student, Method::POST, &format!("{base}/attempts"), Some(start.clone()))
            .await;
    assert_eq!(s, StatusCode::OK, "{a}");
    let attempt = a["id"].as_str().unwrap();
    let path = format!("{base}/attempts/{attempt}");
    assert_eq!(
        request(&ctx, &student, Method::POST, &format!("{base}/attempts"), Some(start)).await.1,
        a
    );
    let v = request(&ctx, &student, Method::GET, &path, None).await.1;
    assert_eq!(v["task"]["id"], "task-easy");
    assert!(v.get("snapshot").is_none());
    assert!(v["task"].get("solution").is_none());
    assert!(v["task"].get("answer").is_none());
    assert_eq!(v["can_check"], true);
    assert_eq!(request(&ctx, &teacher, Method::GET, &path, None).await.0, StatusCode::NOT_FOUND);
    // Chat is available before a solution. Retrying one request creates one job/message.
    let action = json!({"request_id":Uuid::new_v4().to_string(),"kind":"message","message":"С чего начать?","revision":0});
    assert_eq!(
        request(&ctx, &student, Method::POST, &format!("{path}/actions"), Some(action.clone()))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        request(&ctx, &student, Method::POST, &format!("{path}/actions"), Some(action)).await.0,
        StatusCode::OK
    );
    let v = request(&ctx, &student, Method::GET, &path, None).await.1;
    assert_eq!(v["jobs"].as_array().unwrap().len(), 1);
    assert_eq!(v["messages"].as_array().unwrap().len(), 1);
    assert_eq!(
        request(
            &ctx,
            &student,
            Method::PUT,
            &path,
            Some(json!({"draft":"Новое решение","revision":0}))
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let (s,_) = request(&ctx,&student,Method::POST,&format!("{base}/attempts"),Some(json!({"request_id":Uuid::new_v4().to_string(),"trainer_id":id,"section_id":definition["sections"][0]["id"],"difficulty":"easy","preview":true}))).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let jobs: i64 = sqlx::query_scalar("SELECT count(*) FROM exam_sessions WHERE student_id=$1")
        .bind(&student_id)
        .fetch_one(ctx.state.db())
        .await
        .unwrap();
    assert_eq!(jobs, 0);
    // A draft edit leaves the published definition and started task unchanged.
    let mut changed = definition.clone();
    changed["title"] = json!("Черновик новой темы");
    assert_eq!(
        request(
            &ctx,
            &teacher,
            Method::PUT,
            &format!("{base}/catalog/{id}"),
            Some(json!({"definition":changed,"revision":2}))
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        request(&ctx, &student, Method::GET, &format!("{base}/catalog"), None).await.1["items"][0]
            ["definition"]["title"],
        "Растворы"
    );
    sqlx::query("UPDATE task_bank_items SET text='Изменено' WHERE id='task-easy'")
        .execute(ctx.state.db())
        .await
        .unwrap();
    assert_eq!(
        request(&ctx, &student, Method::GET, &path, None).await.1["task"]["text"],
        "Найдите массовую долю"
    );
}
#[tokio::test]
async fn worker_uses_pinned_tutor_then_production_grader_and_persists_progress() {
    let mut ctx = test_support::setup_test_context().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app=axum::Router::new().route("/chat/completions",axum::routing::post(|Json(v):Json<Value>|async move{
  let system=v["messages"][0]["content"].as_str().unwrap();
  let content=if system.contains("ОБЯЗАТЕЛЬНЫЙ КОНТРАКТ ПЛАТФОРМЫ") {json!({"unreadable":false,"total_score":5,"max_score":5,"criteria_scores":[{"criterion_name":"Метод","score":5,"max_score":5,"comment":"Верно"}],"feedback":"Массовая доля найдена верно."}).to_string()}else{assert!(system.contains("РЕЖИМ УЧЕБНОЙ ПРАКТИКИ"));assert!(system.contains("Найдите массовую долю"));"Начните с массы вещества и массы раствора.".into()};
  Json(json!({"choices":[{"message":{"content":content}}]}))
 }));
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let prior = std::env::var("ASSISTANT_AI_BASE_URL").ok();
    std::env::set_var("ASSISTANT_AI_BASE_URL", format!("http://{address}"));
    let settings = crate::core::config::Settings::load().unwrap();
    ctx.state = crate::core::state::AppState::new(
        settings,
        ctx.state.db().clone(),
        ctx.state.redis().clone(),
        None,
    );
    ctx.app = crate::api::router::router(ctx.state.clone());
    let (course, teacher, student, _, definition) = fixture(&ctx).await;
    let base = format!("/api/v1/courses/{course}/practice");
    let t = request(
        &ctx,
        &teacher,
        Method::POST,
        &format!("{base}/catalog"),
        Some(json!({"definition":definition})),
    )
    .await
    .1;
    let trainer = t["id"].as_str().unwrap();
    let (status, error) = request(
        &ctx,
        &teacher,
        Method::POST,
        &format!("{base}/catalog/{trainer}/publish"),
        Some(json!({"definition":definition,"revision":1})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{error}");
    let a=request(&ctx,&student,Method::POST,&format!("{base}/attempts"),Some(json!({"request_id":Uuid::new_v4().to_string(),"trainer_id":trainer,"section_id":definition["sections"][0]["id"],"difficulty":"easy"}))).await.1;
    let path = format!("{base}/attempts/{}", a["id"].as_str().unwrap());
    request(&ctx,&student,Method::POST,&format!("{path}/actions"),Some(json!({"request_id":Uuid::new_v4().to_string(),"kind":"message","message":"С чего начать?","revision":0}))).await;
    crate::services::practice::tick(&ctx.state).await.unwrap();
    let a = request(&ctx, &student, Method::GET, &path, None).await.1;
    assert_eq!(a["jobs"][0]["status"], "completed", "{a}");
    assert_eq!(a["revision"], 1);
    assert_eq!(a["solved"], false);
    assert_eq!(
        request(
            &ctx,
            &student,
            Method::PUT,
            &path,
            Some(json!({"draft":"10 / 100 = 0.1 = 10%","revision":1}))
        )
        .await
        .0,
        StatusCode::OK
    );
    request(
        &ctx,
        &student,
        Method::POST,
        &format!("{path}/actions"),
        Some(json!({"request_id":Uuid::new_v4().to_string(),"kind":"check","revision":2})),
    )
    .await;
    crate::services::practice::tick(&ctx.state).await.unwrap();
    let a = request(&ctx, &student, Method::GET, &path, None).await.1;
    assert_eq!(a["solved"], true, "{a}");
    assert_eq!(a["checks"][0]["total_score"], 5);
    assert_eq!(a["checks"][0]["helped"], true);
    let t = request(&ctx, &student, Method::GET, &format!("{base}/catalog"), None).await.1;
    assert_eq!(t["items"][0]["progress"][0]["solved"], 1);
    assert_eq!(t["items"][0]["progress"][0]["independent"], 0);
    server.abort();
    match prior {
        Some(v) => std::env::set_var("ASSISTANT_AI_BASE_URL", v),
        None => std::env::remove_var("ASSISTANT_AI_BASE_URL"),
    }
}

#[tokio::test]
async fn photo_upload_runs_durable_ocr_without_submitting_solution() {
    let mut ctx = test_support::setup_test_context().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = axum::Router::new()
        .route(
            "/marker",
            axum::routing::post(|| async { Json(json!({"success":true,"request_id":"ocr-one"})) }),
        )
        .route(
            "/marker/ocr-one",
            axum::routing::get(|| async {
                Json(json!({"status":"complete","markdown":"m = 10 г; масса раствора 100 г"}))
            }),
        )
        .fallback(axum::routing::any(|| async { StatusCode::OK }));
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let vars = [
        ("S3_ENDPOINT", format!("http://{address}")),
        ("S3_ACCESS_KEY", "test-key".into()),
        ("S3_SECRET_KEY", "test-secret".into()),
        ("S3_BUCKET", "practice-photos".into()),
        ("S3_REGION", "us-east-1".into()),
        ("DATALAB_BASE_URL", format!("http://{address}")),
    ];
    let prior = vars.iter().map(|(k, _)| (*k, std::env::var(k).ok())).collect::<Vec<_>>();
    for (k, v) in &vars {
        std::env::set_var(k, v);
    }
    let settings = crate::core::config::Settings::load().unwrap();
    let storage = crate::services::storage::StorageService::from_settings(&settings).await.unwrap();
    ctx.state = crate::core::state::AppState::new(
        settings,
        ctx.state.db().clone(),
        ctx.state.redis().clone(),
        storage,
    );
    ctx.app = crate::api::router::router(ctx.state.clone());
    let (course, teacher, student, _, definition) = fixture(&ctx).await;
    let base = format!("/api/v1/courses/{course}/practice");
    let t = request(
        &ctx,
        &teacher,
        Method::POST,
        &format!("{base}/catalog"),
        Some(json!({"definition":definition})),
    )
    .await
    .1;
    let trainer = t["id"].as_str().unwrap();
    request(
        &ctx,
        &teacher,
        Method::POST,
        &format!("{base}/catalog/{trainer}/publish"),
        Some(json!({"definition":definition,"revision":1})),
    )
    .await;
    let a=request(&ctx,&student,Method::POST,&format!("{base}/attempts"),Some(json!({"request_id":Uuid::new_v4().to_string(),"trainer_id":trainer,"section_id":definition["sections"][0]["id"],"difficulty":"easy"}))).await.1;
    let path = format!("{base}/attempts/{}", a["id"].as_str().unwrap());
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(20, 20).write_to(&mut png, image::ImageFormat::Png).unwrap();
    let mut body=b"--BOUNDARY\r\nContent-Disposition: form-data; name=\"file\"; filename=\"solution.png\"\r\nContent-Type: image/png\r\n\r\n".to_vec();
    body.extend(png.into_inner());
    body.extend(b"\r\n--BOUNDARY--\r\n");
    let req = axum::http::Request::builder()
        .method(Method::POST)
        .uri(format!("{path}/photos"))
        .header("authorization", format!("Bearer {student}"))
        .header("content-type", "multipart/form-data; boundary=BOUNDARY")
        .body(axum::body::Body::from(body))
        .unwrap();
    let response = ctx.app.clone().oneshot(req).await.unwrap();
    let status = response.status();
    let value = test_support::read_json(response).await;
    assert_eq!(status, StatusCode::OK, "{value}");
    crate::services::practice::tick(&ctx.state).await.unwrap();
    let a = request(&ctx, &student, Method::GET, &path, None).await.1;
    assert_eq!(a["jobs"][0]["status"], "completed", "{a}");
    assert!(a["draft"].as_str().unwrap().contains("100 г"));
    assert_eq!(a["photos"].as_array().unwrap().len(), 1);
    assert_eq!(a["checks"], json!([]));
    assert_eq!(a["solved"], false);
    server.abort();
    for (k, v) in prior {
        match v {
            Some(v) => std::env::set_var(k, v),
            None => std::env::remove_var(k),
        }
    }
}

/// Isolated browser preview against the real API/database; external models are stubbed.
#[tokio::test]
#[ignore]
async fn manual_preview_server() {
    let mut ctx = test_support::setup_test_context().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let model_app=axum::Router::new().route("/chat/completions",axum::routing::post(|Json(v):Json<Value>|async move{
  let content=if v["response_format"]["type"]=="json_object" {json!({"unreadable":false,"total_score":5,"max_score":5,"criteria_scores":[{"criterion_name":"Метод","score":5,"max_score":5,"comment":"Отношение массы вещества к массе раствора записано правильно."}],"feedback":"Метод выбран верно. Для массовой доли разделите массу вещества на массу раствора, а для процентов умножьте на 100."}).to_string()}else{"Начнём с определения: массовая доля — это отношение массы растворённого вещества к массе всего раствора. Какие две массы известны в условии?".into()};Json(json!({"choices":[{"message":{"content":content}}]}))
 }));
    tokio::spawn(async move {
        axum::serve(listener, model_app).await.unwrap();
    });
    std::env::set_var("ASSISTANT_AI_BASE_URL", format!("http://{address}"));
    std::env::set_var("BACKEND_CORS_ORIGINS", "http://localhost:5188");
    let settings = crate::core::config::Settings::load().unwrap();
    ctx.state = crate::core::state::AppState::new(
        settings,
        ctx.state.db().clone(),
        ctx.state.redis().clone(),
        None,
    );
    ctx.app = crate::api::router::router(ctx.state.clone());
    let (course, _, _, _, _) = fixture(&ctx).await;
    std::fs::create_dir_all("output/playwright").unwrap();
    std::fs::write("output/playwright/practice-preview.json",json!({"course_id":course,"api":"http://localhost:8188/api/v1","teacher":"author","student":"learner","password":"password"}).to_string()).unwrap();
    let worker = ctx.state.clone();
    tokio::spawn(async move {
        loop {
            let _ = crate::services::practice::tick(&worker).await;
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:8188").await.unwrap();
    axum::serve(listener, ctx.app.clone()).await.unwrap();
}

#[tokio::test]
async fn personal_sets_keep_self_check_without_published_ai() {
    let ctx = test_support::setup_test_context().await;
    let (course, _, student, _, _) = fixture(&ctx).await;
    sqlx::query("UPDATE task_bank_items SET solution='',answer='Ответ: 10%' WHERE id='task-easy'")
        .execute(ctx.state.db())
        .await
        .unwrap();
    let prefix = format!("/api/v1/courses/{course}");
    let (status, set) = request(
        &ctx,
        &student,
        Method::POST,
        &format!("{prefix}/trainer/sets/manual"),
        Some(json!({"source":"bank","numbers":["task-easy"],"title":"Мои задачи"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{set}");
    sqlx::query("DELETE FROM course_ai_assistants WHERE course_id=$1")
        .bind(&course)
        .execute(ctx.state.db())
        .await
        .unwrap();
    let (status,attempt)=request(&ctx,&student,Method::POST,&format!("{prefix}/practice/attempts"),Some(json!({"request_id":Uuid::new_v4().to_string(),"set_id":set["id"],"task_id":"task-easy"}))).await;
    assert_eq!(status, StatusCode::OK, "{attempt}");
    let path = format!("{prefix}/practice/attempts/{}", attempt["id"].as_str().unwrap());
    let a = request(&ctx, &student, Method::GET, &path, None).await.1;
    assert_eq!(a["can_chat"], false);
    assert_eq!(a["can_check"], false);
    assert!(a["task"].get("answer").is_none());
    let status = request(
        &ctx,
        &student,
        Method::POST,
        &format!("{path}/actions"),
        Some(json!({"request_id":Uuid::new_v4().to_string(),"kind":"reveal","revision":0})),
    )
    .await
    .0;
    assert_eq!(status, StatusCode::OK);
    let a = request(&ctx, &student, Method::GET, &path, None).await.1;
    assert_eq!(a["revealed"], true);
    assert_eq!(a["solved"], false);
    assert!(a["messages"][0]["content"].as_str().unwrap().contains("Ответ: 10%"));
}
