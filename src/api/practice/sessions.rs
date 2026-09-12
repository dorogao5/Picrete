use super::catalog::{db, Definition};
use crate::{
    api::{
        errors::ApiError,
        guards::{require_course_membership, require_course_role, CurrentUser},
    },
    core::state::AppState,
    db::types::CourseRole,
    repositories,
};
use axum::{
    extract::{Multipart, Path, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

pub(super) async fn snapshot(state: &AppState, course: &str) -> Result<Value, ApiError> {
    repositories::course_ai_assistants::find(state.db(), course)
        .await
        .map_err(db)?
        .filter(|s| s.enabled)
        .map(|s| s.snapshot)
        .ok_or_else(|| ApiError::BadRequest("Ассистент курса ещё не опубликован".into()))
}
#[derive(Deserialize)]
pub(super) struct Start {
    request_id: String,
    trainer_id: Option<String>,
    section_id: Option<String>,
    difficulty: Option<String>,
    set_id: Option<String>,
    task_id: Option<String>,
    #[serde(default)]
    preview: bool,
    #[serde(default)]
    next: bool,
}
pub(super) async fn start(
    Path(course): Path<String>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
    Json(body): Json<Start>,
) -> Result<Json<Value>, ApiError> {
    require_course_membership(&state, &user, &course).await?;
    if body.preview {
        require_course_role(&state, &user, &course, CourseRole::Teacher).await?;
    } else {
        require_course_role(&state, &user, &course, CourseRole::Student).await?;
    }
    Uuid::parse_str(&body.request_id)
        .map_err(|_| ApiError::BadRequest("Некорректный запрос".into()))?;
    let mut tx = state.db().begin().await.map_err(db)?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("practice-start:{course}:{}", user.id))
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    if let Some(id) = sqlx::query_scalar::<_, String>(
        "SELECT id FROM practice_attempts WHERE id=$1 AND student_id=$2 AND course_id=$3",
    )
    .bind(&body.request_id)
    .bind(&user.id)
    .bind(&course)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db)?
    {
        return Ok(Json(json!({"id":id})));
    }
    let (ids, title, release, difficulty) = if let Some(trainer) = &body.trainer_id {
        if body.set_id.is_some() {
            return Err(ApiError::BadRequest("Выберите один источник задачи".into()));
        }
        let r = sqlx::query(
            "SELECT draft,published,release_id FROM course_trainers WHERE id=$1 AND course_id=$2",
        )
        .bind(trainer)
        .bind(&course)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db)?
        .ok_or_else(|| ApiError::NotFound("Тренажёр не найден".into()))?;
        let v: Option<Value> = if body.preview { Some(r.get("draft")) } else { r.get("published") };
        let def: Definition = serde_json::from_value(
            v.ok_or_else(|| ApiError::NotFound("Тренажёр ещё не опубликован".into()))?,
        )
        .map_err(db)?;
        let s = def
            .sections
            .iter()
            .find(|s| Some(&s.id) == body.section_id.as_ref())
            .ok_or_else(|| ApiError::BadRequest("Подтема не найдена".into()))?;
        let level = body.difficulty.clone().unwrap_or_else(|| "easy".into());
        (
            s.items
                .iter()
                .filter(|i| i.difficulty == level)
                .map(|i| i.task_id.clone())
                .collect::<Vec<_>>(),
            format!("{} · {}", def.title, s.title),
            r.get::<Option<String>, _>("release_id"),
            level,
        )
    } else if let Some(set) = &body.set_id {
        let s = repositories::trainer_sets::find_for_student(state.db(), &course, &user.id, set)
            .await
            .map_err(db)?
            .ok_or_else(|| ApiError::NotFound("Личный набор не найден".into()))?;
        let mut ids =
            repositories::trainer_sets::list_item_ids(state.db(), set).await.map_err(db)?;
        if let Some(task) = &body.task_id {
            ids.retain(|i| i == task);
        }
        (ids, s.title, None, "personal".into())
    } else {
        return Err(ApiError::BadRequest("Выберите тренажёр или личный набор".into()));
    };
    if ids.is_empty() {
        return Err(ApiError::BadRequest("На этом уровне пока нет задач".into()));
    }
    if !body.next {
        let active=sqlx::query_scalar::<_,String>("SELECT id FROM practice_attempts WHERE course_id=$1 AND student_id=$2 AND trainer_id IS NOT DISTINCT FROM $3 AND set_id IS NOT DISTINCT FROM $4 AND section_id IS NOT DISTINCT FROM $5 AND difficulty=$6 AND preview=$7 AND NOT solved AND task_id=ANY($8) AND release_id IS NOT DISTINCT FROM $9 ORDER BY updated_at DESC LIMIT 1").bind(&course).bind(&user.id).bind(&body.trainer_id).bind(&body.set_id).bind(&body.section_id).bind(&difficulty).bind(body.preview).bind(&ids).bind(&release).fetch_optional(&mut *tx).await.map_err(db)?;
        if let Some(id) = active {
            return Ok(Json(json!({"id":id})));
        }
    }
    let tasks = repositories::task_bank::list_items_with_source_by_ids_for_course(
        state.db(),
        &course,
        &ids,
    )
    .await
    .map_err(db)?;
    if tasks.len() != ids.len() {
        return Err(ApiError::BadRequest(
            "Состав банка изменился. Обратитесь к преподавателю".into(),
        ));
    }
    let chosen=sqlx::query_scalar::<_,String>("SELECT t.id FROM task_bank_items t LEFT JOIN LATERAL (SELECT max(created_at) AS seen FROM practice_attempts WHERE student_id=$1 AND course_id=$2 AND task_id=t.id) p ON true WHERE t.id=ANY($3) ORDER BY p.seen ASC NULLS FIRST,random() LIMIT 1").bind(&user.id).bind(&course).bind(&ids).fetch_one(&mut *tx).await.map_err(db)?;
    let t = tasks
        .iter()
        .find(|t| t.id == chosen)
        .ok_or_else(|| ApiError::BadRequest("Задача недоступна".into()))?;
    let image_rows = repositories::task_bank::list_item_images_by_item_ids(
        state.db(),
        std::slice::from_ref(&chosen),
    )
    .await
    .map_err(db)?;
    let images=image_rows.iter().map(|i|json!({"id":i.id,"full_url":format!("{}/courses/{course}/task-bank/items/{chosen}/images/{}/view?size=full",state.settings().api().api_v1_str,i.id)})).collect::<Vec<_>>();
    let snap = if body.trainer_id.is_some() {
        snapshot(&state, &course).await?
    } else {
        repositories::course_ai_assistants::find(state.db(), &course)
            .await
            .map_err(db)?
            .filter(|s| s.enabled)
            .map(|s| s.snapshot)
            .unwrap_or_else(|| json!({}))
    };
    let task = json!({"id":t.id,"number":t.number,"text":t.text,"solution":t.solution,"answer":t.answer,"images":images});
    let revealed:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM practice_attempts WHERE student_id=$1 AND course_id=$2 AND task_id=$3 AND revealed)").bind(&user.id).bind(&course).bind(&chosen).fetch_one(&mut *tx).await.map_err(db)?;
    sqlx::query("INSERT INTO practice_attempts(id,course_id,student_id,trainer_id,release_id,section_id,set_id,difficulty,task_id,task,snapshot,title,preview,revealed) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)").bind(&body.request_id).bind(&course).bind(&user.id).bind(&body.trainer_id).bind(release).bind(body.section_id).bind(body.set_id).bind(difficulty).bind(chosen).bind(task).bind(snap).bind(title).bind(body.preview).bind(revealed).execute(&mut *tx).await.map_err(db)?;
    tx.commit().await.map_err(db)?;
    Ok(Json(json!({"id":body.request_id})))
}
async fn owned(state: &AppState, course: &str, user: &str, id: &str) -> Result<Value, ApiError> {
    sqlx::query_scalar::<_,Value>("SELECT to_jsonb(a) FROM practice_attempts a WHERE id=$1 AND course_id=$2 AND student_id=$3").bind(id).bind(course).bind(user).fetch_optional(state.db()).await.map_err(db)?.ok_or_else(||ApiError::NotFound("Попытка не найдена".into()))
}
pub(super) async fn get(
    Path((course, id)): Path<(String, String)>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
) -> Result<Json<Value>, ApiError> {
    require_course_membership(&state, &user, &course).await?;
    let mut a = owned(&state, &course, &user.id, &id).await?;
    a["can_check"] = json!(a["task"]["solution"].as_str().is_some_and(|s| !s.trim().is_empty()));
    a["can_check"] = json!(
        a["can_check"] == true
            && crate::services::ai_grading::validate_grading_snapshot(
                &a["snapshot"],
                &crate::services::ai_grading::snapshot_decision_model(
                    &a["snapshot"],
                    &state.settings().ai().assistant_model,
                )
            )
            .is_ok()
            && crate::services::ai_grading::bank_rubric(&a["snapshot"]).is_ok()
    );
    a["can_chat"] = json!(a["snapshot"]
        .pointer("/prompts/tutor/system_prompt")
        .and_then(Value::as_str)
        .is_some_and(|s| !s.trim().is_empty()));
    a.as_object_mut().unwrap().remove("snapshot");
    if let Some(t) = a["task"].as_object_mut() {
        t.remove("solution");
        t.remove("answer");
    }
    a["jobs"]=json!(sqlx::query_scalar::<_,Value>("SELECT to_jsonb(j)-'payload' FROM practice_jobs j WHERE attempt_id=$1 ORDER BY created_at DESC LIMIT 10").bind(&id).fetch_all(state.db()).await.map_err(db)?);
    let photos=sqlx::query("SELECT id,storage_key,filename FROM practice_photos WHERE attempt_id=$1 ORDER BY created_at").bind(&id).fetch_all(state.db()).await.map_err(db)?;
    let mut p = Vec::new();
    for r in photos {
        let url = if let Some(s) = state.storage() {
            s.presign_get(&r.get::<String, _>("storage_key"), std::time::Duration::from_secs(600))
                .await
                .map_err(db)?
        } else {
            String::new()
        };
        p.push(json!({"id":r.get::<String,_>("id"),"filename":r.get::<String,_>("filename"),"url":url}));
    }
    a["photos"] = json!(p);
    Ok(Json(a))
}
#[derive(Deserialize)]
pub(super) struct Edit {
    revision: i32,
    draft: String,
}
pub(super) async fn edit(
    Path((course, id)): Path<(String, String)>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
    Json(body): Json<Edit>,
) -> Result<Json<Value>, ApiError> {
    require_course_membership(&state, &user, &course).await?;
    if body.draft.len() > 60000 {
        return Err(ApiError::BadRequest("Решение слишком длинное".into()));
    }
    let mut tx = state.db().begin().await.map_err(db)?;
    let current = sqlx::query_scalar::<_,i32>("SELECT revision FROM practice_attempts WHERE id=$1 AND course_id=$2 AND student_id=$3 FOR UPDATE")
        .bind(&id).bind(&course).bind(&user.id).fetch_optional(&mut *tx).await.map_err(db)?
        .ok_or_else(||ApiError::NotFound("Попытка не найдена".into()))?;
    let pending:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM practice_jobs WHERE attempt_id=$1 AND status IN ('queued','running'))").bind(&id).fetch_one(&mut *tx).await.map_err(db)?;
    if current != body.revision || pending {
        return Err(ApiError::Conflict(
            "Дождитесь обработки или обновите попытку: решение изменилось".into(),
        ));
    }
    let rev=sqlx::query_scalar::<_,i32>("UPDATE practice_attempts SET draft=$1,revision=revision+1,updated_at=now() WHERE id=$2 RETURNING revision").bind(body.draft).bind(&id).fetch_one(&mut *tx).await.map_err(db)?;
    tx.commit().await.map_err(db)?;
    Ok(Json(json!({"revision":rev})))
}
#[derive(Deserialize)]
pub(super) struct Action {
    request_id: String,
    kind: String,
    #[serde(default)]
    message: String,
    revision: i32,
}
pub(super) async fn action(
    Path((course, id)): Path<(String, String)>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
    Json(body): Json<Action>,
) -> Result<Json<Value>, ApiError> {
    require_course_membership(&state, &user, &course).await?;
    let a = owned(&state, &course, &user.id, &id).await?;
    if !state
        .redis()
        .rate_limit(&format!("practice:{course}:{}", user.id), 20, 60)
        .await
        .map_err(db)?
    {
        return Err(ApiError::TooManyRequests("Слишком много запросов. Подождите минуту"));
    }
    if body.kind == "retry" {
        let previous=sqlx::query("SELECT kind,payload FROM practice_jobs WHERE id=$1 AND attempt_id=$2 AND status='failed'").bind(&body.message).bind(&id).fetch_optional(state.db()).await.map_err(db)?.ok_or_else(||ApiError::NotFound("Неудачный запрос не найден".into()))?;
        let mut payload: Value = previous.get("payload");
        payload["retry"] = json!(true);
        enqueue(
            &state,
            &id,
            &body.request_id,
            &previous.get::<String, _>("kind"),
            payload,
            body.revision,
        )
        .await?;
        return Ok(Json(json!({"job_id":body.request_id})));
    }
    if body.kind == "reveal" {
        let content = a["task"]["solution"]
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| a["task"]["answer"].as_str().filter(|s| !s.trim().is_empty()))
            .ok_or_else(|| ApiError::BadRequest("Разбор ещё не добавлен".into()))?;
        let mut tx = state.db().begin().await.map_err(db)?;
        sqlx::query("SELECT id FROM practice_attempts WHERE id=$1 FOR UPDATE")
            .bind(&id)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        let result=sqlx::query("UPDATE practice_attempts SET revealed=true,helped=true,messages=messages || $1::jsonb,revision=revision+1,updated_at=now() WHERE id=$2 AND revision=$3 AND NOT EXISTS(SELECT 1 FROM practice_jobs WHERE attempt_id=$2 AND status IN ('queued','running'))").bind(json!([{"role":"assistant","content":format!("Разбор задачи\n\n{content}"),"kind":"reveal"}])).bind(&id).bind(body.revision).execute(&mut *tx).await.map_err(db)?;
        if result.rows_affected() == 0 {
            return Err(ApiError::Conflict("Попытка изменилась. Обновите страницу".into()));
        }
        tx.commit().await.map_err(db)?;
        return Ok(Json(json!({"ok":true})));
    }
    if !["message", "check"].contains(&body.kind.as_str())
        || body.message.len() > 12000
        || (body.kind == "message" && body.message.trim().is_empty())
    {
        return Err(ApiError::BadRequest("Введите вопрос".into()));
    }
    if body.kind == "message"
        && a["snapshot"].pointer("/prompts/tutor/system_prompt").and_then(Value::as_str).is_none()
    {
        return Err(ApiError::BadRequest(
            "Помощник курса ещё не настроен. Доступен просмотр ответа.".into(),
        ));
    }
    if body.kind == "check"
        && (a["draft"].as_str().unwrap_or("").trim().is_empty()
            || a["task"]["solution"].as_str().is_none_or(|s| s.trim().is_empty()))
    {
        return Err(ApiError::BadRequest(
            "Для проверки нужны решение студента и эталон задачи".into(),
        ));
    }
    enqueue(
        &state,
        &id,
        &body.request_id,
        &body.kind,
        json!({"message":body.message}),
        body.revision,
    )
    .await?;
    Ok(Json(json!({"job_id":body.request_id})))
}
async fn enqueue(
    state: &AppState,
    id: &str,
    job: &str,
    kind: &str,
    payload: Value,
    revision: i32,
) -> Result<(), ApiError> {
    Uuid::parse_str(job)
        .map_err(|_| ApiError::BadRequest("Некорректный идентификатор запроса".into()))?;
    let mut tx = state.db().begin().await.map_err(db)?;
    let current: i32 =
        sqlx::query_scalar("SELECT revision FROM practice_attempts WHERE id=$1 FOR UPDATE")
            .bind(id)
            .fetch_one(&mut *tx)
            .await
            .map_err(db)?;
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM practice_jobs WHERE id=$1 AND attempt_id=$2)",
    )
    .bind(job)
    .bind(id)
    .fetch_one(&mut *tx)
    .await
    .map_err(db)?;
    if exists {
        return Ok(());
    }
    let busy:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM practice_jobs WHERE attempt_id=$1 AND status IN ('queued','running'))").bind(id).fetch_one(&mut *tx).await.map_err(db)?;
    if current != revision || busy {
        return Err(ApiError::Conflict("Дождитесь предыдущего ответа и обновите попытку".into()));
    }
    let message_count: i32 = sqlx::query_scalar(
        "SELECT jsonb_array_length(messages) FROM practice_attempts WHERE id=$1",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await
    .map_err(db)?;
    if message_count >= 400 {
        return Err(ApiError::BadRequest("Диалог слишком длинный. Начните новую задачу.".into()));
    }
    if kind == "ocr" && payload["retry"] != true {
        let count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM practice_photos WHERE attempt_id=$1")
                .bind(id)
                .fetch_one(&mut *tx)
                .await
                .map_err(db)?;
        if count >= 10 {
            return Err(ApiError::BadRequest("В одной попытке можно загрузить до 10 фото".into()));
        }
    }
    sqlx::query(
        "INSERT INTO practice_jobs(id,attempt_id,kind,payload,revision) VALUES($1,$2,$3,$4,$5)",
    )
    .bind(job)
    .bind(id)
    .bind(kind)
    .bind(&payload)
    .bind(revision)
    .execute(&mut *tx)
    .await
    .map_err(db)?;
    if kind == "message" && payload["retry"] != true {
        sqlx::query(
            "UPDATE practice_attempts SET messages=messages || $1::jsonb,helped=true WHERE id=$2",
        )
        .bind(json!([{"role":"user","content":payload["message"]}]))
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    }
    tx.commit().await.map_err(db)?;
    Ok(())
}
pub(super) async fn upload(
    Path((course, id)): Path<(String, String)>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<Value>, ApiError> {
    require_course_membership(&state, &user, &course).await?;
    let a = owned(&state, &course, &user.id, &id).await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM practice_photos WHERE attempt_id=$1")
        .bind(&id)
        .fetch_one(state.db())
        .await
        .map_err(db)?;
    if count >= 10 {
        return Err(ApiError::BadRequest("В одной попытке можно загрузить до 10 фото".into()));
    }
    let field = multipart
        .next_field()
        .await
        .map_err(db)?
        .ok_or_else(|| ApiError::BadRequest("Выберите фото".into()))?;
    let name = field.file_name().unwrap_or("Фото").chars().take(180).collect::<String>();
    let bytes = field.bytes().await.map_err(db)?.to_vec();
    let format = image::guess_format(&bytes)
        .map_err(|_| ApiError::BadRequest("Нужна фотография JPEG или PNG".into()))?;
    let mime = match format {
        image::ImageFormat::Jpeg => "image/jpeg",
        image::ImageFormat::Png => "image/png",
        _ => return Err(ApiError::BadRequest("Поддерживаются JPEG и PNG".into())),
    };
    if bytes.len() > 10 * 1024 * 1024 {
        return Err(ApiError::BadRequest("Фото должно быть не больше 10 МБ".into()));
    }
    let mut reader = image::ImageReader::new(std::io::Cursor::new(&bytes));
    reader.set_format(format);
    let (w, h) = reader
        .into_dimensions()
        .map_err(|_| ApiError::BadRequest("Не удалось прочитать фото".into()))?;
    if u64::from(w) * u64::from(h) > 40_000_000 {
        return Err(ApiError::BadRequest("Уменьшите фото до 40 мегапикселей".into()));
    }
    let photo = Uuid::new_v4().to_string();
    let key = format!("practice/{course}/{id}/{photo}");
    let storage = state
        .storage()
        .ok_or_else(|| ApiError::ServiceUnavailable("Загрузка фото временно недоступна".into()))?;
    let bytes = tokio::task::spawn_blocking(move || {
        crate::services::submission_images::normalize_jpeg_orientation(bytes, mime)
    })
    .await
    .map_err(db)?;
    storage.upload_bytes(&key, mime, bytes).await.map_err(db)?;
    let job = Uuid::new_v4().to_string();
    if let Err(e) = enqueue(
        &state,
        &id,
        &job,
        "ocr",
        json!({"storage_key":key,"photo_id":photo,"filename":name}),
        a["revision"].as_i64().unwrap_or(0) as i32,
    )
    .await
    {
        let _ = storage.delete_object(&key).await;
        return Err(e);
    }
    // Photo metadata is inserted by the same durable job that performs OCR.
    Ok(Json(json!({"job_id":job})))
}
