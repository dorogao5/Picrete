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
    extract::{Path, Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

pub(super) fn db(e: impl std::fmt::Display) -> ApiError {
    ApiError::internal(e, "Не удалось сохранить или загрузить практику")
}
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct PoolItem {
    pub task_id: String,
    pub difficulty: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Section {
    pub id: String,
    pub title: String,
    pub target: i32,
    pub items: Vec<PoolItem>,
}
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Definition {
    pub title: String,
    pub description: String,
    pub sections: Vec<Section>,
}
impl Definition {
    pub(super) fn validate(&self) -> Result<(), ApiError> {
        let mut ids = std::collections::HashSet::new();
        if self.title.trim().is_empty()
            || self.title.len() > 300
            || self.description.len() > 4000
            || self.sections.len() > 40
        {
            return Err(ApiError::BadRequest("Укажите название и не более 40 подтем".into()));
        }
        for s in &self.sections {
            if Uuid::parse_str(&s.id).is_err()
                || !ids.insert(&s.id)
                || s.title.trim().is_empty()
                || s.title.len() > 300
                || !(1..=30).contains(&s.target)
                || s.items.len() > 300
            {
                return Err(ApiError::BadRequest(
                    "Проверьте название, цель и задачи подтемы".into(),
                ));
            }
            let mut tasks = std::collections::HashSet::new();
            for i in &s.items {
                if !tasks.insert(&i.task_id)
                    || !["easy", "medium", "hard"].contains(&i.difficulty.as_str())
                {
                    return Err(ApiError::BadRequest(
                        "Повтор задачи или неизвестная сложность".into(),
                    ));
                }
            }
        }
        Ok(())
    }
}
#[derive(Deserialize, Default)]
pub(super) struct QueryOptions {
    #[serde(default)]
    pub manage: bool,
}
#[derive(Deserialize)]
pub(super) struct Save {
    pub definition: Definition,
    pub revision: Option<i32>,
}

pub(super) async fn list(
    Path(course): Path<String>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
    Query(q): Query<QueryOptions>,
) -> Result<Json<Value>, ApiError> {
    let access = require_course_membership(&state, &user, &course).await?;
    if q.manage {
        require_course_role(&state, &user, &course, CourseRole::Teacher).await?;
    }
    let rows=sqlx::query("SELECT id,draft,published,release_id,revision FROM course_trainers WHERE course_id=$1 AND ($2 OR published IS NOT NULL) ORDER BY updated_at DESC").bind(&course).bind(q.manage).fetch_all(state.db()).await.map_err(db)?;
    let mut items = Vec::new();
    for r in rows {
        let id: String = r.get("id");
        let release: Option<String> = r.get("release_id");
        let def: Value = if q.manage { r.get("draft") } else { r.get("published") };
        let progress = progress(&state, &course, &user.id, &id, release.as_deref()).await?;
        let source = trainer_source(&state, &course, &def).await?;
        let mut generation_unlock = serde_json::Map::new();
        let mut generation_progress = serde_json::Map::new();
        let studio_generation =
            repositories::trainer_sets::uses_studio_generation(state.db(), &course, &source)
                .await
                .map_err(db)?;
        let assistant =
            repositories::course_ai_assistants::find(state.db(), &course).await.map_err(db)?;
        let blueprints = assistant
            .as_ref()
            .and_then(|a| a.snapshot.pointer("/assistant/runtime_policy/generation_blueprints"))
            .and_then(Value::as_array)
            .filter(|items| !items.is_empty());
        let mut generation_levels = serde_json::Map::new();
        if studio_generation {
            for section in def["sections"].as_array().into_iter().flatten() {
                if let Some(section_id) = section["id"].as_str() {
                    if let Some(blueprints) = blueprints {
                        let mut levels: Vec<&str> = blueprints
                            .iter()
                            .filter(|b| {
                                b["topic"] == section["title"] || b["name"] == section["title"]
                            })
                            .filter_map(|b| b["difficulty"].as_str())
                            .collect();
                        levels.sort_unstable();
                        levels.dedup();
                        generation_levels.insert(section_id.to_owned(), json!(levels));
                    }
                    let unlocked = if access.roles.contains(&CourseRole::Teacher) {
                        true
                    } else {
                        let solved = repositories::trainer_sets::generation_solved_count(
                            state.db(),
                            &course,
                            &user.id,
                            &id,
                            section_id,
                        )
                        .await
                        .map_err(db)?;
                        generation_progress
                            .insert(section_id.to_owned(), json!({"solved":solved,"required":3}));
                        solved >= 3
                    };
                    generation_unlock.insert(section_id.to_owned(), json!(unlocked));
                }
            }
        }
        let mut item = json!({"id":id,"definition":def,"source":source,"published":r.get::<Option<Value>,_>("published").is_some(),"release_id":release,"revision":r.get::<i32,_>("revision"),"progress":progress,"generation_unlock":generation_unlock,"studio_generation":studio_generation});
        if blueprints.is_some() {
            item["generation_levels"] = json!(generation_levels);
        }
        if !generation_progress.is_empty() {
            item["generation_progress"] = json!(generation_progress);
        }
        items.push(item);
    }
    Ok(Json(json!({"items":items})))
}

async fn trainer_source(
    state: &AppState,
    course: &str,
    definition: &Value,
) -> Result<String, ApiError> {
    let task_ids = definition
        .get("sections")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|section| section.get("items").and_then(Value::as_array))
        .flatten()
        .filter_map(|item| item.get("task_id").and_then(Value::as_str))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if task_ids.is_empty() {
        return Ok(String::new());
    }
    sqlx::query_scalar::<_, String>(
        "SELECT s.code FROM task_bank_items i JOIN task_bank_sources s ON s.id=i.source_id JOIN course_task_bank_sources cs ON cs.source_id=s.id WHERE cs.course_id=$1 AND i.id=ANY($2) LIMIT 1",
    )
    .bind(course)
    .bind(task_ids)
    .fetch_optional(state.db())
    .await
    .map_err(db)
    .map(|value| value.unwrap_or_default())
}
pub(super) async fn progress(
    state: &AppState,
    course: &str,
    user: &str,
    id: &str,
    release: Option<&str>,
) -> Result<Value, ApiError> {
    let rows=sqlx::query("SELECT section_id,difficulty, count(DISTINCT task_id) FILTER(WHERE solved) AS solved, count(DISTINCT task_id) FILTER(WHERE jsonb_path_exists(checks, '$[*] ? (@.solved == true && @.helped == false && @.revealed == false)')) AS independent FROM practice_attempts WHERE course_id=$1 AND student_id=$2 AND trainer_id=$3 AND release_id=$4 AND NOT preview GROUP BY section_id,difficulty").bind(course).bind(user).bind(id).bind(release).fetch_all(state.db()).await.map_err(db)?;
    Ok(json!(rows.iter().map(|r|json!({"section_id":r.get::<String,_>("section_id"),"difficulty":r.get::<String,_>("difficulty"),"solved":r.get::<i64,_>("solved"),"independent":r.get::<i64,_>("independent")})).collect::<Vec<_>>()))
}
pub(super) async fn get(
    Path((course, id)): Path<(String, String)>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
    Query(q): Query<QueryOptions>,
) -> Result<Json<Value>, ApiError> {
    let Json(all) = list(Path(course), CurrentUser(user), State(state), Query(q)).await?;
    all["items"]
        .as_array()
        .and_then(|a| a.iter().find(|v| v["id"] == id))
        .cloned()
        .map(Json)
        .ok_or_else(|| ApiError::NotFound("Тренажёр не опубликован или не найден".into()))
}
pub(super) async fn save_new(
    Path(course): Path<String>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
    Json(body): Json<Save>,
) -> Result<Json<Value>, ApiError> {
    require_course_role(&state, &user, &course, CourseRole::Teacher).await?;
    body.definition.validate()?;
    let id = Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO course_trainers(id,course_id,author_id,draft) VALUES($1,$2,$3,$4)")
        .bind(&id)
        .bind(course)
        .bind(user.id)
        .bind(json!(body.definition))
        .execute(state.db())
        .await
        .map_err(db)?;
    Ok(Json(json!({"id":id,"revision":1})))
}
pub(super) async fn save(
    Path((course, id)): Path<(String, String)>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
    Json(body): Json<Save>,
) -> Result<Json<Value>, ApiError> {
    require_course_role(&state, &user, &course, CourseRole::Teacher).await?;
    body.definition.validate()?;
    let rev=sqlx::query_scalar::<_,i32>("UPDATE course_trainers SET draft=$1,revision=revision+1,updated_at=now() WHERE id=$2 AND course_id=$3 AND revision=$4 RETURNING revision").bind(json!(body.definition)).bind(&id).bind(course).bind(body.revision).fetch_optional(state.db()).await.map_err(db)?.ok_or_else(||ApiError::Conflict("Тренажёр изменён. Обновите страницу перед сохранением".into()))?;
    Ok(Json(json!({"id":id,"revision":rev})))
}
pub(super) async fn publish(
    Path((course, id)): Path<(String, String)>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
    Json(body): Json<Save>,
) -> Result<Json<Value>, ApiError> {
    require_course_role(&state, &user, &course, CourseRole::Teacher).await?;
    let mut tx = state.db().begin().await.map_err(db)?;
    let row = sqlx::query(
        "SELECT draft,published,release_id,revision FROM course_trainers WHERE id=$1 AND course_id=$2 FOR UPDATE",
    )
    .bind(&id)
    .bind(&course)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db)?
    .ok_or_else(|| ApiError::NotFound("Тренажёр не найден".into()))?;
    if body.revision != Some(row.get("revision")) {
        return Err(ApiError::Conflict("Сначала сохраните актуальную версию".into()));
    }
    if row.get::<Option<Value>, _>("published").as_ref() == Some(&row.get::<Value, _>("draft")) {
        return Ok(Json(json!({"release_id":row.get::<Option<String>,_>("release_id")})));
    }
    let def: Definition = serde_json::from_value(row.get("draft")).map_err(db)?;
    def.validate()?;
    if def.sections.is_empty() || def.sections.iter().any(|s| s.items.is_empty()) {
        return Err(ApiError::BadRequest("Добавьте задачи в каждую подтему".into()));
    }
    let ids = def
        .sections
        .iter()
        .flat_map(|s| s.items.iter().map(|i| i.task_id.clone()))
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let tasks = repositories::task_bank::list_items_with_source_by_ids_for_course(
        state.db(),
        &course,
        &ids,
    )
    .await
    .map_err(db)?;
    if tasks.len() != ids.len()
        || tasks.iter().any(|t| t.solution.as_deref().is_none_or(|s| s.trim().is_empty()))
    {
        return Err(ApiError::BadRequest(
            "Для публикации нужны задачи этого курса с полными эталонными решениями".into(),
        ));
    }
    let snapshot = super::sessions::snapshot(&state, &course).await?;
    crate::services::ai_grading::validate_grading_snapshot(
        &snapshot,
        &crate::services::ai_grading::snapshot_decision_model(
            &snapshot,
            &state.settings().ai().assistant_model,
        ),
    )
    .map_err(|_| {
        ApiError::BadRequest("Опубликуйте в Studio ассистента с проверкой решений".into())
    })?;
    crate::services::ai_grading::bank_rubric(&snapshot).map_err(|_| {
        ApiError::BadRequest("Заполните критерии проверки ассистента в Studio".into())
    })?;
    let release = Uuid::new_v4().to_string();
    sqlx::query("UPDATE course_trainers SET published=draft,release_id=$1,revision=revision+1,updated_at=now() WHERE id=$2").bind(&release).bind(&id).execute(&mut *tx).await.map_err(db)?;
    tx.commit().await.map_err(db)?;
    Ok(Json(json!({"release_id":release})))
}
pub(super) async fn unpublish(
    Path((course, id)): Path<(String, String)>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
) -> Result<Json<Value>, ApiError> {
    require_course_role(&state, &user, &course, CourseRole::Teacher).await?;
    sqlx::query("UPDATE course_trainers SET published=NULL,revision=revision+1,updated_at=now() WHERE id=$1 AND course_id=$2").bind(id).bind(course).execute(state.db()).await.map_err(db)?;
    Ok(Json(json!({"ok":true})))
}
