use anyhow::{Context, Result};
use reqwest::multipart::Form;
use reqwest::Client;
use serde_json::Value;
use std::time::Duration;

use crate::core::config::Settings;

#[derive(Debug, Clone)]
pub(crate) struct OcrResult {
    pub(crate) markdown: Option<String>,
    pub(crate) chunks: Option<Value>,
    pub(crate) model: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct DatalabOcrService {
    client: Client,
    api_key: String,
    fallback_api_key: Option<String>,
    base_url: String,
    mode: String,
    output_format: String,
    poll_interval: Duration,
    max_poll_attempts: u32,
    max_submit_retries: u32,
}

#[derive(Debug, Clone)]
struct MarkerJobRef {
    request_id: String,
    request_check_url: String,
}

impl DatalabOcrService {
    pub(crate) fn from_settings(settings: &Settings) -> Result<Self> {
        let timeout = Duration::from_secs(settings.datalab().timeout_seconds);
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(20))
            .timeout(timeout)
            .build()
            .context("Failed to build DataLab HTTP client")?;

        Ok(Self {
            client,
            api_key: settings.datalab().api_key.clone(),
            fallback_api_key: settings.datalab().fallback_api_key.clone(),
            base_url: settings.datalab().base_url.trim_end_matches('/').to_string(),
            mode: settings.datalab().mode.clone(),
            output_format: settings.datalab().output_format.clone(),
            poll_interval: Duration::from_secs(settings.datalab().poll_interval_seconds),
            max_poll_attempts: settings.datalab().max_poll_attempts,
            max_submit_retries: settings.datalab().max_submit_retries,
        })
    }

    pub(crate) async fn run_marker_for_file_url(&self, file_url: &str) -> Result<OcrResult> {
        let (job_ref, api_key) = match self.submit_marker_job(file_url, &self.api_key).await {
            Ok(job) => (job, self.api_key.as_str()),
            Err(error) if error.downcast_ref::<CreditExhausted>().is_some() => {
                let key = self
                    .fallback_api_key
                    .as_deref()
                    .filter(|key| *key != self.api_key)
                    .ok_or(error)?;
                tracing::warn!("DataLab primary credits exhausted; using fallback key for OCR");
                (self.submit_marker_job(file_url, key).await?, key)
            }
            Err(error) => return Err(error),
        };
        self.poll_marker_result(&job_ref, api_key).await
    }

    async fn submit_marker_job(&self, file_url: &str, api_key: &str) -> Result<MarkerJobRef> {
        let endpoint = format!("{}/marker", self.base_url);

        let mut last_error = None;

        for attempt in 0..=self.max_submit_retries {
            let form = Form::new()
                .text("file_url", file_url.to_string())
                .text("mode", self.mode.clone())
                .text("output_format", self.output_format.clone());

            let response = self
                .client
                .post(&endpoint)
                .header("X-Api-Key", api_key)
                .multipart(form)
                .send()
                .await;

            match response {
                Ok(resp) => {
                    let status = resp.status();
                    let raw_body =
                        resp.text().await.context("Failed to read DataLab marker response")?;

                    let parsed = serde_json::from_str::<Value>(&raw_body).map_err(|err| {
                        anyhow::anyhow!(
                            "DataLab marker returned non-JSON body (status {}): {}: {}",
                            status,
                            err,
                            raw_body
                        )
                    })?;

                    if credits_exhausted(status.as_u16(), &parsed) {
                        return Err(CreditExhausted.into());
                    }
                    if !status.is_success() {
                        last_error = Some(anyhow::anyhow!(
                            "DataLab marker submit failed (status {}): {}",
                            status,
                            extract_error_message(&parsed)
                        ));
                    } else if parsed
                        .get("success")
                        .and_then(Value::as_bool)
                        .is_some_and(|value| !value)
                    {
                        last_error = Some(anyhow::anyhow!(
                            "DataLab marker submit returned success=false: {}",
                            extract_error_message(&parsed)
                        ));
                    } else if let Some(job_ref) = extract_marker_job_ref(&self.base_url, &parsed) {
                        return Ok(job_ref);
                    } else {
                        last_error = Some(anyhow::anyhow!(
                            "DataLab marker submit response missing request reference"
                        ));
                    }
                }
                Err(err) => {
                    last_error =
                        Some(anyhow::anyhow!(err).context("Failed to call DataLab marker API"));
                }
            }

            if attempt < self.max_submit_retries {
                let backoff = Duration::from_secs(2_u64.pow(attempt));
                tokio::time::sleep(backoff).await;
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Unknown DataLab submit error")))
    }

    async fn poll_marker_result(&self, job_ref: &MarkerJobRef, api_key: &str) -> Result<OcrResult> {
        for attempt in 0..self.max_poll_attempts {
            let response = self
                .client
                .get(&job_ref.request_check_url)
                .header("X-Api-Key", api_key)
                .send()
                .await
                .context("Failed to call DataLab marker result endpoint")?;

            let status_code = response.status();
            let raw_body = response.text().await.context("Failed to read DataLab poll response")?;
            let parsed: Value = serde_json::from_str(&raw_body).map_err(|err| {
                anyhow::anyhow!(
                    "DataLab poll returned non-JSON body (status {}): {}: {}",
                    status_code,
                    err,
                    raw_body
                )
            })?;

            if !status_code.is_success() {
                return Err(anyhow::anyhow!(
                    "DataLab poll failed (status {}): {}",
                    status_code,
                    extract_error_message(&parsed)
                ));
            }

            let status = parsed
                .get("status")
                .and_then(Value::as_str)
                .map(|value| value.to_ascii_lowercase())
                .unwrap_or_else(|| "unknown".to_string());

            if status == "complete" || status == "completed" {
                let (markdown, chunks, model) = extract_result_payload(&parsed);
                return Ok(OcrResult { markdown, chunks, model });
            }

            if status == "failed" || status == "error" {
                return Err(anyhow::anyhow!(
                    "DataLab OCR job {} failed: {}",
                    job_ref.request_id,
                    extract_error_message(&parsed)
                ));
            }

            if parsed.get("success").and_then(Value::as_bool).is_some_and(|value| !value) {
                return Err(anyhow::anyhow!(
                    "DataLab OCR job {} returned success=false: {}",
                    job_ref.request_id,
                    extract_error_message(&parsed)
                ));
            }

            if attempt + 1 >= self.max_poll_attempts {
                break;
            }

            tokio::time::sleep(self.poll_interval).await;
        }

        Err(anyhow::anyhow!(
            "DataLab OCR polling timed out for request {} after {} attempts",
            job_ref.request_id,
            self.max_poll_attempts
        ))
    }
}

fn extract_marker_job_ref(base_url: &str, payload: &Value) -> Option<MarkerJobRef> {
    let request_check_url = extract_request_check_url(base_url, payload);
    let request_id = extract_request_id(payload).or_else(|| {
        request_check_url
            .clone()
            .and_then(|url| url.trim_end_matches('/').rsplit('/').next().map(ToString::to_string))
    })?;

    let request_check_url =
        request_check_url.unwrap_or_else(|| format!("{}/marker/{}", base_url, request_id));

    Some(MarkerJobRef { request_id, request_check_url })
}

fn extract_request_check_url(base_url: &str, payload: &Value) -> Option<String> {
    let raw = payload.get("request_check_url").and_then(Value::as_str)?;
    if raw.starts_with("http://") || raw.starts_with("https://") {
        return Some(raw.to_string());
    }
    let normalized_base = format!("{}/", base_url.trim_end_matches('/'));
    reqwest::Url::parse(&normalized_base)
        .ok()
        .and_then(|base| base.join(raw).ok())
        .map(|url| url.to_string())
}

fn extract_request_id(payload: &Value) -> Option<String> {
    if let Some(id) = payload.get("request_id").and_then(Value::as_str) {
        return Some(id.to_string());
    }

    if let Some(id) = payload.get("request_check_id").and_then(Value::as_str) {
        return Some(id.to_string());
    }

    None
}

fn extract_result_payload(payload: &Value) -> (Option<String>, Option<Value>, Option<String>) {
    let container = payload.get("result").unwrap_or(payload);

    let markdown =
        container.get("markdown").and_then(Value::as_str).map(|value| value.to_string()).or_else(
            || payload.get("markdown").and_then(Value::as_str).map(|value| value.to_string()),
        );

    let chunks = container.get("chunks").cloned().or_else(|| payload.get("chunks").cloned());

    let model =
        container.get("model").and_then(Value::as_str).map(|value| value.to_string()).or_else(
            || payload.get("model").and_then(Value::as_str).map(|value| value.to_string()),
        );

    (markdown, chunks, model)
}

fn extract_error_message(payload: &Value) -> String {
    if let Some(detail) = payload.get("detail") {
        if let Some(text) = detail.as_str() {
            return text.to_string();
        }
        if let Some(items) = detail.as_array() {
            let joined = items
                .iter()
                .filter_map(|item| {
                    item.get("msg")
                        .and_then(Value::as_str)
                        .or_else(|| item.get("message").and_then(Value::as_str))
                })
                .collect::<Vec<_>>()
                .join("; ");
            if !joined.is_empty() {
                return joined;
            }
        }
    }

    payload
        .get("message")
        .and_then(Value::as_str)
        .or_else(|| payload.get("error").and_then(Value::as_str))
        .unwrap_or("unknown_error")
        .to_string()
}

#[derive(Debug)]
struct CreditExhausted;
impl std::fmt::Display for CreditExhausted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DataLab OCR credits exhausted")
    }
}
impl std::error::Error for CreditExhausted {}

fn credits_exhausted(status: u16, payload: &Value) -> bool {
    if payload.get("cloudflare_error").and_then(Value::as_bool) == Some(true) {
        return false;
    }
    let message = extract_error_message(payload).to_ascii_lowercase();
    (status == 402
        || status == 403
        || payload.get("success").and_then(Value::as_bool) == Some(false))
        && [
            "insufficient credits",
            "insufficient balance",
            "out of credits",
            "no credits",
            "credits exhausted",
            "credit balance",
            "exceeded your credits",
            "payment required",
        ]
        .iter()
        .any(|reason| message.contains(reason))
}

#[cfg(test)]
mod fallback_tests {
    use super::*;
    #[test]
    fn fallback_only_for_billing_denials() {
        assert!(credits_exhausted(403, &serde_json::json!({"detail":"Insufficient credits"})));
        assert!(credits_exhausted(402, &serde_json::json!({"detail":"Payment required"})));
        assert!(!credits_exhausted(403, &serde_json::json!({"detail":"Invalid API key"})));
        assert!(!credits_exhausted(
            403,
            &serde_json::json!({"cloudflare_error":true,"detail":"Access denied"})
        ));
        assert!(!credits_exhausted(429, &serde_json::json!({"detail":"Rate limit exceeded"})));
        assert!(!credits_exhausted(
            200,
            &serde_json::json!({"success":true,"detail":"credit balance"})
        ));
    }
}

#[cfg(test)]
mod fallback_flow_tests {
    use super::*;
    use axum::{
        http::{HeaderMap, StatusCode},
        routing::{get, post},
        Json, Router,
    };
    use std::sync::{Arc, Mutex};

    #[tokio::test]
    async fn exhausted_primary_submits_and_polls_with_backup() {
        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        let submit_seen = seen.clone();
        let poll_seen = seen.clone();
        let app = Router::new()
            .route(
                "/marker",
                post(move |headers: HeaderMap| {
                    let seen = submit_seen.clone();
                    async move {
                        let key = headers["x-api-key"].to_str().unwrap().to_string();
                        seen.lock().unwrap().push(format!("submit:{key}"));
                        if key == "primary" {
                            (
                                StatusCode::FORBIDDEN,
                                Json(serde_json::json!({"detail":"Insufficient credits"})),
                            )
                        } else {
                            (
                                StatusCode::OK,
                                Json(serde_json::json!({"success":true,"request_id":"job"})),
                            )
                        }
                    }
                }),
            )
            .route(
                "/marker/job",
                get(move |headers: HeaderMap| {
                    let seen = poll_seen.clone();
                    async move {
                        let key = headers["x-api-key"].to_str().unwrap().to_string();
                        seen.lock().unwrap().push(format!("poll:{key}"));
                        Json(serde_json::json!({"status":"complete","markdown":"2 + 3 = 5"}))
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let service = DatalabOcrService {
            client: Client::new(),
            api_key: "primary".into(),
            fallback_api_key: Some("backup".into()),
            base_url,
            mode: "accurate".into(),
            output_format: "markdown".into(),
            poll_interval: Duration::ZERO,
            max_poll_attempts: 2,
            max_submit_retries: 2,
        };
        let result = service.run_marker_for_file_url("https://example.com/test.pdf").await.unwrap();
        assert_eq!(result.markdown.as_deref(), Some("2 + 3 = 5"));
        assert_eq!(*seen.lock().unwrap(), vec!["submit:primary", "submit:backup", "poll:backup"]);
        server.abort();
    }
}
