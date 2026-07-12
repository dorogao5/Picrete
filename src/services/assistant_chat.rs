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
            api_key: settings.ai().openai_api_key.clone(),
            base_url: settings.ai().openai_base_url.trim_end_matches('/').to_string(),
            model: settings.ai().ai_model.clone(),
        })
    }

    pub(crate) async fn reply(&self, snapshot: &Value, history: &[Value]) -> Result<String> {
        let prompt = snapshot
            .pointer("/prompts/tutor/system_prompt")
            .and_then(Value::as_str)
            .context("Published assistant has no tutor prompt")?;
        let assistant = snapshot.get("assistant").cloned().unwrap_or_else(|| json!({}));
        let mut reference = String::new();
        if let Some(sheets) = snapshot.get("reference_sheets").and_then(Value::as_array) {
            for sheet in sheets {
                let title = sheet.get("title").and_then(Value::as_str).unwrap_or("Справочник");
                let content = sheet.get("content_markdown").and_then(Value::as_str).unwrap_or("");
                if reference.len() + title.len() + content.len() > 40_000 {
                    break;
                }
                reference.push_str(&format!("\n\n### {title}\n{content}"));
            }
        }
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
        let payload = json!({
            "model": self.model,
            "messages": messages,
            "max_completion_tokens": 1800
        });
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
