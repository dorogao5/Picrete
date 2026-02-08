use uuid::Uuid;

use crate::api::errors::ApiError;
use crate::core::time::primitive_now_utc;
use crate::db::models::{Exam, User};
use crate::db::types::UserRole;
use crate::repositories;
use crate::schemas::exam::{
    format_primitive, ExamResponse, TaskTypeCreate, TaskTypeResponse, TaskVariantCreate,
    TaskVariantResponse,
};

pub(super) async fn insert_task_types(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    exam_id: &str,
    task_types: Vec<TaskTypeCreate>,
) -> Result<Vec<TaskTypeResponse>, ApiError> {
    let mut responses = Vec::new();
    let now = primitive_now_utc();

    for task_type in task_types {
        let task_type_id = Uuid::new_v4().to_string();

        repositories::task_types::create(
            &mut **tx,
            repositories::task_types::CreateTaskType {
                id: &task_type_id,
                exam_id,
                title: &task_type.title,
                description: &task_type.description,
                order_index: task_type.order_index,
                max_score: task_type.max_score,
                rubric: task_type.rubric.clone(),
                difficulty: task_type.difficulty,
                taxonomy_tags: task_type.taxonomy_tags.clone(),
                formulas: task_type.formulas.clone(),
                units: task_type.units.clone(),
                validation_rules: task_type.validation_rules.clone(),
                created_at: now,
                updated_at: now,
            },
        )
        .await
        .map_err(|e| ApiError::internal(e, "Failed to create task type"))?;

        let variants = insert_variants(tx, &task_type_id, task_type.variants).await?;

        responses.push(TaskTypeResponse {
            id: task_type_id,
            exam_id: exam_id.to_string(),
            title: task_type.title,
            description: task_type.description,
            order_index: task_type.order_index,
            max_score: task_type.max_score,
            rubric: task_type.rubric,
            difficulty: task_type.difficulty,
            taxonomy_tags: task_type.taxonomy_tags,
            formulas: task_type.formulas,
            units: task_type.units,
            validation_rules: task_type.validation_rules,
            created_at: format_primitive(now),
            updated_at: format_primitive(now),
            variants,
        });
    }

    Ok(responses)
}

pub(super) async fn insert_variants(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    task_type_id: &str,
    variants: Vec<TaskVariantCreate>,
) -> Result<Vec<TaskVariantResponse>, ApiError> {
    let mut responses = Vec::new();
    let now = primitive_now_utc();

    for variant in variants {
        let variant_id = Uuid::new_v4().to_string();

        repositories::task_types::create_variant(
            &mut **tx,
            repositories::task_types::CreateTaskVariant {
                id: &variant_id,
                task_type_id,
                content: &variant.content,
                parameters: variant.parameters.clone(),
                reference_solution: variant.reference_solution.clone(),
                reference_answer: variant.reference_answer.clone(),
                answer_tolerance: variant.answer_tolerance,
                attachments: variant.attachments.clone(),
                created_at: now,
            },
        )
        .await
        .map_err(|e| ApiError::internal(e, "Failed to create task variant"))?;

        responses.push(TaskVariantResponse {
            id: variant_id,
            task_type_id: task_type_id.to_string(),
            content: variant.content,
            parameters: variant.parameters,
            reference_solution: variant.reference_solution,
            reference_answer: variant.reference_answer,
            answer_tolerance: variant.answer_tolerance,
            attachments: variant.attachments,
            created_at: format_primitive(now),
        });
    }

    Ok(responses)
}

pub(super) async fn fetch_task_types(
    pool: &sqlx::PgPool,
    exam_id: &str,
) -> Result<Vec<TaskTypeResponse>, ApiError> {
    let task_types = repositories::task_types::list_by_exam(pool, exam_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch task types"))?;

    let mut responses = Vec::new();
    for task_type in task_types {
        let variants = repositories::task_types::list_variants(pool, &task_type.id)
            .await
            .map_err(|e| ApiError::internal(e, "Failed to fetch variants"))?;

        let variant_responses = variants
            .into_iter()
            .map(|variant| TaskVariantResponse {
                id: variant.id,
                task_type_id: variant.task_type_id,
                content: variant.content,
                parameters: variant.parameters.0,
                reference_solution: variant.reference_solution,
                reference_answer: variant.reference_answer,
                answer_tolerance: variant.answer_tolerance,
                attachments: variant.attachments.0,
                created_at: format_primitive(variant.created_at),
            })
            .collect();

        responses.push(TaskTypeResponse {
            id: task_type.id,
            exam_id: task_type.exam_id,
            title: task_type.title,
            description: task_type.description,
            order_index: task_type.order_index,
            max_score: task_type.max_score,
            rubric: task_type.rubric.0,
            difficulty: task_type.difficulty,
            taxonomy_tags: task_type.taxonomy_tags.0,
            formulas: task_type.formulas.0,
            units: task_type.units.0,
            validation_rules: task_type.validation_rules.0,
            created_at: format_primitive(task_type.created_at),
            updated_at: format_primitive(task_type.updated_at),
            variants: variant_responses,
        });
    }

    Ok(responses)
}

pub(super) fn exam_to_response(exam: Exam, task_types: Vec<TaskTypeResponse>) -> ExamResponse {
    ExamResponse {
        id: exam.id,
        title: exam.title,
        description: exam.description,
        start_time: format_primitive(exam.start_time),
        end_time: format_primitive(exam.end_time),
        duration_minutes: exam.duration_minutes,
        timezone: exam.timezone,
        max_attempts: exam.max_attempts,
        allow_breaks: exam.allow_breaks,
        break_duration_minutes: exam.break_duration_minutes,
        auto_save_interval: exam.auto_save_interval,
        settings: exam.settings.0,
        status: exam.status,
        created_by: exam.created_by,
        created_at: format_primitive(exam.created_at),
        updated_at: format_primitive(exam.updated_at),
        published_at: exam.published_at.map(format_primitive),
        task_types,
    }
}

pub(super) fn can_manage_exam(user: &User, exam: &Exam) -> bool {
    matches!(user.role, UserRole::Admin) || exam.created_by.as_deref() == Some(user.id.as_str())
}
