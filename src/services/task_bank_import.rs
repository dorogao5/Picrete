use anyhow::{anyhow, Context};
use serde::Deserialize;

use crate::core::config::Settings;
use crate::core::time::primitive_now_utc;
use crate::repositories;
use crate::services::materials;

const SVIRIDOV_SOURCE_ID: &str = "tb_source_sviridov";
const SVIRIDOV_SOURCE_CODE: &str = "sviridov";
const SVIRIDOV_SOURCE_TITLE: &str = "Задачник Свиридова";
const SVIRIDOV_SOURCE_VERSION: &str = "v1";

#[derive(Debug, Clone)]
pub(crate) struct ImportSummary {
    pub(crate) source_code: String,
    pub(crate) imported_items: usize,
    pub(crate) imported_images: usize,
}

#[derive(Debug, Deserialize)]
struct RawParagraph {
    paragraph: String,
    topic: String,
    #[serde(default)]
    theory_text: String,
    #[serde(default)]
    tasks: Vec<RawTask>,
}

#[derive(Debug, Deserialize)]
struct RawTask {
    number: String,
    text: String,
    #[serde(default)]
    images: Vec<String>,
    #[serde(default)]
    answer: String,
}

pub(crate) async fn import_sviridov(
    pool: &sqlx::PgPool,
    settings: &Settings,
) -> anyhow::Result<ImportSummary> {
    let json_path =
        materials::task_bank_json_path(settings).context("TASK_BANK_ROOT is invalid")?;
    let raw = tokio::fs::read_to_string(&json_path)
        .await
        .with_context(|| format!("failed to read task bank json: {}", json_path.display()))?;
    let paragraphs: Vec<RawParagraph> =
        serde_json::from_str(&raw).context("task bank json has invalid format")?;

    let now = primitive_now_utc();
    let mut tx = pool.begin().await.context("failed to begin import transaction")?;

    repositories::task_bank::upsert_source(
        &mut *tx,
        repositories::task_bank::UpsertSource {
            id: SVIRIDOV_SOURCE_ID,
            code: SVIRIDOV_SOURCE_CODE,
            title: SVIRIDOV_SOURCE_TITLE,
            version: SVIRIDOV_SOURCE_VERSION,
            is_active: true,
            now,
        },
    )
    .await
    .context("failed to upsert task bank source")?;

    let mut imported_items = 0usize;
    let mut imported_images = 0usize;

    for paragraph in paragraphs {
        let paragraph_value = paragraph.paragraph.trim();
        let topic_value = paragraph.topic.trim();
        let theory_text = paragraph.theory_text.clone();
        if paragraph_value.is_empty() {
            return Err(anyhow!("paragraph is empty in source file"));
        }
        if topic_value.is_empty() {
            return Err(anyhow!("topic is empty for paragraph {paragraph_value}"));
        }

        for task in paragraph.tasks {
            let number = task.number.trim();
            let text = task.text.trim();
            if number.is_empty() {
                return Err(anyhow!("task number is empty in paragraph {}", paragraph_value));
            }
            if text.is_empty() {
                return Err(anyhow!("task text is empty for number {number}"));
            }

            let normalized_answer = normalize_optional_text(&task.answer);
            let has_answer = normalized_answer.is_some();
            let item_id = stable_item_id(number);

            let item = repositories::task_bank::upsert_item(
                &mut *tx,
                repositories::task_bank::UpsertItem {
                    id: &item_id,
                    source_id: SVIRIDOV_SOURCE_ID,
                    number,
                    paragraph: paragraph_value,
                    topic: topic_value,
                    text,
                    answer: normalized_answer.as_deref(),
                    has_answer,
                    metadata: serde_json::json!({
                        "theory_text": theory_text.clone(),
                    }),
                    now,
                },
            )
            .await
            .with_context(|| format!("failed to upsert task bank item {number}"))?;

            let mut images = Vec::new();
            for (order_index, raw_path) in task.images.iter().enumerate() {
                let normalized_relative_path = materials::normalize_sviridov_image_path(raw_path)
                    .with_context(|| {
                    format!("invalid image path for task {number}: {raw_path}")
                })?;
                let resolved =
                    materials::resolve_task_bank_media_path(settings, &normalized_relative_path)
                        .with_context(|| {
                            format!(
                        "image file is missing for task {number}: {normalized_relative_path}"
                    )
                        })?;

                images.push(repositories::task_bank::CreateItemImage {
                    id: stable_image_id(number, order_index),
                    task_bank_item_id: item.id.clone(),
                    relative_path: normalized_relative_path,
                    order_index: order_index as i32,
                    mime_type: materials::guess_mime(&resolved).to_string(),
                    created_at: now,
                });
                imported_images += 1;
            }

            repositories::task_bank::replace_item_images(&mut tx, &item.id, &images)
                .await
                .with_context(|| format!("failed to upsert images for task {number}"))?;
            imported_items += 1;
        }
    }

    tx.commit().await.context("failed to commit task bank import transaction")?;

    metrics::counter!("task_bank_import_items_total", "source" => SVIRIDOV_SOURCE_CODE.to_string())
        .increment(imported_items as u64);

    Ok(ImportSummary {
        source_code: SVIRIDOV_SOURCE_CODE.to_string(),
        imported_items,
        imported_images,
    })
}

fn normalize_optional_text(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn stable_item_id(number: &str) -> String {
    format!("tb_sviridov_{}", sanitize_id_fragment(number))
}

fn stable_image_id(number: &str, order_index: usize) -> String {
    format!("tbimg_sviridov_{}_{}", sanitize_id_fragment(number), order_index)
}

fn sanitize_id_fragment(raw: &str) -> String {
    raw.chars().map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' }).collect()
}
