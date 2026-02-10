use sqlx::types::Json as SqlxJson;
use sqlx::{PgPool, Postgres, QueryBuilder};
use time::PrimitiveDateTime;

use crate::db::models::{TaskBankItem, TaskBankItemImage, TaskBankSource};

pub(crate) const SOURCE_COLUMNS: &str =
    "id, code, title, version, is_active, created_at, updated_at";
pub(crate) const ITEM_COLUMNS: &str = "\
    id, source_id, number, paragraph, topic, text, answer, has_answer, metadata, created_at, \
    updated_at";
pub(crate) const IMAGE_COLUMNS: &str =
    "id, task_bank_item_id, relative_path, order_index, mime_type, created_at";

#[derive(Debug, sqlx::FromRow)]
pub(crate) struct TaskBankItemListRow {
    pub(crate) id: String,
    pub(crate) source_code: String,
    pub(crate) number: String,
    pub(crate) paragraph: String,
    pub(crate) topic: String,
    pub(crate) text: String,
    pub(crate) answer: Option<String>,
    pub(crate) has_answer: bool,
    pub(crate) total_count: i64,
}

#[derive(Debug, sqlx::FromRow)]
pub(crate) struct TaskBankItemWithSourceRow {
    pub(crate) id: String,
    pub(crate) source_code: String,
    pub(crate) source_title: String,
    pub(crate) number: String,
    pub(crate) paragraph: String,
    pub(crate) topic: String,
    pub(crate) text: String,
    pub(crate) answer: Option<String>,
    pub(crate) has_answer: bool,
}

pub(crate) struct UpsertSource<'a> {
    pub(crate) id: &'a str,
    pub(crate) code: &'a str,
    pub(crate) title: &'a str,
    pub(crate) version: &'a str,
    pub(crate) is_active: bool,
    pub(crate) now: PrimitiveDateTime,
}

pub(crate) async fn upsert_source(
    executor: impl sqlx::PgExecutor<'_>,
    params: UpsertSource<'_>,
) -> Result<TaskBankSource, sqlx::Error> {
    sqlx::query_as::<_, TaskBankSource>(&format!(
        "INSERT INTO task_bank_sources (
            id, code, title, version, is_active, created_at, updated_at
         ) VALUES ($1,$2,$3,$4,$5,$6,$7)
         ON CONFLICT (code) DO UPDATE SET
            title = EXCLUDED.title,
            version = EXCLUDED.version,
            is_active = EXCLUDED.is_active,
            updated_at = EXCLUDED.updated_at
         RETURNING {SOURCE_COLUMNS}"
    ))
    .bind(params.id)
    .bind(params.code)
    .bind(params.title)
    .bind(params.version)
    .bind(params.is_active)
    .bind(params.now)
    .bind(params.now)
    .fetch_one(executor)
    .await
}

pub(crate) struct UpsertItem<'a> {
    pub(crate) id: &'a str,
    pub(crate) source_id: &'a str,
    pub(crate) number: &'a str,
    pub(crate) paragraph: &'a str,
    pub(crate) topic: &'a str,
    pub(crate) text: &'a str,
    pub(crate) answer: Option<&'a str>,
    pub(crate) has_answer: bool,
    pub(crate) metadata: serde_json::Value,
    pub(crate) now: PrimitiveDateTime,
}

pub(crate) async fn upsert_item(
    executor: impl sqlx::PgExecutor<'_>,
    params: UpsertItem<'_>,
) -> Result<TaskBankItem, sqlx::Error> {
    sqlx::query_as::<_, TaskBankItem>(&format!(
        "INSERT INTO task_bank_items (
            id, source_id, number, paragraph, topic, text, answer, has_answer, metadata,
            created_at, updated_at
         ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)
         ON CONFLICT (source_id, number) DO UPDATE SET
            paragraph = EXCLUDED.paragraph,
            topic = EXCLUDED.topic,
            text = EXCLUDED.text,
            answer = EXCLUDED.answer,
            has_answer = EXCLUDED.has_answer,
            metadata = EXCLUDED.metadata,
            updated_at = EXCLUDED.updated_at
         RETURNING {ITEM_COLUMNS}"
    ))
    .bind(params.id)
    .bind(params.source_id)
    .bind(params.number)
    .bind(params.paragraph)
    .bind(params.topic)
    .bind(params.text)
    .bind(params.answer)
    .bind(params.has_answer)
    .bind(SqlxJson(params.metadata))
    .bind(params.now)
    .bind(params.now)
    .fetch_one(executor)
    .await
}

pub(crate) struct CreateItemImage {
    pub(crate) id: String,
    pub(crate) task_bank_item_id: String,
    pub(crate) relative_path: String,
    pub(crate) order_index: i32,
    pub(crate) mime_type: String,
    pub(crate) created_at: PrimitiveDateTime,
}

pub(crate) async fn replace_item_images(
    executor: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    task_bank_item_id: &str,
    images: &[CreateItemImage],
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM task_bank_item_images WHERE task_bank_item_id = $1")
        .bind(task_bank_item_id)
        .execute(&mut **executor)
        .await?;

    for image in images {
        sqlx::query(
            "INSERT INTO task_bank_item_images (
                id, task_bank_item_id, relative_path, order_index, mime_type, created_at
             ) VALUES ($1,$2,$3,$4,$5,$6)",
        )
        .bind(&image.id)
        .bind(&image.task_bank_item_id)
        .bind(&image.relative_path)
        .bind(image.order_index)
        .bind(&image.mime_type)
        .bind(image.created_at)
        .execute(&mut **executor)
        .await?;
    }

    Ok(())
}

pub(crate) async fn list_sources(pool: &PgPool) -> Result<Vec<TaskBankSource>, sqlx::Error> {
    sqlx::query_as::<_, TaskBankSource>(&format!(
        "SELECT {SOURCE_COLUMNS}
         FROM task_bank_sources
         WHERE is_active = TRUE
         ORDER BY code"
    ))
    .fetch_all(pool)
    .await
}

pub(crate) async fn find_source_by_code(
    pool: &PgPool,
    code: &str,
) -> Result<Option<TaskBankSource>, sqlx::Error> {
    sqlx::query_as::<_, TaskBankSource>(&format!(
        "SELECT {SOURCE_COLUMNS}
         FROM task_bank_sources
         WHERE code = $1 AND is_active = TRUE"
    ))
    .bind(code)
    .fetch_optional(pool)
    .await
}

pub(crate) async fn find_source_by_id(
    pool: &PgPool,
    id: &str,
) -> Result<Option<TaskBankSource>, sqlx::Error> {
    sqlx::query_as::<_, TaskBankSource>(&format!(
        "SELECT {SOURCE_COLUMNS}
         FROM task_bank_sources
         WHERE id = $1 AND is_active = TRUE"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub(crate) struct ListItemsParams {
    pub(crate) source_code: Option<String>,
    pub(crate) paragraph: Option<String>,
    pub(crate) topic: Option<String>,
    pub(crate) has_answer: Option<bool>,
    pub(crate) skip: i64,
    pub(crate) limit: i64,
}

pub(crate) async fn list_items(
    pool: &PgPool,
    params: ListItemsParams,
) -> Result<Vec<TaskBankItemListRow>, sqlx::Error> {
    let mut builder = QueryBuilder::<Postgres>::new(
        "SELECT i.id,
                s.code AS source_code,
                i.number,
                i.paragraph,
                i.topic,
                i.text,
                i.answer,
                i.has_answer,
                COUNT(*) OVER() AS total_count
         FROM task_bank_items i
         JOIN task_bank_sources s ON s.id = i.source_id
         WHERE s.is_active = TRUE",
    );

    if let Some(source_code) = params.source_code {
        builder.push(" AND s.code = ");
        builder.push_bind(source_code);
    }
    if let Some(paragraph) = params.paragraph {
        builder.push(" AND i.paragraph = ");
        builder.push_bind(paragraph);
    }
    if let Some(topic) = params.topic {
        builder.push(" AND i.topic ILIKE ");
        builder.push_bind(format!("%{}%", topic));
    }
    if let Some(has_answer) = params.has_answer {
        builder.push(" AND i.has_answer = ");
        builder.push_bind(has_answer);
    }

    builder.push(
        " ORDER BY
            COALESCE(NULLIF(regexp_replace(split_part(i.number, '.', 1), '[^0-9]', '', 'g'), '')::int, 2147483647),
            COALESCE(NULLIF(regexp_replace(split_part(i.number, '.', 2), '[^0-9]', '', 'g'), '')::int, 2147483647),
            i.number",
    );
    builder.push(" OFFSET ");
    builder.push_bind(params.skip.max(0));
    builder.push(" LIMIT ");
    builder.push_bind(params.limit.clamp(1, 1000));

    builder.build_query_as::<TaskBankItemListRow>().fetch_all(pool).await
}

pub(crate) async fn list_item_images_by_item_ids(
    pool: &PgPool,
    item_ids: &[String],
) -> Result<Vec<TaskBankItemImage>, sqlx::Error> {
    if item_ids.is_empty() {
        return Ok(Vec::new());
    }

    sqlx::query_as::<_, TaskBankItemImage>(&format!(
        "SELECT {IMAGE_COLUMNS}
         FROM task_bank_item_images
         WHERE task_bank_item_id = ANY($1)
         ORDER BY task_bank_item_id, order_index"
    ))
    .bind(item_ids)
    .fetch_all(pool)
    .await
}

pub(crate) async fn find_item_image(
    pool: &PgPool,
    item_id: &str,
    image_id: &str,
) -> Result<Option<TaskBankItemImage>, sqlx::Error> {
    sqlx::query_as::<_, TaskBankItemImage>(&format!(
        "SELECT {IMAGE_COLUMNS}
         FROM task_bank_item_images
         WHERE task_bank_item_id = $1 AND id = $2"
    ))
    .bind(item_id)
    .bind(image_id)
    .fetch_optional(pool)
    .await
}

pub(crate) async fn list_items_with_source_by_ids(
    pool: &PgPool,
    item_ids: &[String],
) -> Result<Vec<TaskBankItemWithSourceRow>, sqlx::Error> {
    if item_ids.is_empty() {
        return Ok(Vec::new());
    }

    sqlx::query_as::<_, TaskBankItemWithSourceRow>(
        "SELECT i.id,
                s.code AS source_code,
                s.title AS source_title,
                i.number,
                i.paragraph,
                i.topic,
                i.text,
                i.answer,
                i.has_answer
         FROM task_bank_items i
         JOIN task_bank_sources s ON s.id = i.source_id
         WHERE i.id = ANY($1)
         ORDER BY array_position($1::text[], i.id)",
    )
    .bind(item_ids)
    .fetch_all(pool)
    .await
}

pub(crate) async fn list_items_by_numbers(
    pool: &PgPool,
    source_id: &str,
    numbers: &[String],
) -> Result<Vec<TaskBankItem>, sqlx::Error> {
    if numbers.is_empty() {
        return Ok(Vec::new());
    }

    sqlx::query_as::<_, TaskBankItem>(&format!(
        "SELECT {ITEM_COLUMNS}
         FROM task_bank_items
         WHERE source_id = $1 AND number = ANY($2)
         ORDER BY array_position($2::text[], number)"
    ))
    .bind(source_id)
    .bind(numbers)
    .fetch_all(pool)
    .await
}

pub(crate) struct FilterParams {
    pub(crate) source_id: String,
    pub(crate) paragraph: Option<String>,
    pub(crate) topic: Option<String>,
    pub(crate) has_answer: Option<bool>,
}

pub(crate) async fn count_items_by_filters(
    pool: &PgPool,
    params: &FilterParams,
) -> Result<i64, sqlx::Error> {
    let mut builder = QueryBuilder::<Postgres>::new(
        "SELECT COUNT(*)
         FROM task_bank_items
         WHERE source_id = ",
    );
    builder.push_bind(&params.source_id);

    if let Some(paragraph) = &params.paragraph {
        builder.push(" AND paragraph = ");
        builder.push_bind(paragraph);
    }
    if let Some(topic) = &params.topic {
        builder.push(" AND topic ILIKE ");
        builder.push_bind(format!("%{}%", topic));
    }
    if let Some(has_answer) = params.has_answer {
        builder.push(" AND has_answer = ");
        builder.push_bind(has_answer);
    }

    builder.build_query_scalar::<i64>().fetch_one(pool).await
}

pub(crate) async fn list_item_ids_by_filters(
    pool: &PgPool,
    params: &FilterParams,
    limit: i64,
) -> Result<Vec<String>, sqlx::Error> {
    let mut builder = QueryBuilder::<Postgres>::new(
        "SELECT id
         FROM task_bank_items
         WHERE source_id = ",
    );
    builder.push_bind(&params.source_id);

    if let Some(paragraph) = &params.paragraph {
        builder.push(" AND paragraph = ");
        builder.push_bind(paragraph);
    }
    if let Some(topic) = &params.topic {
        builder.push(" AND topic ILIKE ");
        builder.push_bind(format!("%{}%", topic));
    }
    if let Some(has_answer) = params.has_answer {
        builder.push(" AND has_answer = ");
        builder.push_bind(has_answer);
    }

    builder.push(" ORDER BY id");
    builder.push(" LIMIT ");
    builder.push_bind(limit.clamp(1, 50_000));

    builder.build_query_scalar::<String>().fetch_all(pool).await
}
