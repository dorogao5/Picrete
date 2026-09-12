//! Durable practice operations. No exam sessions or submissions are created here.
use crate::{
    core::state::AppState,
    services::{
        ai_grading::{bank_rubric, AiGradingService, LlmPrecheckRequest},
        assistant_chat::AssistantChatService,
        datalab_ocr::DatalabOcrService,
    },
};
use anyhow::{Context, Result};
use serde_json::{json, Value};
use sqlx::Row;
use tokio::{
    sync::watch,
    time::{sleep, timeout, Duration},
};

pub(crate) async fn run(state: AppState, mut shutdown: watch::Receiver<bool>) {
    loop {
        if *shutdown.borrow() {
            break;
        }
        if let Err(e) = tick(&state).await {
            tracing::error!(error=%e,"Practice worker failed");
        }
        tokio::select! { _=shutdown.changed()=>break, _=sleep(Duration::from_secs(1))=>{} }
    }
}
pub(crate) async fn tick(state: &AppState) -> Result<()> {
    // A bounded operation cannot still be running after this lease. A crashed job is
    // failed explicitly so students can retry without silently repeating paid calls.
    sqlx::query("UPDATE practice_jobs SET status='failed',error='Обработка прервалась. Повторите действие.' WHERE status='running' AND started_at < now()-interval '6 minutes'").execute(state.db()).await?;
    let job=sqlx::query("UPDATE practice_jobs SET status='running',started_at=now() WHERE id=(SELECT id FROM practice_jobs WHERE status='queued' ORDER BY created_at FOR UPDATE SKIP LOCKED LIMIT 1) RETURNING id,attempt_id,kind,payload,revision").fetch_optional(state.db()).await?;
    let Some(job) = job else {
        return Ok(());
    };
    let id: String = job.get("id");
    let attempt: String = job.get("attempt_id");
    let kind: String = job.get("kind");
    let payload: Value = job.get("payload");
    let revision: i32 = job.get("revision");
    let a: Value = sqlx::query_scalar("SELECT to_jsonb(a) FROM practice_attempts a WHERE id=$1")
        .bind(&attempt)
        .fetch_one(state.db())
        .await?;
    if kind == "ocr" {
        sqlx::query("INSERT INTO practice_photos(id,attempt_id,storage_key,filename) VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING").bind(payload["photo_id"].as_str()).bind(&attempt).bind(payload["storage_key"].as_str()).bind(payload["filename"].as_str()).execute(state.db()).await?;
    }
    let result = match timeout(Duration::from_secs(300), compute(state, &a, &kind, &payload)).await
    {
        Ok(r) => r,
        Err(_) => Err(anyhow::anyhow!("Practice operation timed out")),
    };
    let mut tx = state.db().begin().await?;
    let current: i32 =
        sqlx::query_scalar("SELECT revision FROM practice_attempts WHERE id=$1 FOR UPDATE")
            .bind(&attempt)
            .fetch_one(&mut *tx)
            .await?;
    match result {
        Ok(output) if current == revision => {
            let (draft, messages, checks, solved) = if kind == "ocr" {
                let text = output["text"].as_str().unwrap_or("");
                (
                    format!("{}\n\n{}", a["draft"].as_str().unwrap_or(""), text).trim().to_string(),
                    json!([{"role":"assistant","content":"Фото распознано и добавлено в черновик. Проверьте формулы перед отправкой на проверку.","kind":"ocr"}]),
                    json!([]),
                    a["solved"].as_bool().unwrap_or(false),
                )
            } else if kind == "check" {
                (
                    a["draft"].as_str().unwrap_or("").to_string(),
                    json!([{"role":"user","content":"Проверь моё решение","kind":"check"},{"role":"assistant","content":output["feedback"],"kind":"check"}]),
                    json!([output]),
                    a["solved"].as_bool().unwrap_or(false) || is_solved(&output),
                )
            } else {
                (
                    a["draft"].as_str().unwrap_or("").to_string(),
                    json!([{"role":"assistant","content":output["text"]}]),
                    json!([]),
                    a["solved"].as_bool().unwrap_or(false),
                )
            };
            sqlx::query("UPDATE practice_attempts SET draft=$1,messages=messages || $2::jsonb,checks=checks || $3::jsonb,solved=$4,revision=revision+1,updated_at=now() WHERE id=$5").bind(draft).bind(messages).bind(checks).bind(solved).bind(&attempt).execute(&mut *tx).await?;
            sqlx::query("UPDATE practice_jobs SET status='completed' WHERE id=$1")
                .bind(&id)
                .execute(&mut *tx)
                .await?;
        }
        other => {
            if let Err(e) = &other {
                tracing::warn!(job_id=%id,error=%e,"Practice operation failed");
            }
            let message = if current != revision {
                "Черновик изменился. Проверьте актуальную версию."
            } else {
                "Не удалось завершить обработку. Решение сохранено; повторите действие."
            };
            sqlx::query("UPDATE practice_jobs SET status='failed',error=$1 WHERE id=$2")
                .bind(message)
                .bind(&id)
                .execute(&mut *tx)
                .await?;
        }
    }
    tx.commit().await?;
    Ok(())
}
fn is_solved(result: &Value) -> bool {
    result["unreadable"].as_bool() == Some(false)
        && result["max_score"].as_f64().is_some_and(|max| {
            max > 0.0 && result["total_score"].as_f64().is_some_and(|score| score >= max)
        })
}
async fn compute(state: &AppState, a: &Value, kind: &str, payload: &Value) -> Result<Value> {
    let snapshot = &a["snapshot"];
    match kind {
        "ocr" => {
            let storage = state.storage().context("Storage unavailable")?;
            let url = storage
                .presign_get(
                    payload["storage_key"].as_str().context("Missing image")?,
                    Duration::from_secs(600),
                )
                .await?;
            let r = DatalabOcrService::from_settings(state.settings())?
                .run_marker_for_file_url(&url)
                .await?;
            let text =
                r.markdown.filter(|s| !s.trim().is_empty()).context("OCR returned no text")?;
            anyhow::ensure!(
                text.len() + a["draft"].as_str().unwrap_or("").len() < 60000,
                "OCR exceeds draft limit"
            );
            Ok(json!({"text":text}))
        }
        "check" => {
            let (criteria, max) = bank_rubric(snapshot)?;
            let ai = AiGradingService::for_snapshot(state.settings(), snapshot)?;
            let mut result = ai
                .run_precheck(LlmPrecheckRequest {
                    submission_id: None,
                    snapshot: Some(snapshot.clone()),
                    ocr_markdown_pages: vec![a["draft"].as_str().unwrap_or("").into()],
                    ocr_report_issues: vec![],
                    report_summary: None,
                    task_description: a["task"]["text"].as_str().context("Task missing")?.into(),
                    reference_solution: a["task"]["solution"]
                        .as_str()
                        .context("Reference missing")?
                        .into(),
                    rubric: json!({"criteria":criteria,"total_max_score":max}),
                    max_score: max,
                    chemistry_rules: Some(json!({"task_count":1,"numeric_answers":[]})),
                })
                .await?;
            result["solved"] = json!(is_solved(&result));
            result["helped"] = a["helped"].clone();
            result["revealed"] = a["revealed"].clone();
            result["revision"] = a["revision"].clone();
            result["student_draft"] = a["draft"].clone();
            // Persist the grader's own feedback. It is not independently regraded by tutor.
            if result["feedback"].as_str().is_none_or(|s| s.trim().is_empty()) {
                result["feedback"]=json!("Проверка завершена. Посмотрите критерии ниже или задайте вопрос по результату.");
            }
            Ok(result)
        }
        "message" => {
            let context = json!({"task":a["task"],"draft":a["draft"],"last_check":a["checks"].as_array().and_then(|c|c.last()),"topic":a["title"],"difficulty":a["difficulty"]});
            let history = a["messages"]
                .as_array()
                .context("History missing")?
                .iter()
                .map(|m| json!({"role":m["role"],"content":m["content"]}))
                .collect::<Vec<_>>();
            let text = AssistantChatService::from_snapshot(state.settings(), snapshot)?
                .reply_with_context(snapshot, &history, Some(&context))
                .await?;
            Ok(json!({"text":text}))
        }
        _ => anyhow::bail!("Unknown practice operation"),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn progress_requires_readable_full_credit() {
        assert!(is_solved(&json!({"unreadable":false,"total_score":5,"max_score":5})));
        for v in [
            json!({"unreadable":true,"total_score":5,"max_score":5}),
            json!({"unreadable":false,"total_score":4,"max_score":5}),
            json!({"unreadable":false,"total_score":0,"max_score":0}),
            json!({}),
        ] {
            assert!(!is_solved(&v));
        }
    }
}
