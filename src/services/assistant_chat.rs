use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::{json, Value};
use std::time::Duration;

use crate::core::config::Settings;

pub(crate) struct AssistantChatService {
    client: Client,
    api_key: String,
    base_url: String,
    model: String,
}

impl AssistantChatService {
    pub(crate) fn from_settings(settings: &Settings) -> Result<Self> {
        let timeout = Duration::from_secs(settings.ai().ai_request_timeout.min(120));
        Ok(Self {
            client: Client::builder()
                .connect_timeout(Duration::from_secs(10))
                .timeout(timeout)
                .build()
                .context("Failed to build assistant HTTP client")?,
            api_key: settings.ai().assistant_api_key.clone(),
            base_url: settings.ai().assistant_base_url.trim_end_matches('/').to_string(),
            model: settings.ai().assistant_model.clone(),
        })
    }

    pub(crate) async fn reply(&self, snapshot: &Value, history: &[Value]) -> Result<String> {
        let prompt = snapshot
            .pointer("/prompts/tutor/system_prompt")
            .and_then(Value::as_str)
            .context("Published assistant has no tutor prompt")?;
        let assistant = snapshot.get("assistant").cloned().unwrap_or_else(|| json!({}));
        let query = history
            .iter()
            .rev()
            .find(|message| message.get("role").and_then(Value::as_str) == Some("user"))
            .and_then(|message| message.get("content"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let reference = select_reference_sheets(snapshot, query, 40_000);
        let system = format!(
            "{prompt}\n\nПрофиль курса:\n{}\n\nКанонические материалы курса:{}\n\n\
             Отвечайте по-русски, если студент не попросил иначе. Не выдумывайте факты вне материалов. \
             Помогайте понять ход решения: задавайте уточняющие вопросы и не подменяйте самостоятельную работу готовым ответом без объяснения.",
            serde_json::to_string_pretty(&assistant).unwrap_or_default(),
            reference,
        );
        let mut messages = vec![json!({"role": "system", "content": system})];
        messages
            .extend(history.iter().rev().take(12).cloned().collect::<Vec<_>>().into_iter().rev());
        let payload = build_payload(&self.model, messages);
        let response = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&payload)
            .send()
            .await
            .context("Failed to call assistant model")?;
        let status = response.status();
        let body: Value = response.json().await.context("Assistant model returned invalid JSON")?;
        if !status.is_success() {
            anyhow::bail!("Assistant model returned HTTP {status}");
        }
        body.pointer("/choices/0/message/content")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(str::to_owned)
            .context("Assistant model returned an empty answer")
    }
}

fn build_payload(model: &str, messages: Vec<Value>) -> Value {
    let mut payload = json!({
        "model": model,
        "messages": messages,
    });
    if model.to_ascii_lowercase().contains("deepseek") {
        payload["max_tokens"] = json!(1800);
        payload["thinking"] = json!({"type": "enabled"});
    } else {
        payload["max_completion_tokens"] = json!(1800);
    }
    payload
}

fn select_reference_sheets(snapshot: &Value, query: &str, max_chars: usize) -> String {
    let Some(sheets) = snapshot.get("reference_sheets").and_then(Value::as_array) else {
        return String::new();
    };
    let query_lower = query.to_lowercase();
    let mut terms = query_lower
        .split(|character: char| !character.is_alphanumeric())
        .filter(|term| {
            term.chars().count() >= 3
                || matches!(*term, "ph" | "pk" | "пр" | "мо" | "вс" | "ir" | "ик" | "ямр")
        })
        .take(30)
        .collect::<Vec<_>>();
    terms.sort_unstable();
    terms.dedup();

    let mut ranked = sheets
        .iter()
        .enumerate()
        .map(|(index, sheet)| {
            let title = sheet.get("title").and_then(Value::as_str).unwrap_or("");
            let description = sheet.get("description").and_then(Value::as_str).unwrap_or("");
            let content = sheet.get("content_markdown").and_then(Value::as_str).unwrap_or("");
            let title_lower = title.to_lowercase();
            let description_lower = description.to_lowercase();
            let content_lower = content.to_lowercase();
            let score = terms.iter().fold(0_u32, |score, term| {
                score
                    + if title_lower.contains(term) { 8 } else { 0 }
                    + if description_lower.contains(term) { 3 } else { 0 }
                    + if content_lower.contains(term) { 1 } else { 0 }
            });
            (score, index, title, content)
        })
        .collect::<Vec<_>>();
    ranked.sort_by_key(|(score, index, _, _)| (std::cmp::Reverse(*score), *index));

    let mut reference = String::new();
    for (_, _, title, content) in ranked.into_iter().filter(|(score, _, _, _)| *score > 0).take(8) {
        let section =
            format!("\n\n### {}\n{}", if title.is_empty() { "Справочник" } else { title }, content);
        if reference.len() + section.len() > max_chars {
            continue;
        }
        reference.push_str(&section);
    }
    reference
}

#[cfg(test)]
mod tests {
    use super::{build_payload, select_reference_sheets};
    use serde_json::json;

    #[test]
    fn relevant_sheets_are_selected_before_unrelated_ones() {
        let snapshot = json!({
            "reference_sheets": [
                {"title": "Растворимость", "description": "", "content_markdown": "Общие правила"},
                {"title": "Карбонаты", "description": "Реакции с кислотами", "content_markdown": "Выделяется CO2"}
            ]
        });
        let selected =
            select_reference_sheets(&snapshot, "Почему карбонат реагирует с кислотой?", 2_000);
        assert!(selected.starts_with("\n\n### Карбонаты"));
        assert!(!selected.contains("### Растворимость"));
    }

    #[test]
    fn deepseek_payload_uses_its_native_token_and_thinking_fields() {
        let payload =
            build_payload("deepseek-v4-flash", vec![json!({"role": "user", "content": "test"})]);

        assert_eq!(payload["max_tokens"], 1800);
        assert_eq!(
            payload.pointer("/thinking/type").and_then(|value| value.as_str()),
            Some("enabled")
        );
        assert!(payload.get("max_completion_tokens").is_none());
    }

    #[test]
    fn generic_payload_keeps_openai_completion_limit() {
        let payload = build_payload("gpt-5.5", Vec::new());

        assert_eq!(payload["max_completion_tokens"], 1800);
        assert!(payload.get("max_tokens").is_none());
        assert!(payload.get("thinking").is_none());
    }
}
