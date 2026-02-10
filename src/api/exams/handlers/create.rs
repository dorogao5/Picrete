use axum::Json;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;
use validator::Validate;

use crate::api::errors::ApiError;
use crate::api::guards::{require_course_role, CurrentUser};
use crate::core::state::AppState;
use crate::core::time::{primitive_now_utc, to_primitive_utc};
use crate::db::types::{CourseRole, ExamStatus};
use crate::repositories;
use crate::schemas::exam::{ExamCreate, ExamResponse, TaskTypeCreate};
use crate::schemas::task_bank::{AddBankTaskToWorkRequest, AddBankTaskToWorkResponse};
use crate::services::work_processing::WorkProcessingSettings;
use crate::services::work_timing::normalize_duration_for_kind;

use super::super::helpers;

pub(in crate::api::exams) async fn create_exam(
    axum::extract::Path(course_id): axum::extract::Path<String>,
    CurrentUser(user): CurrentUser,
    state: axum::extract::State<AppState>,
    Json(payload): Json<ExamCreate>,
) -> Result<(axum::http::StatusCode, Json<ExamResponse>), ApiError> {
    require_course_role(&state, &user, &course_id, CourseRole::Teacher).await?;

    payload.validate().map_err(|e| ApiError::BadRequest(e.to_string()))?;

    if payload.end_time <= payload.start_time {
        return Err(ApiError::BadRequest("end_time must be after start_time".to_string()));
    }

    let duration_minutes = normalize_duration_for_kind(payload.kind, payload.duration_minutes)
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;

    let start_time = to_primitive_utc(payload.start_time);
    let end_time = to_primitive_utc(payload.end_time);
    let processing = WorkProcessingSettings {
        ocr_enabled: payload.ocr_enabled,
        llm_precheck_enabled: payload.llm_precheck_enabled,
    }
    .validate()
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let settings = processing.merge_into_exam_settings(payload.settings.clone());

    let now = primitive_now_utc();
    let mut tx = state
        .db()
        .begin()
        .await
        .map_err(|e| ApiError::internal(e, "Failed to start transaction"))?;

    let exam_id = Uuid::new_v4().to_string();
    let exam = repositories::exams::create(
        &mut *tx,
        repositories::exams::CreateExam {
            id: &exam_id,
            course_id: &course_id,
            title: &payload.title,
            description: payload.description.as_deref(),
            kind: payload.kind,
            start_time,
            end_time,
            duration_minutes,
            timezone: &payload.timezone,
            max_attempts: payload.max_attempts,
            allow_breaks: payload.allow_breaks,
            break_duration_minutes: payload.break_duration_minutes,
            auto_save_interval: payload.auto_save_interval,
            status: ExamStatus::Draft,
            created_by: &user.id,
            created_at: now,
            updated_at: now,
            settings,
        },
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to create exam"))?;

    let task_types =
        helpers::insert_task_types(&mut tx, &course_id, &exam.id, payload.task_types).await?;
    tx.commit().await.map_err(|e| ApiError::internal(e, "Failed to commit transaction"))?;

    Ok((axum::http::StatusCode::CREATED, Json(helpers::exam_to_response(exam, task_types))))
}

pub(in crate::api::exams) async fn add_task_type(
    axum::extract::Path((course_id, exam_id)): axum::extract::Path<(String, String)>,
    CurrentUser(user): CurrentUser,
    state: axum::extract::State<AppState>,
    Json(payload): Json<TaskTypeCreate>,
) -> Result<(axum::http::StatusCode, Json<serde_json::Value>), ApiError> {
    require_course_role(&state, &user, &course_id, CourseRole::Teacher).await?;

    payload.validate().map_err(|e| ApiError::BadRequest(e.to_string()))?;

    let exam = repositories::exams::find_by_id(state.db(), &course_id, &exam_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch exam"))?;

    let Some(_exam) = exam else {
        return Err(ApiError::NotFound("Exam not found".to_string()));
    };

    let mut tx = state
        .db()
        .begin()
        .await
        .map_err(|e| ApiError::internal(e, "Failed to start transaction"))?;

    let now = primitive_now_utc();
    let task_type_id = Uuid::new_v4().to_string();

    let TaskTypeCreate {
        title,
        description,
        order_index,
        max_score,
        rubric,
        difficulty,
        taxonomy_tags,
        formulas,
        units,
        validation_rules,
        variants,
    } = payload;

    repositories::task_types::create(
        &mut *tx,
        repositories::task_types::CreateTaskType {
            id: &task_type_id,
            course_id: &course_id,
            exam_id: &exam_id,
            title: &title,
            description: &description,
            order_index,
            max_score,
            rubric,
            difficulty,
            taxonomy_tags,
            formulas,
            units,
            validation_rules,
            created_at: now,
            updated_at: now,
        },
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to create task type"))?;

    helpers::insert_variants(&mut tx, &course_id, &task_type_id, variants).await?;
    tx.commit().await.map_err(|e| ApiError::internal(e, "Failed to commit transaction"))?;

    Ok((
        axum::http::StatusCode::CREATED,
        Json(serde_json::json!({
            "message": "Task type added successfully",
            "task_type_id": task_type_id
        })),
    ))
}

pub(in crate::api::exams) async fn add_task_types_from_bank(
    axum::extract::Path((course_id, exam_id)): axum::extract::Path<(String, String)>,
    CurrentUser(user): CurrentUser,
    state: axum::extract::State<AppState>,
    Json(payload): Json<AddBankTaskToWorkRequest>,
) -> Result<(axum::http::StatusCode, Json<AddBankTaskToWorkResponse>), ApiError> {
    require_course_role(&state, &user, &course_id, CourseRole::Teacher).await?;
    payload.validate().map_err(|e| ApiError::BadRequest(e.to_string()))?;

    let exam = repositories::exams::find_by_id(state.db(), &course_id, &exam_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch exam"))?;
    if exam.is_none() {
        return Err(ApiError::NotFound("Exam not found".to_string()));
    }

    let mut ordered_ids = Vec::new();
    let mut seen = HashSet::new();
    for raw_id in payload.bank_item_ids {
        let id = raw_id.trim().to_string();
        if id.is_empty() {
            continue;
        }
        if seen.insert(id.clone()) {
            ordered_ids.push(id);
        }
    }
    if ordered_ids.is_empty() {
        return Err(ApiError::BadRequest("bank_item_ids must not be empty".to_string()));
    }

    let items = repositories::task_bank::list_items_with_source_by_ids(state.db(), &ordered_ids)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to load task bank items"))?;
    let by_id = items.into_iter().map(|item| (item.id.clone(), item)).collect::<HashMap<_, _>>();
    if by_id.len() != ordered_ids.len() {
        let invalid_ids = ordered_ids
            .iter()
            .filter(|item_id| !by_id.contains_key(item_id.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        return Err(ApiError::UnprocessableEntity(format!(
            "Unknown bank_item_ids: {}",
            invalid_ids.join(", ")
        )));
    }

    let images = repositories::task_bank::list_item_images_by_item_ids(state.db(), &ordered_ids)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to load task bank images"))?;
    let mut images_by_item = HashMap::<String, Vec<crate::db::models::TaskBankItemImage>>::new();
    for image in images {
        images_by_item.entry(image.task_bank_item_id.clone()).or_default().push(image);
    }

    let existing_task_types =
        repositories::task_types::list_by_exam(state.db(), &course_id, &exam_id)
            .await
            .map_err(|e| ApiError::internal(e, "Failed to load existing task types"))?;
    let mut next_order_index =
        existing_task_types.iter().map(|task_type| task_type.order_index).max().unwrap_or(-1) + 1;

    let api_prefix = state.settings().api().api_v1_str.trim_end_matches('/');
    let now = primitive_now_utc();
    let mut created_task_type_ids = Vec::with_capacity(ordered_ids.len());
    let mut tx = state
        .db()
        .begin()
        .await
        .map_err(|e| ApiError::internal(e, "Failed to start transaction"))?;

    for item_id in ordered_ids {
        let item = by_id
            .get(&item_id)
            .ok_or_else(|| ApiError::Internal("Task bank item lookup failed".to_string()))?;
        let source_code = item.source_code.clone();
        let source_title = item.source_title.clone();
        let item_number = item.number.clone();
        let item_paragraph = item.paragraph.clone();
        let item_topic = item.topic.clone();
        let item_text = item.text.clone();
        let task_type_id = Uuid::new_v4().to_string();
        let variant_id = Uuid::new_v4().to_string();

        repositories::task_types::create(
            &mut *tx,
            repositories::task_types::CreateTaskType {
                id: &task_type_id,
                course_id: &course_id,
                exam_id: &exam_id,
                title: &format!("Задача {}", item_number),
                description: &item_text,
                order_index: next_order_index,
                max_score: 1.0,
                rubric: serde_json::json!({
                    "snapshot": true,
                    "source": "task_bank",
                    "source_code": source_code.clone(),
                    "number": item_number.clone(),
                }),
                difficulty: crate::db::types::DifficultyLevel::Medium,
                taxonomy_tags: vec![item_topic.clone()],
                formulas: Vec::new(),
                units: Vec::new(),
                validation_rules: serde_json::json!({
                    "source_ref": {
                        "source_code": source_code.clone(),
                        "number": item_number.clone(),
                    }
                }),
                created_at: now,
                updated_at: now,
            },
        )
        .await
        .map_err(|e| ApiError::internal(e, "Failed to create snapshot task type"))?;

        let attachments = images_by_item
            .remove(&item.id)
            .unwrap_or_default()
            .into_iter()
            .map(|image| {
                format!(
                    "{api_prefix}/courses/{course_id}/materials/task-bank-image/{}",
                    image.relative_path
                )
            })
            .collect::<Vec<_>>();

        repositories::task_types::create_variant(
            &mut *tx,
            repositories::task_types::CreateTaskVariant {
                id: &variant_id,
                course_id: &course_id,
                task_type_id: &task_type_id,
                content: &item_text,
                parameters: serde_json::json!({
                    "snapshot": true,
                    "source": "task_bank",
                    "source_code": source_code,
                    "source_title": source_title,
                    "number": item_number,
                    "paragraph": item_paragraph,
                }),
                reference_solution: None,
                reference_answer: if item.has_answer { item.answer.clone() } else { None },
                answer_tolerance: 0.01,
                attachments,
                created_at: now,
            },
        )
        .await
        .map_err(|e| ApiError::internal(e, "Failed to create snapshot task variant"))?;

        created_task_type_ids.push(task_type_id);
        next_order_index += 1;
    }

    tx.commit().await.map_err(|e| ApiError::internal(e, "Failed to commit transaction"))?;

    Ok((
        axum::http::StatusCode::CREATED,
        Json(AddBankTaskToWorkResponse {
            created_count: created_task_type_ids.len(),
            created_task_type_ids,
        }),
    ))
}
