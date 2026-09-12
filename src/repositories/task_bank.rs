use sqlx::types::Json as SqlxJson;
use sqlx::{PgPool, Postgres, QueryBuilder};
use time::PrimitiveDateTime;

use crate::db::models::{TaskBankItem, TaskBankItemImage, TaskBankSource};

pub(crate) const SOURCE_COLUMNS: &str =
    "id, code, title, version, is_active, created_at, updated_at";
const ALIASED_SOURCE_COLUMNS: &str =
    "s.id, s.code, s.title, s.version, s.is_active, s.created_at, s.updated_at";
pub(crate) const ITEM_COLUMNS: &str = "\
    id, source_id, number, paragraph, topic, text, answer, has_answer, solution, task_type, difficulty, volume, metadata, created_at, \
    updated_at";
pub(crate) const IMAGE_COLUMNS: &str =
    "id, task_bank_item_id, relative_path, order_index, mime_type, created_at";

#[derive(Debug, sqlx::FromRow)]
pub(crate) struct TaskBankItemListRow {
    pub(crate) solution: Option<String>,
    pub(crate) task_type: Option<String>,
    pub(crate) difficulty: Option<String>,
    pub(crate) volume: Option<String>,

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
    pub(crate) solution: Option<String>,
    pub(crate) task_type: Option<String>,
    pub(crate) difficulty: Option<String>,
    pub(crate) volume: Option<String>,

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
    pub(crate) solution: Option<&'a str>,
    pub(crate) task_type: Option<&'a str>,
    pub(crate) difficulty: Option<&'a str>,
    pub(crate) volume: Option<&'a str>,

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
            created_at, updated_at, solution, task_type, difficulty, volume
         ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)
         ON CONFLICT (source_id, number) DO UPDATE SET
            paragraph = EXCLUDED.paragraph,
            topic = EXCLUDED.topic,
            text = EXCLUDED.text,
            answer = EXCLUDED.answer,
            has_answer = EXCLUDED.has_answer,
            metadata = task_bank_items.metadata || EXCLUDED.metadata,
            solution = COALESCE(EXCLUDED.solution, task_bank_items.solution),
            task_type = COALESCE(EXCLUDED.task_type, task_bank_items.task_type),
            difficulty = COALESCE(EXCLUDED.difficulty, task_bank_items.difficulty),
            volume = COALESCE(EXCLUDED.volume, task_bank_items.volume),
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
    .bind(params.solution)
    .bind(params.task_type)
    .bind(params.difficulty)
    .bind(params.volume)
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

pub(crate) async fn list_sources_for_course(
    pool: &PgPool,
    course_id: &str,
) -> Result<Vec<TaskBankSource>, sqlx::Error> {
    sqlx::query_as::<_, TaskBankSource>(&format!(
        "SELECT {ALIASED_SOURCE_COLUMNS}
         FROM task_bank_sources s
         JOIN course_task_bank_sources cs ON cs.source_id = s.id
         WHERE cs.course_id = $1 AND s.is_active = TRUE
         ORDER BY s.code"
    ))
    .bind(course_id)
    .fetch_all(pool)
    .await
}

pub(crate) async fn find_source_by_code_for_course(
    pool: &PgPool,
    course_id: &str,
    code: &str,
) -> Result<Option<TaskBankSource>, sqlx::Error> {
    sqlx::query_as::<_, TaskBankSource>(&format!(
        "SELECT {ALIASED_SOURCE_COLUMNS}
         FROM task_bank_sources s
         JOIN course_task_bank_sources cs ON cs.source_id = s.id
         WHERE cs.course_id = $1 AND s.code = $2 AND s.is_active = TRUE"
    ))
    .bind(course_id)
    .bind(code)
    .fetch_optional(pool)
    .await
}

pub(crate) async fn map_sviridov_to_matching_courses(
    executor: impl sqlx::PgExecutor<'_>,
    source_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO course_task_bank_sources (course_id, source_id)
         SELECT id, $1 FROM courses
         WHERE LOWER(title) ~ '(неорган|общая.{0,20}хим|infochem)'
         ON CONFLICT DO NOTHING",
    )
    .bind(source_id)
    .execute(executor)
    .await?;
    Ok(())
}

pub(crate) async fn sync_default_sources_for_course(
    pool: &PgPool,
    course_id: &str,
    course_title: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "DELETE FROM course_task_bank_sources cs
         USING task_bank_sources s
         WHERE cs.course_id = $1 AND cs.source_id = s.id AND s.code = 'sviridov'",
    )
    .bind(course_id)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO course_task_bank_sources (course_id, source_id)
         SELECT $1, id FROM task_bank_sources
         WHERE code = 'sviridov' AND LOWER($2) ~ '(неорган|общая.{0,20}хим|infochem)'
         ON CONFLICT DO NOTHING",
    )
    .bind(course_id)
    .bind(course_title)
    .execute(pool)
    .await?;
    Ok(())
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

#[derive(Clone)]
pub(crate) struct ListItemsParams {
    pub(crate) filters: crate::schemas::trainer::TrainerFilters,
    pub(crate) course_id: String,
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
                i.solution, i.task_type, i.difficulty, i.volume,
                i.answer,
                i.has_answer,
                COUNT(*) OVER() AS total_count
         FROM task_bank_items i
         JOIN task_bank_sources s ON s.id = i.source_id
         JOIN course_task_bank_sources cs ON cs.source_id = s.id
         WHERE s.is_active = TRUE AND cs.course_id = ",
    );
    builder.push_bind(params.course_id);

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

    push_extended_filters(&mut builder, &params.filters, "i.");

    // Generated task numbers contain batch IDs; their extracted digits can
    // exceed even bigint. Numeric preserves the existing chapter/item ordering
    // without restricting these IDs to a machine integer.
    builder.push(
        " ORDER BY
            COALESCE(NULLIF(regexp_replace(split_part(i.number, '.', 1), '[^0-9]', '', 'g'), '')::numeric, 2147483647),
            COALESCE(NULLIF(regexp_replace(split_part(i.number, '.', 2), '[^0-9]', '', 'g'), '')::numeric, 2147483647),
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
                i.solution, i.task_type, i.difficulty, i.volume,
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

pub(crate) async fn list_items_with_source_by_ids_for_course(
    pool: &PgPool,
    course_id: &str,
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
                i.solution, i.task_type, i.difficulty, i.volume,
                i.answer,
                i.has_answer
         FROM task_bank_items i
         JOIN task_bank_sources s ON s.id = i.source_id
         JOIN course_task_bank_sources cs ON cs.source_id = s.id
         WHERE cs.course_id = $1 AND i.id = ANY($2)
         ORDER BY array_position($2::text[], i.id)",
    )
    .bind(course_id)
    .bind(item_ids)
    .fetch_all(pool)
    .await
}

pub(crate) async fn course_has_item(
    pool: &PgPool,
    course_id: &str,
    item_id: &str,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1
            FROM task_bank_items i
            JOIN course_task_bank_sources cs ON cs.source_id = i.source_id
            WHERE cs.course_id = $1 AND i.id = $2
         )",
    )
    .bind(course_id)
    .bind(item_id)
    .fetch_one(pool)
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
    pub(crate) filters: crate::schemas::trainer::TrainerFilters,
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

    push_extended_filters(&mut builder, &params.filters, "");
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

    push_extended_filters(&mut builder, &params.filters, "");
    builder.push(" ORDER BY id");
    builder.push(" LIMIT ");
    builder.push_bind(limit.clamp(1, 50_000));

    builder.build_query_scalar::<String>().fetch_all(pool).await
}

fn push_extended_filters<'a>(
    builder: &mut QueryBuilder<'a, Postgres>,
    filters: &'a crate::schemas::trainer::TrainerFilters,
    prefix: &str,
) {
    for (column, value) in [
        ("task_type", &filters.task_type),
        ("difficulty", &filters.difficulty),
        ("volume", &filters.volume),
    ] {
        if let Some(value) = value.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
            builder.push(format!(" AND {prefix}{column} = ")).push_bind(value);
        }
    }
    if let Some(has_solution) = filters.has_solution {
        builder.push(format!(" AND ({prefix}solution IS NOT NULL) = ")).push_bind(has_solution);
    }
    if let Some(q) = filters.q.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        if q.split_once('.').is_some_and(|(a, b)| {
            !a.is_empty()
                && !b.is_empty()
                && a.bytes().all(|v| v.is_ascii_digit())
                && b.bytes().all(|v| v.is_ascii_digit())
        }) {
            builder.push(format!(" AND {prefix}number = ")).push_bind(q);
            return;
        }
        let escaped = q.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
        builder.push(format!(" AND ({prefix}number ILIKE ")).push_bind(format!("%{escaped}%"));
        builder.push(format!(" OR {prefix}text ILIKE ")).push_bind(format!("%{escaped}%"));
        builder.push(format!(" OR {prefix}topic ILIKE ")).push_bind(format!("%{escaped}%"));
        builder.push(")");
    }
}

#[derive(serde::Serialize, sqlx::FromRow)]
pub(crate) struct TaskBankFacets {
    paragraphs: Vec<String>,
    topics: Vec<String>,
    task_types: Vec<String>,
    difficulties: Vec<String>,
    volumes: Vec<String>,
}

pub(crate) async fn facets(
    pool: &PgPool,
    course_id: &str,
    source: Option<&str>,
) -> Result<TaskBankFacets, sqlx::Error> {
    sqlx::query_as(
        "SELECT
            COALESCE(array_agg(DISTINCT i.paragraph ORDER BY i.paragraph), '{}'::text[]) AS paragraphs,
            COALESCE(array_agg(DISTINCT i.topic ORDER BY i.topic), '{}'::text[]) AS topics,
            COALESCE(array_agg(DISTINCT i.task_type ORDER BY i.task_type) FILTER (WHERE i.task_type IS NOT NULL), '{}'::text[]) AS task_types,
            COALESCE(array_agg(DISTINCT i.difficulty ORDER BY i.difficulty) FILTER (WHERE i.difficulty IS NOT NULL), '{}'::text[]) AS difficulties,
            COALESCE(array_agg(DISTINCT i.volume ORDER BY i.volume) FILTER (WHERE i.volume IS NOT NULL), '{}'::text[]) AS volumes
         FROM task_bank_items i
         JOIN task_bank_sources s ON s.id = i.source_id AND s.is_active
         JOIN course_task_bank_sources cs ON cs.source_id = s.id
         WHERE cs.course_id = $1 AND ($2::text IS NULL OR s.code = $2)"
    ).bind(course_id).bind(source).fetch_one(pool).await
}
