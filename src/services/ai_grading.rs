use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use time::OffsetDateTime;

use crate::core::config::Settings;

const PRECHECK_SYSTEM_PROMPT: &str = r#"Вы — эксперт по химии и опытный преподаватель.
Ваша задача — выполнить ПРЕДВАРИТЕЛЬНУЮ проверку решения студента по OCR-расшифровке.

Контекст:
1. OCR может содержать ошибки.
2. Если студент отправил REPORT, используйте report_issues как источник правок OCR.
3. Препроверка не заменяет преподавателя: будьте консервативны и объясняйте выводы.

Критерии оценивания:
1. Корректность метода решения
2. Правильность вычислений
3. Соблюдение размерностей и единиц измерения
4. Правильная запись ответа
5. Обоснование решения

Формат ответа (строгий JSON):
{
  "unreadable": false,
  "unreadable_reason": null,
  "total_score": <число>,
  "max_score": <число>,
  "criteria_scores": [
    {
      "criterion_name": "название критерия",
      "score": <число>,
      "max_score": <число>,
      "comment": "комментарий"
    }
  ],
  "detailed_analysis": {
    "method_correctness": "анализ метода",
    "calculations": "анализ вычислений",
    "units_and_dimensions": "анализ размерностей",
    "chemical_rules": "проверка химических правил",
    "errors_found": ["список ошибок"]
  },
  "feedback": "Общий фидбек для студента с рекомендациями",
  "recommendations": ["рекомендация 1", "рекомендация 2"],
  "full_transcription_md": "Сводная OCR-расшифровка в Markdown c LaTeX ($ ... $)",
  "per_page_transcriptions": ["OCR-страница 1", "OCR-страница 2"]
}
"#;

#[derive(Debug, Clone)]
pub(crate) struct LlmPrecheckRequest {
    pub(crate) submission_id: Option<String>,
    pub(crate) ocr_markdown_pages: Vec<String>,
    pub(crate) ocr_report_issues: Vec<Value>,
    pub(crate) report_summary: Option<String>,
    pub(crate) task_description: String,
    pub(crate) reference_solution: String,
    pub(crate) rubric: Value,
    pub(crate) max_score: f64,
    pub(crate) chemistry_rules: Option<Value>,
}

#[derive(Debug, Clone)]
pub(crate) struct AiGradingService {
    client: Client,
    api_key: String,
    base_url: String,
    model: String,
    max_tokens: u32,
}

impl AiGradingService {
    pub(crate) fn from_settings(settings: &Settings) -> Result<Self> {
        let timeout = Duration::from_secs(settings.ai().ai_request_timeout);
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(30))
            .timeout(timeout)
            .build()
            .context("Failed to build HTTP client")?;

        Ok(Self {
            client,
            api_key: settings.ai().openai_api_key.clone(),
            base_url: settings.ai().openai_base_url.trim_end_matches('/').to_string(),
            model: settings.ai().ai_model.clone(),
            max_tokens: settings.ai().ai_max_tokens,
        })
    }

    pub(crate) async fn run_precheck(&self, request: LlmPrecheckRequest) -> Result<Value> {
        let started_at = OffsetDateTime::now_utc();
        let timer = Instant::now();
        let submission_id = request.submission_id.clone().unwrap_or_default();
        let full_ocr = request.ocr_markdown_pages.join("\n\n---\n\n");

        let user_prompt = format!(
            "\nЗадача:\n{}\n\nЭталонное решение:\n{}\n\nКритерии оценивания (максимум {} баллов):\n{}\n\nПравила проверки:\n{}\n\nOCR по страницам:\n{}\n\nREPORT summary:\n{}\n\nREPORT issues:\n{}\n\nВыполните предварительную проверку по OCR и report-правкам. Ответ строго JSON.\n",
            request.task_description,
            request.reference_solution,
            request.max_score,
            serde_json::to_string_pretty(&request.rubric).unwrap_or_default(),
            serde_json::to_string_pretty(&request.chemistry_rules.unwrap_or_else(|| json!({})))
                .unwrap_or_default(),
            serde_json::to_string_pretty(&request.ocr_markdown_pages).unwrap_or_else(|_| full_ocr.clone()),
            request.report_summary.clone().unwrap_or_default(),
            serde_json::to_string_pretty(&request.ocr_report_issues).unwrap_or_default(),
        );

        let payload = json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": PRECHECK_SYSTEM_PROMPT},
                {"role": "user", "content": user_prompt}
            ],
            "max_completion_tokens": self.max_tokens,
            "response_format": {"type": "json_object"}
        });

        tracing::info!(submission_id = %submission_id, "Sending LLM precheck request");

        let url = format!("{}/chat/completions", self.base_url);
        let mut last_error = None;
        let mut body = Value::Null;

        for attempt in 0..=3 {
            let response =
                self.client.post(&url).bearer_auth(&self.api_key).json(&payload).send().await;

            match response {
                Ok(resp) => {
                    let status = resp.status();
                    let raw_body =
                        resp.text().await.context("Failed to read OpenAI response body")?;

                    match serde_json::from_str::<Value>(&raw_body) {
                        Ok(parsed) => {
                            body = parsed;
                            if status.is_success() {
                                last_error = None;
                                break;
                            }
                            last_error = Some(anyhow::anyhow!(
                                "OpenAI API error (status {status}): {raw_body}"
                            ));
                        }
                        Err(parse_err) => {
                            last_error = Some(anyhow::anyhow!(
                                "OpenAI API returned non-JSON response (status {status}): {parse_err}; body: {raw_body}"
                            ));
                        }
                    }
                }
                Err(err) => {
                    last_error = Some(anyhow::anyhow!(err).context("Failed to call OpenAI API"));
                }
            }

            if attempt < 3 {
                tokio::time::sleep(Duration::from_secs(2_u64.pow(attempt as u32))).await;
            }
        }

        if let Some(err) = last_error {
            return Err(err);
        }

        let content = body
            .get("choices")
            .and_then(|choices| choices.get(0))
            .and_then(|choice| choice.get("message"))
            .and_then(|message| message.get("content"))
            .and_then(Value::as_str)
            .context("Missing OpenAI response content")?;

        let mut result: Value = serde_json::from_str(content).context("Failed to parse AI JSON")?;

        if result.get("unreadable").is_none() {
            result["unreadable"] = Value::Bool(false);
        }
        if result.get("full_transcription_md").is_none() {
            result["full_transcription_md"] = Value::String(full_ocr);
        }
        if result.get("per_page_transcriptions").is_none() {
            result["per_page_transcriptions"] = serde_json::to_value(&request.ocr_markdown_pages)
                .unwrap_or_else(|_| Value::Array(Vec::new()));
        }

        let elapsed = timer.elapsed().as_secs_f64();
        let completed_at = OffsetDateTime::now_utc();
        let tokens_used =
            body.get("usage").and_then(|usage| usage.get("total_tokens")).and_then(Value::as_u64);

        result["_metadata"] = json!({
            "request_started_at": started_at.format(&time::format_description::well_known::Rfc3339).unwrap_or_default(),
            "request_completed_at": completed_at.format(&time::format_description::well_known::Rfc3339).unwrap_or_default(),
            "duration_seconds": elapsed,
            "tokens_used": tokens_used,
            "model": self.model,
        });

        tracing::info!(
            submission_id = %submission_id,
            duration_seconds = elapsed,
            tokens_used = tokens_used,
            "LLM precheck completed"
        );

        Ok(result)
    }
}
