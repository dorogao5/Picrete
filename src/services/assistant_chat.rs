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
        let profile = build_assistant_profile(&assistant);
        let system = format!(
            "{prompt}\n\n{profile}\n\nКанонические материалы курса:{reference}\n\n\
             Отвечайте по-русски, если студент не попросил иначе. Не выдумывайте факты вне материалов. \
             Помогайте понять ход решения: задавайте уточняющие вопросы и не подменяйте самостоятельную работу готовым ответом без объяснения."
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

fn build_assistant_profile(assistant: &Value) -> String {
    let text = |key: &str| {
        assistant.get(key).and_then(Value::as_str).map(str::trim).filter(|value| !value.is_empty())
    };
    let joined = |key: &str| {
        assistant
            .get(key)
            .and_then(Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .collect::<Vec<_>>()
                    .join("; ")
            })
            .filter(|value| !value.is_empty())
    };
    let mut lines = vec![
        "ПРОФИЛЬ КУРСА И ПРЕПОДАВАТЕЛЯ".to_string(),
        "Это актуальные настройки преподавателя; соблюдайте их во всех ответах.".to_string(),
    ];
    for (label, key) in [
        ("Ассистент", "name"),
        ("Дисциплина", "discipline"),
        ("Аудитория", "audience"),
        ("Язык", "language"),
        ("Назначение", "description"),
    ] {
        if let Some(value) = text(key) {
            lines.push(format!("{label}: {value}"));
        }
    }
    if let Some(value) = joined("topics") {
        lines.push(format!("Темы курса: {value}"));
    }
    if let Some(criteria) =
        assistant.get("criteria").and_then(Value::as_array).filter(|v| !v.is_empty())
    {
        lines.push("Критерии оценивания:".to_string());
        lines.extend(criteria.iter().map(|criterion| {
            let name = criterion.get("name").and_then(Value::as_str).unwrap_or("Критерий");
            let score = criterion.get("max_score").map(Value::to_string);
            let description = criterion
                .get("description")
                .and_then(Value::as_str)
                .filter(|v| !v.trim().is_empty());
            let mut parts = vec![name.trim().to_string()];
            if let Some(score) = score {
                parts.push(format!("максимум {score} балла"));
            }
            if let Some(description) = description {
                parts.push(description.trim().to_string());
            }
            format!("- {}", parts.join(" — "))
        }));
    }
    if let Some(value) = joined("nuances") {
        lines.push(format!("Требования преподавателя: {value}"));
    }
    lines.join("\n")
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
                    + if contains_search_term(&title_lower, term) { 8 } else { 0 }
                    + if contains_search_term(&description_lower, term) { 3 } else { 0 }
                    + if contains_search_term(&content_lower, term) { 1 } else { 0 }
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

fn contains_search_term(text: &str, term: &str) -> bool {
    if term.chars().count() > 2 {
        return text.contains(term);
    }
    text.match_indices(term).any(|(start, matched)| {
        let before_is_boundary =
            text[..start].chars().next_back().map_or(true, |ch| !ch.is_alphanumeric());
        let end = start + matched.len();
        let after_is_boundary = text[end..].chars().next().map_or(true, |ch| !ch.is_alphanumeric());
        before_is_boundary && after_is_boundary
    })
}

#[cfg(test)]
mod tests {
    use super::{build_assistant_profile, build_payload, select_reference_sheets};
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
    fn short_spectroscopy_term_does_not_match_inside_unrelated_word() {
        let snapshot = json!({
            "reference_sheets": [
                {"title": "Прикладная аналитика", "description": "Метрики аналитики", "content_markdown": "Общий обзор"},
                {"title": "ИК-спектроскопия", "description": "Колебательные спектры", "content_markdown": "Характеристические полосы"}
            ]
        });

        let selected =
            select_reference_sheets(&snapshot, "Что показывает ИК-спектроскопия?", 2_000);

        assert!(selected.starts_with("\n\n### ИК-спектроскопия"));
        assert!(!selected.contains("### Прикладная аналитика"));
    }

    #[test]
    fn profile_uses_all_published_teacher_settings() {
        let profile = build_assistant_profile(&json!({
            "name": "Практикум",
            "discipline": "Неорганическая химия",
            "description": "Первый курс",
            "audience": "студенты 1 курса",
            "language": "ru",
            "topics": ["Растворы", "Лабораторная работа"],
            "criteria": [{"name": "Расчёт", "max_score": 4, "description": "Проверить единицы"}],
            "nuances": ["Не придумывать наблюдения"]
        }));

        assert!(profile.contains("Аудитория: студенты 1 курса"));
        assert!(profile.contains("Темы курса: Растворы; Лабораторная работа"));
        assert!(profile.contains("- Расчёт — максимум 4 балла — Проверить единицы"));
        assert!(profile.contains("Требования преподавателя: Не придумывать наблюдения"));
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
