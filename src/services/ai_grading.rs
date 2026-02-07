use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use time::OffsetDateTime;

use crate::core::config::Settings;

const GRADING_SYSTEM_PROMPT: &str = r#"Вы — эксперт по химии и опытный преподаватель.
Ваша задача — проверить решение студента по контрольной работе и выставить баллы согласно критериям.

ВАЖНО: Если вы не можете распознать текст на изображении или изображение нечитаемо,
ВЫ ОБЯЗАНЫ вернуть предупреждение с флагом \"unreadable\": true и подробным описанием проблемы.

Критерии оценивания:
1. Корректность метода решения
2. Правильность вычислений
3. Соблюдение размерностей и единиц измерения
4. Правильная запись ответа
5. Обоснование решения

Правила химии:
- Проверка баланса химических реакций
- Проверка валентностей и зарядов
- Стехиометрические расчеты
- Перевод единиц измерения
- Округление по значащим цифрам
- Проверка формул органических соединений по ИЮПАК

Формат ответа (строгий JSON):
{
  \"unreadable\": false,
  \"unreadable_reason\": null,
  \"total_score\": <число>,
  \"max_score\": <число>,
  \"criteria_scores\": [
    {
      \"criterion_name\": \"название критерия\",
      \"score\": <число>,
      \"max_score\": <число>,
      \"comment\": \"комментарий\"
    }
  ],
  \"detailed_analysis\": {
    \"method_correctness\": \"анализ метода\",
    \"calculations\": \"анализ вычислений\",
    \"units_and_dimensions\": \"анализ размерностей\",
    \"chemical_rules\": \"проверка химических правил\",
    \"errors_found\": [\"список ошибок\"]
  },
  \"feedback\": \"Общий фидбек для студента с рекомендациями\",
  \"recommendations\": [\"рекомендация 1\", \"рекомендация 2\"],
  \"full_transcription_md\": \"ПОЛНАЯ расшифровка решения студента без ИСПРАВЛЕНИЙ, в Markdown c LaTeX ($ ... $)\",
  \"per_page_transcriptions\": [\"строго посимвольная md+LaTeX расшифровка для страницы 1\", \"... для страницы 2\", \"...\" ]
}
"#;

#[derive(Debug, Clone)]
pub(crate) struct GradeRequest {
    pub(crate) images: Vec<String>,
    pub(crate) task_description: String,
    pub(crate) reference_solution: String,
    pub(crate) rubric: Value,
    pub(crate) max_score: f64,
    pub(crate) chemistry_rules: Option<Value>,
    pub(crate) submission_id: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct AiGradingService {
    client: Client,
    api_key: String,
    base_url: String,
    model: String,
    max_tokens: u32,
    temperature: f64,
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
            temperature: settings.ai().ai_temperature,
        })
    }

    pub(crate) async fn grade_submission(&self, request: GradeRequest) -> Result<Value> {
        let started_at = OffsetDateTime::now_utc();
        let timer = Instant::now();
        let submission_id = request.submission_id.clone().unwrap_or_default();

        let user_prompt = format!(
            "\nЗадача:\n{}\n\nЭталонное решение:\n{}\n\nКритерии оценивания (максимум {} баллов):\n{}\n\nПравила проверки:\n{}\n\nПроанализируйте решение студента на изображениях и выставите баллы согласно критериям.\nОБЯЗАТЕЛЬНО используйте JSON формат ответа как описано в системном промпте.\n",
            request.task_description,
            request.reference_solution,
            request.max_score,
            serde_json::to_string_pretty(&request.rubric).unwrap_or_default(),
            serde_json::to_string_pretty(&request.chemistry_rules.unwrap_or_else(|| json!({})))
                .unwrap_or_default()
        );

        let mut content = vec![json!({"type": "text", "text": user_prompt})];
        for image in &request.images {
            if image.starts_with("http") {
                content.push(json!({
                    "type": "image_url",
                    "image_url": {"url": image}
                }));
            } else {
                content.push(json!({
                    "type": "image_url",
                    "image_url": {"url": format!("data:image/jpeg;base64,{image}")}
                }));
            }
        }

        let payload = json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": GRADING_SYSTEM_PROMPT},
                {"role": "user", "content": content}
            ],
            "max_completion_tokens": self.max_tokens,
            "temperature": self.temperature,
            "response_format": {"type": "json_object"}
        });

        tracing::info!(submission_id = %submission_id, "Sending AI grading request");

        let url = format!("{}/chat/completions", self.base_url);
        let mut last_error = None;
        let mut body = Value::Null;

        for attempt in 0..=3 {
            let response =
                self.client.post(&url).bearer_auth(&self.api_key).json(&payload).send().await;

            match response {
                Ok(resp) => {
                    let status = resp.status();
                    body = resp.json().await.unwrap_or(Value::Null);
                    if status.is_success() {
                        last_error = None;
                        break;
                    }
                    last_error = Some(anyhow::anyhow!("OpenAI API error: {body}"));
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
            .and_then(|value| value.as_str())
            .context("Missing OpenAI response content")?;

        let mut result: Value = serde_json::from_str(content).context("Failed to parse AI JSON")?;

        let elapsed = timer.elapsed().as_secs_f64();
        let completed_at = OffsetDateTime::now_utc();
        let tokens_used = body
            .get("usage")
            .and_then(|usage| usage.get("total_tokens"))
            .and_then(|value| value.as_u64());

        if result.get("unreadable").is_none() {
            result["unreadable"] = Value::Bool(false);
        }

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
            "AI grading completed"
        );

        Ok(result)
    }
}
