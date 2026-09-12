//! Private deterministic tools. Configuration is server-owned, never snapshot/model supplied.
use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

use crate::core::config::Settings;

const MAX_BODY_BYTES: usize = 256 * 1024;
const TOOL_FINALIZATION_PROMPT: &str = concat!(
    "Предыдущий ответ завершился без итогового текста. Завершите ответ для этой же ",
    "задачи в ранее заданном формате, используя уже полученные результаты инструментов. ",
    "Не меняйте выбранные исходные данные и вопросы, не создавайте другую задачу, ",
    "не повторяйте расчёты и не приписывайте невыполненных проверок. ",
    "Сохраните учебный режим: для подсказки отвечайте только на текущий шаг."
);

// Mirrored from Studio services/contracts.py ESSENTIAL_TOOLS_INSTRUCTION.
// The text file's final LF is formatting, not part of the shared prompt.
fn essential_tools_instruction() -> &'static str {
    include_str!("essential_tools_instruction.txt").trim_end_matches('\n')
}

#[derive(Clone, Debug)]
pub(crate) struct ToolGateway {
    client: Client,
    url: String,
    token: String,
    max_rounds: usize,
    max_calls: usize,
    flow_timeout: Duration,
    max_output_tokens_by_model: HashMap<String, u64>,
}

impl ToolGateway {
    pub(crate) fn from_settings(settings: &Settings) -> Result<Self> {
        let mut gateway = Self::new(
            &settings.ai().essential_tools_gateway_url,
            &settings.ai().essential_tools_gateway_token,
        )?;
        anyhow::ensure!(
            (1..=32).contains(&settings.ai().essential_tools_max_rounds)
                && (1..=96).contains(&settings.ai().essential_tools_max_calls),
            "Invalid essential tool limits"
        );
        gateway.max_rounds = settings.ai().essential_tools_max_rounds;
        gateway.max_calls = settings.ai().essential_tools_max_calls;
        gateway.flow_timeout = Duration::from_secs(settings.ai().assistant_request_timeout);
        gateway.max_output_tokens_by_model = settings.ai().max_output_tokens_by_model.clone();
        Ok(gateway)
    }

    fn new(base: &str, token: &str) -> Result<Self> {
        let mut url = reqwest::Url::parse(base.trim())
            .context("Missing or invalid essential tools gateway URL")?;
        anyhow::ensure!(
            matches!(url.scheme(), "http" | "https")
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none(),
            "Invalid essential tools gateway URL"
        );
        anyhow::ensure!(
            token.len() >= 32
                && token.is_ascii()
                && !token.bytes().any(|b| b.is_ascii_whitespace()),
            "Essential tools gateway token must be at least 32 ASCII non-whitespace characters"
        );
        url.set_path(&format!("{}/invoke", url.path().trim_end_matches('/')));
        Ok(Self {
            client: Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(3))
                .timeout(Duration::from_secs(10))
                .build()?,
            url: url.into(),
            token: token.to_owned(),
            max_rounds: 8,
            max_calls: 24,
            flow_timeout: Duration::from_secs(110),
            max_output_tokens_by_model: HashMap::new(),
        })
    }

    async fn invoke(&self, name: &str, arguments: &Value) -> Result<Value> {
        let response = self
            .client
            .post(&self.url)
            .bearer_auth(&self.token)
            .json(&json!({"tool":name,"arguments":arguments}))
            .send()
            .await
            .context("Essential tool request failed")?;
        anyhow::ensure!(
            response.status().is_success(),
            "Essential tool returned HTTP {}",
            response.status()
        );
        let result = bounded_json(response).await?;
        anyhow::ensure!(
            result["tool_name"] == name
                && result["tool_version"]
                    .as_str()
                    .is_some_and(|v| !v.trim().is_empty() && v.len() <= 128)
                && result["arguments"] == *arguments
                && result["trace_id"].as_str().is_some_and(|s| !s.is_empty())
                && result.get("normalized_result").is_some(),
            "Invalid essential tool envelope"
        );
        if matches!(result["status"].as_str(), Some("error" | "invalid_request")) {
            anyhow::ensure!(result["error"].is_string(), "Invalid essential tool error");
            return Ok(result);
        }
        anyhow::ensure!(result["status"] == "success", "Invalid essential tool status");
        anyhow::ensure!(
            result.get("normalized_result").is_some()
                && result["trace_id"].as_str().is_some_and(|s| !s.is_empty()),
            "Invalid essential tool result"
        );
        // Preserve scientific provenance for internal model context, never return
        // this envelope as a student response.
        Ok(result)
    }
}

async fn bounded_json(mut response: reqwest::Response) -> Result<Value> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        anyhow::ensure!(
            bytes.len() + chunk.len() <= MAX_BODY_BYTES,
            "Tool flow response too large"
        );
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).context("Tool flow returned invalid JSON")
}

fn definitions() -> Value {
    // Vendored unchanged from Studio-Picrete/essential_skills/tool-definitions.json.
    let definitions: Value = serde_json::from_str(include_str!("essential_tool_definitions.json"))
        .expect("Bundled tool definitions");
    Value::Array(definitions.as_object().unwrap().iter().map(|(name, def)| json!({
        "type":"function", "function":{"name":name,"description":def["description"],"parameters":def["parameters"]}
    })).collect())
}

fn validate_arguments(name: &str, arguments: &Value) -> Result<()> {
    let fields = arguments.as_object().context("Tool arguments must be an object")?;
    let allowed: &[&str] = match name {
        "calculator" => &["expression"],
        "reference_db" => &["reference_id"],
        "sympy" => &["operation", "expression", "equation", "left", "right", "variable", "symbols"],
        _ => anyhow::bail!("Unknown essential tool"),
    };
    for (key, value) in fields {
        anyhow::ensure!(allowed.contains(&key.as_str()), "Unknown tool argument");
        if key == "symbols" {
            anyhow::ensure!(
                value.as_array().is_some_and(|v| v.len() <= 64
                    && v.iter()
                        .all(|s| s.as_str().is_some_and(|s| !s.is_empty() && s.len() <= 64))),
                "Invalid tool symbols"
            );
        } else {
            anyhow::ensure!(
                value.as_str().is_some_and(|s| !s.trim().is_empty() && s.len() <= 512),
                "Invalid tool argument type or length"
            );
        }
    }
    if name == "sympy" {
        let symbols = arguments["symbols"].as_array().context("Missing tool symbols")?;
        let unique: HashSet<_> = symbols.iter().filter_map(Value::as_str).collect();
        anyhow::ensure!(unique.len() == symbols.len(), "Duplicate tool symbols");
        if let Some(variable) = arguments["variable"].as_str() {
            anyhow::ensure!(unique.contains(variable), "Declare the tool variable in symbols");
        }
    }
    let required: &[&str] = match name {
        "calculator" => &["expression"],
        "reference_db" => &["reference_id"],
        "sympy" => match arguments["operation"].as_str() {
            Some("identity") => &["left", "right"],
            Some("solve") => &["equation", "variable"],
            Some("derivative" | "integral") => &["expression", "variable"],
            Some("simplify") => &["expression"],
            _ => anyhow::bail!("Invalid symbolic operation"),
        },
        _ => unreachable!(),
    };
    anyhow::ensure!(required.iter().all(|key| fields.contains_key(*key)), "Missing tool argument");
    Ok(())
}

/// Each completion is called once. An empty final after successful tools may
/// receive one same-context, tools-disabled finalization continuation.
/// Argument/computation errors can be corrected in this same conversation;
/// transport errors, timeouts and exhaustion never trigger retries.
pub(crate) async fn complete(
    client: &Client,
    url: &str,
    api_key: &str,
    mut payload: Value,
    gateway: &ToolGateway,
) -> Result<Value> {
    tokio::time::timeout(gateway.flow_timeout, async {
        let model = payload["model"].as_str().unwrap_or_default();
        if payload.get("max_tokens").is_none() {
            if let Some(limit) = gateway.max_output_tokens_by_model.get(model) {
                payload["max_tokens"] = json!(limit);
            }
        }
        payload["tools"] = definitions();
        payload["tool_choice"] = json!("auto");
        let system = payload["messages"][0]["content"].as_str().context("Missing system prompt")?;
        payload["messages"][0]["content"] = json!(format!("{system}{}\nНе раскрывайте внутренние трассы или эталон целиком в режиме практики. Сохраняйте учебный режим; результат инструмента сам по себе не является оценкой.", essential_tools_instruction()));
        let mut seen = HashSet::new();
        let mut used = 0;
        let mut usage = [Some(0_u64); 3];
        let mut traces = Vec::new();
        let mut usage_by_call = Vec::new();
        let mut finalization_continuations = 0;
        let flow_id = uuid::Uuid::new_v4().to_string();
        // The extra slot is exclusively for finalizing an empty stop response;
        // normal tool rounds retain their existing limit below.
        for round in 0..=gateway.max_rounds + 1 {
            let response = client.post(url).bearer_auth(api_key).json(&payload).send().await
                .context("Tool-enabled model request failed")?;
            anyhow::ensure!(response.status().is_success(), "Tool-enabled model returned HTTP {}", response.status());
            let mut body = bounded_json(response).await?;
            tracing::info!(%flow_id, round, finalization_continuations, model = %payload["model"], usage = %body["usage"], "Essential tool model completion received");
            usage_by_call.push(body["usage"].clone());
            for (index, key) in ["prompt_tokens", "completion_tokens", "total_tokens"].iter().enumerate() {
                usage[index] = usage[index].zip(body["usage"][key].as_u64()).map(|(sum, n)| sum.saturating_add(n));
            }
            let choice = body["choices"].get(0).context("Missing model choice")?;
            let message = &choice["message"];
            for part in [&body, choice, message] {
                anyhow::ensure!(part["error"].is_null() && part["refusal"].is_null(), "Tool-enabled model returned error or refusal");
            }
            if choice["finish_reason"] == "stop" {
                anyhow::ensure!(message["tool_calls"].is_null() || message["tool_calls"].as_array().is_some_and(Vec::is_empty), "Unexpected final tool calls");
                let empty = match message.get("content") {
                    None | Some(Value::Null) => true,
                    Some(Value::String(content)) => content.trim().is_empty(),
                    _ => anyhow::bail!("Invalid final model content type"),
                };
                if empty && finalization_continuations == 0 && traces.iter().any(|trace: &Value| trace["status"] == "success") {
                    finalization_continuations = 1;
                    payload["tool_choice"] = json!("none");
                    payload["messages"].as_array_mut().unwrap().push(json!({"role":"user","content":TOOL_FINALIZATION_PROMPT}));
                    tracing::info!(%flow_id, finalization_continuations, usage_by_call = %json!(usage_by_call), "Finalizing empty answer from existing tool results");
                    continue;
                }
                anyhow::ensure!(!empty, "Missing final model content");
                body["usage"] = json!({"prompt_tokens":usage[0],"completion_tokens":usage[1],"total_tokens":usage[2]});
                body["_private_tool_metadata"] = json!({"flow_id":flow_id,"traces":traces,"usage":body["usage"],"usage_by_call":usage_by_call,"finalization_continuations":finalization_continuations});
                tracing::info!(tool_flow = %body["_private_tool_metadata"], "Essential tool flow completed");
                return Ok(body);
            }
            anyhow::ensure!(finalization_continuations == 0, "Unexpected non-final completion during finalization; tools not executed");
            anyhow::ensure!(choice["finish_reason"] == "tool_calls", "Tool-enabled model did not finish with stop or tool_calls");
            anyhow::ensure!(round < gateway.max_rounds, "Essential tool round limit exceeded");
            let calls = message["tool_calls"].as_array().filter(|v| !v.is_empty()).context("Missing tool calls")?;
            anyhow::ensure!(message["role"] == "assistant", "Invalid tool-calling message role");
            anyhow::ensure!(used + calls.len() <= gateway.max_calls, "Essential tool call limit exceeded");
            let mut validated = Vec::new();
            // Validate the whole batch before any gateway side effect.
            for call in calls {
                anyhow::ensure!(call["type"] == "function", "Invalid tool call type");
                let id = call["id"].as_str().filter(|v| !v.is_empty() && v.len() <= 128).context("Invalid tool call ID")?;
                anyhow::ensure!(seen.insert(id.to_owned()), "Repeated tool call ID");
                let name = call["function"]["name"].as_str().filter(|v| !v.is_empty() && v.len() <= 64).context("Missing or invalid tool name")?;
                let raw = call["function"]["arguments"].as_str().filter(|v| v.len() <= 8192).context("Invalid tool arguments")?;
                let arguments: Result<Value> = serde_json::from_str(raw).context("Invalid tool arguments JSON")
                    .and_then(|arguments| { validate_arguments(name, &arguments)?; Ok(arguments) });
                validated.push((id.to_owned(), name.to_owned(), arguments));
            }
            let mut continuation = message.clone();
            if is_qwen_model(payload["model"].as_str().unwrap_or_default()) {
                // Qwen replays tool reasoning when echoed; preserve all other
                // fields and leave DeepSeek's required reasoning continuation intact.
                continuation.as_object_mut().unwrap().remove("reasoning_content");
            }
            payload["messages"].as_array_mut().unwrap().push(continuation);
            for (id, name, arguments) in validated {
                let result = match arguments {
                    Ok(arguments) => gateway.invoke(&name, &arguments).await?,
                    Err(error) => json!({"status":"invalid_request","error":error.to_string()}),
                };
                let trace = json!({"tool_call_id":id,"tool_name":name,"tool_version":result["tool_version"],"trace_id":result["trace_id"],"status":result["status"]});
                tracing::info!(%flow_id, tool_trace = %trace, "Essential tool invocation completed");
                traces.push(trace);
                payload["messages"].as_array_mut().unwrap().push(json!({"role":"tool","tool_call_id":id,"content":result.to_string()}));
                used += 1;
            }
            if round + 1 == gateway.max_rounds || used == gateway.max_calls {
                payload["tool_choice"] = json!("none");
            }
        }
        anyhow::bail!("Missing final tool-enabled completion")
    }).await.context("Essential tool flow timed out")?
}

// Compatibility only: this never selects or changes the configured model route.
fn is_qwen_model(model: &str) -> bool {
    let model = model.trim().to_ascii_lowercase();
    let name = if let Some(uri) = model.strip_prefix("gpt://") {
        // Yandex: gpt://folder/model[/version].
        uri.split('/').nth(1).unwrap_or_default()
    } else {
        model.rsplit('/').next().unwrap_or_default()
    };
    name.starts_with("qwen")
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use axum::{
        http::{HeaderMap, StatusCode},
        routing::post,
        Json, Router,
    };
    use std::sync::{Arc, Mutex};

    type Seen = Arc<Mutex<Vec<Value>>>;
    pub(crate) struct Mock {
        pub(crate) url: String,
        pub(crate) gateway: ToolGateway,
        pub(crate) model_requests: Seen,
        pub(crate) tool_requests: Seen,
        server: tokio::task::JoinHandle<()>,
    }
    impl Drop for Mock {
        fn drop(&mut self) {
            self.server.abort();
        }
    }

    pub(crate) async fn mock(
        responses: Vec<Value>,
        status: StatusCode,
        tool_result: Value,
    ) -> Mock {
        let model_requests: Seen = Arc::default();
        let tool_requests: Seen = Arc::default();
        let seen_model = model_requests.clone();
        let seen_tool = tool_requests.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let router = Router::new()
            .route(
                "/chat/completions",
                post(move |headers: HeaderMap, Json(body): Json<Value>| {
                    assert_eq!(headers["authorization"], "Bearer model-secret");
                    let mut requests = seen_model.lock().unwrap();
                    let mut response = responses
                        .get(requests.len())
                        .cloned()
                        .unwrap_or(json!({"error":"Unexpected extra request"}));
                    requests.push(body);
                    let status = response
                        .as_object_mut()
                        .unwrap()
                        .remove("_http_status")
                        .and_then(|v| v.as_u64())
                        .map(|v| StatusCode::from_u16(v as u16).unwrap())
                        .unwrap_or(StatusCode::OK);
                    let delay = response
                        .as_object_mut()
                        .unwrap()
                        .remove("_delay_ms")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    async move {
                        if delay > 0 {
                            tokio::time::sleep(Duration::from_millis(delay)).await;
                        }
                        (status, Json(response))
                    }
                }),
            )
            .route(
                "/invoke",
                post(move |headers: HeaderMap, Json(body): Json<Value>| {
                    assert_eq!(
                        headers["authorization"],
                        "Bearer gateway-test-secret-32-characters-long"
                    );
                    seen_tool.lock().unwrap().push(body.clone());
                    let mut result = tool_result.clone();
                    let fields = result.as_object_mut().unwrap();
                    fields.entry("tool_name").or_insert(body["tool"].clone());
                    fields.entry("tool_version").or_insert(json!(match body["tool"]
                        .as_str()
                        .unwrap()
                    {
                        "calculator" => "scientific-calculator-v2",
                        "sympy" => "ast-sympy-v2",
                        _ => "reference-db-private-v1",
                    }));
                    fields.entry("arguments").or_insert(body["arguments"].clone());
                    async move { (status, Json(result)) }
                }),
            );
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        Mock {
            gateway: ToolGateway::new(&url, "gateway-test-secret-32-characters-long").unwrap(),
            url,
            model_requests,
            tool_requests,
            server,
        }
    }

    fn final_body() -> Value {
        json!({"choices":[{"finish_reason":"stop","message":{"role":"assistant","content":"{\"total_score\":5}"}}],"usage":{"prompt_tokens":10,"completion_tokens":2,"total_tokens":12}})
    }
    pub(crate) fn empty_final_body() -> Value {
        let mut body = final_body();
        body["choices"][0]["message"]["content"] = json!("  ");
        body
    }
    pub(crate) fn tool_body(name: &str, arguments: Value, id: &str) -> Value {
        json!({"choices":[{"finish_reason":"tool_calls","message":{"role":"assistant","content":null,"tool_calls":[{"id":id,"type":"function","function":{"name":name,"arguments":arguments.to_string()}}]}}],"usage":{"prompt_tokens":20,"completion_tokens":3,"total_tokens":23}})
    }
    pub(crate) fn success() -> Value {
        json!({"status":"success","normalized_result":{"value":"4","provenance":"internal source"},"trace_id":"trace-1"})
    }
    fn payload() -> Value {
        json!({"model":"published-qwen","messages":[{"role":"system","content":"Practice: one hint, not the full reference"},{"role":"user","content":"Check 2+2"}],"response_format":{"type":"json_schema","json_schema":{"name":"grading","strict":true,"schema":{"type":"object"}}}})
    }
    async fn run(mock: &Mock) -> Result<Value> {
        complete(
            &Client::new(),
            &format!("{}/chat/completions", mock.url),
            "model-secret",
            payload(),
            &mock.gateway,
        )
        .await
    }

    #[tokio::test]
    async fn tool_result_continues_same_model_schema_and_aggregates_usage() {
        let mock = mock(
            vec![tool_body("calculator", json!({"expression":"2+2"}), "call-1"), final_body()],
            StatusCode::OK,
            success(),
        )
        .await;
        let result = run(&mock).await.unwrap();
        assert_eq!(result["usage"]["total_tokens"], 35);
        assert_eq!(result["usage"]["prompt_tokens"], 30);
        assert_eq!(
            mock.tool_requests.lock().unwrap()[0],
            json!({"tool":"calculator","arguments":{"expression":"2+2"}})
        );
        let requests = mock.model_requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        for request in requests.iter() {
            assert_eq!(request["model"], "published-qwen");
            assert_eq!(request["response_format"], payload()["response_format"]);
            assert_eq!(request["tools"].as_array().unwrap().len(), 3);
            assert!(request["messages"][0]["content"]
                .as_str()
                .unwrap()
                .contains(essential_tools_instruction()));
            assert!(request["messages"][0]["content"]
                .as_str()
                .unwrap()
                .contains("not the full reference"));
        }
        assert_eq!(requests[1]["messages"][2]["tool_calls"][0]["id"], "call-1");
        assert_eq!(requests[1]["messages"][3]["role"], "tool");
        assert_eq!(requests[1]["messages"][3]["tool_call_id"], "call-1");
        assert!(requests[1].to_string().contains("internal source"));
        assert_eq!(result["_private_tool_metadata"]["traces"][0]["trace_id"], "trace-1");
        assert!(!result["choices"].to_string().contains("trace-1"));
    }

    #[tokio::test]
    async fn tools_are_optional_and_final_completion_is_not_repeated() {
        let mock = mock(vec![final_body()], StatusCode::OK, success()).await;
        assert!(run(&mock).await.is_ok());
        assert_eq!(mock.model_requests.lock().unwrap().len(), 1);
        assert!(mock.tool_requests.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn configured_output_limit_is_exact_generic_and_preserves_explicit_payload() {
        for (model, key, explicit, expected) in [
            ("gpt://folder/qwen3/latest", Some("gpt://folder/qwen3/latest"), None, Some(81920)),
            ("gpt://folder/qwen3/latest", Some("qwen3"), None, None),
            ("gpt://folder/qwen3/latest", Some("gpt://other/qwen3/latest"), None, None),
            ("gpt://folder/qwen3/latest", Some("gpt://folder/Qwen3/latest"), None, None),
            ("gpt://folder/qwen3/latest", None, None, None),
            (
                "gpt://folder/qwen3/latest",
                Some("gpt://folder/qwen3/latest"),
                Some(1234),
                Some(1234),
            ),
            ("deepseek-v4-pro", Some("deepseek-v4-pro"), None, Some(81920)),
            ("deepseek-v4-pro", Some("gpt://folder/qwen3/latest"), None, None),
        ] {
            let mut fixture = mock(
                vec![
                    tool_body("calculator", json!({"expression":"2+2"}), "calc"),
                    empty_final_body(),
                    final_body(),
                ],
                StatusCode::OK,
                success(),
            )
            .await;
            if let Some(key) = key {
                fixture.gateway.max_output_tokens_by_model.insert(key.into(), 81920);
            }
            let mut request = payload();
            request["model"] = json!(model);
            request["temperature"] = json!(0.15);
            request["thinking"] = json!({"type":"enabled"});
            if let Some(value) = explicit {
                request["max_tokens"] = json!(value);
            }
            complete(
                &Client::new(),
                &format!("{}/chat/completions", fixture.url),
                "model-secret",
                request,
                &fixture.gateway,
            )
            .await
            .unwrap();
            let requests = fixture.model_requests.lock().unwrap();
            assert_eq!(requests.len(), 3);
            for request in requests.iter() {
                assert_eq!(
                    request.get("max_tokens").cloned(),
                    expected.map(|n| json!(n)),
                    "{model} / {key:?}"
                );
                assert_eq!(request["model"], model);
                assert_eq!(request["temperature"], 0.15);
                assert_eq!(request["thinking"], json!({"type":"enabled"}));
                assert_eq!(request["response_format"], payload()["response_format"]);
            }
        }
    }

    #[tokio::test]
    async fn empty_final_after_success_gets_one_same_context_finisher_with_usage() {
        let mut fixture = mock(
            vec![
                tool_body("calculator", json!({"expression":"2+2"}), "calc"),
                empty_final_body(),
                final_body(),
            ],
            StatusCode::OK,
            success(),
        )
        .await;
        fixture.gateway.max_rounds = 1; // A finisher also works at the normal round boundary.
        let result = run(&fixture).await.unwrap();
        assert_eq!(result["usage"]["total_tokens"], 47);
        assert_eq!(result["_private_tool_metadata"]["finalization_continuations"], 1);
        assert_eq!(
            result["_private_tool_metadata"]["usage_by_call"],
            json!([{"prompt_tokens":20,"completion_tokens":3,"total_tokens":23}, empty_final_body()["usage"], final_body()["usage"]])
        );
        let requests = fixture.model_requests.lock().unwrap();
        assert_eq!(requests.len(), 3);
        assert_eq!(fixture.tool_requests.lock().unwrap().len(), 1);
        let previous = requests[1]["messages"].as_array().unwrap();
        let finishing = requests[2]["messages"].as_array().unwrap();
        assert_eq!(&finishing[..previous.len()], previous.as_slice());
        assert_eq!(finishing.len(), previous.len() + 1);
        assert_eq!(
            finishing.last().unwrap(),
            &json!({"role":"user","content":TOOL_FINALIZATION_PROMPT})
        );
        assert_eq!(requests[2]["tool_choice"], "none");
        for key in ["model", "tools", "response_format"] {
            assert_eq!(requests[2][key], requests[1][key]);
        }
        assert!(!result["choices"].to_string().contains("finalization_continuations"));
    }

    #[tokio::test]
    async fn finisher_never_repeats_or_executes_more_tools() {
        let mut refused = empty_final_body();
        refused["choices"][0]["message"]["refusal"] = json!("No");
        let mut http_error = empty_final_body();
        http_error["_http_status"] = json!(503);
        for final_response in [
            empty_final_body(),
            tool_body("calculator", json!({"expression":"3+3"}), "extra"),
            refused,
            http_error,
        ] {
            let fixture = mock(
                vec![
                    tool_body("calculator", json!({"expression":"2+2"}), "calc"),
                    empty_final_body(),
                    final_response,
                ],
                StatusCode::OK,
                success(),
            )
            .await;
            assert!(run(&fixture).await.is_err());
            assert_eq!(fixture.model_requests.lock().unwrap().len(), 3);
            assert_eq!(fixture.tool_requests.lock().unwrap().len(), 1);
        }
    }

    #[tokio::test]
    async fn empty_initial_errors_and_unsuccessful_tools_do_not_get_finisher() {
        let fixture = mock(vec![empty_final_body()], StatusCode::OK, success()).await;
        assert!(run(&fixture).await.is_err());
        assert_eq!(fixture.model_requests.lock().unwrap().len(), 1);
        for failure in ["refusal", "http", "tool_error"] {
            let mut empty = empty_final_body();
            let mut tool = success();
            match failure {
                "refusal" => empty["choices"][0]["message"]["refusal"] = json!("No"),
                "http" => empty["_http_status"] = json!(503),
                _ => {
                    tool["status"] = json!("error");
                    tool["error"] = json!("Bad expression");
                }
            }
            let fixture = mock(
                vec![tool_body("calculator", json!({"expression":"2+2"}), "calc"), empty],
                StatusCode::OK,
                tool,
            )
            .await;
            assert!(run(&fixture).await.is_err());
            assert_eq!(fixture.model_requests.lock().unwrap().len(), 2);
            assert_eq!(fixture.tool_requests.lock().unwrap().len(), 1);
        }
    }

    #[tokio::test]
    async fn finisher_uses_remaining_original_deadline() {
        let mut empty = empty_final_body();
        empty["_delay_ms"] = json!(100);
        let mut final_response = final_body();
        final_response["_delay_ms"] = json!(150);
        let mut fixture = mock(
            vec![
                tool_body("calculator", json!({"expression":"2+2"}), "calc"),
                empty,
                final_response,
            ],
            StatusCode::OK,
            success(),
        )
        .await;
        fixture.gateway.flow_timeout = Duration::from_millis(200);
        assert!(run(&fixture).await.unwrap_err().to_string().contains("timed out"));
        assert_eq!(fixture.model_requests.lock().unwrap().len(), 3);
        assert_eq!(fixture.tool_requests.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn qwen_omits_only_replayed_reasoning_deepseek_keeps_it() {
        for (model, strip_reasoning) in [
            ("qwen3-235b", true),
            ("Qwen/Qwen3.5-2B", true),
            ("gpt://folder/qwen3-235b/latest", true),
            ("gpt://folder/qwen3-235b", true),
            ("deepseek-v4-pro", false),
            ("gpt://qwen-folder/deepseek-v4-pro/latest", false),
            ("other-qwen-model", false),
        ] {
            let mut call = tool_body("calculator", json!({"expression":"2+2"}), "calc");
            call["choices"][0]["message"]["reasoning_content"] = json!("Internal tool reasoning");
            let mut final_response = final_body();
            final_response["choices"][0]["message"]["reasoning_content"] =
                json!("Final reasoning unchanged");
            let fixture =
                mock(vec![call.clone(), final_response.clone()], StatusCode::OK, success()).await;
            let mut request = payload();
            request["model"] = json!(model);
            request["thinking"] = json!({"type":"enabled"});
            let result = complete(
                &Client::new(),
                &format!("{}/chat/completions", fixture.url),
                "model-secret",
                request,
                &fixture.gateway,
            )
            .await
            .unwrap();
            assert_eq!(result["choices"], final_response["choices"]);
            let requests = fixture.model_requests.lock().unwrap();
            assert_eq!(requests.len(), 2);
            assert_eq!(fixture.tool_requests.lock().unwrap().len(), 1);
            let mut expected = call["choices"][0]["message"].clone();
            if strip_reasoning {
                expected.as_object_mut().unwrap().remove("reasoning_content");
            }
            assert_eq!(requests[1]["messages"][2], expected, "{model}");
            let tool_result: Value =
                serde_json::from_str(requests[1]["messages"][3]["content"].as_str().unwrap())
                    .unwrap();
            assert_eq!(tool_result["normalized_result"], success()["normalized_result"]);
            assert_eq!(tool_result["trace_id"], "trace-1");
            let mut first = requests[0].clone();
            let mut second = requests[1].clone();
            first.as_object_mut().unwrap().remove("messages");
            second.as_object_mut().unwrap().remove("messages");
            assert_eq!(first, second);
            assert_eq!(second["model"], model);
            assert_eq!(second["thinking"], json!({"type":"enabled"}));
            assert_eq!(second["tool_choice"], "auto");
            assert_eq!(second["response_format"], payload()["response_format"]);
        }
    }

    #[tokio::test]
    async fn reference_provenance_and_inconclusive_identity_are_preserved_as_context() {
        for (name, arguments, normalized) in [
            (
                "sympy",
                json!({"operation":"identity","left":"sqrt(x**2)","right":"x","symbols":["x"]}),
                json!({"result":false,"interpretation":"not_proven","assumptions":{"x":"real"}}),
            ),
            (
                "reference_db",
                json!({"reference_id":"universal_gas_constant_si"}),
                json!({"database_sha256":"synthetic-test-hash","record":{"source":"test-only provenance","units":"J mol^-1 K^-1"},"review_warnings":["Check conditions"]}),
            ),
        ] {
            let fixture = mock(vec![tool_body(name, arguments, "verify"), final_body()], StatusCode::OK,
                json!({"status":"success","normalized_result":normalized,"trace_id":"internal-qa-trace"})).await;
            let final_result = run(&fixture).await.unwrap();
            assert_eq!(final_result["choices"], final_body()["choices"]);
            let requests = fixture.model_requests.lock().unwrap();
            let tool_result: Value =
                serde_json::from_str(requests[1]["messages"][3]["content"].as_str().unwrap())
                    .unwrap();
            assert_eq!(tool_result["normalized_result"], normalized);
            assert!(!final_result["choices"].to_string().contains("internal-qa-trace"));
        }
    }

    #[tokio::test]
    async fn calculator_units_remain_explicitly_not_inferred() {
        let normalized = json!({"value":"4","unit":null,"unit_inference":"not_performed",
            "unit_note":"Scalar arithmetic only; caller must explicitly convert and verify units.","precision":34});
        let fixture = mock(
            vec![tool_body("calculator", json!({"expression":"2+2"}), "calc"), final_body()],
            StatusCode::OK,
            json!({"status":"success","tool_version":"scientific-calculator-v3","normalized_result":normalized,"trace_id":"units-test"}),
        )
        .await;
        let result = run(&fixture).await.unwrap();
        let requests = fixture.model_requests.lock().unwrap();
        let tool_result: Value =
            serde_json::from_str(requests[1]["messages"][3]["content"].as_str().unwrap()).unwrap();
        assert_eq!(tool_result["normalized_result"], normalized);
        assert!(tool_result["normalized_result"].get("unit").unwrap().is_null());
        assert_eq!(tool_result["normalized_result"]["unit_inference"], "not_performed");
        assert_eq!(tool_result["tool_version"], "scientific-calculator-v3");
        assert_eq!(
            result["_private_tool_metadata"]["traces"][0]["tool_version"],
            "scientific-calculator-v3"
        );
        assert!(!tool_result.to_string().contains("dimensionless"));
        assert_eq!(result["choices"], final_body()["choices"]);
    }

    #[tokio::test]
    async fn validation_unknown_tool_and_computation_errors_can_be_corrected() {
        for (name, arguments, status, expected_calls) in [
            ("unknown", json!({"url":"https://not-a-gateway"}), "success", 1),
            ("calculator", json!({"expression":42}), "success", 1),
            ("calculator", json!({"expression":"1/0"}), "error", 2),
            ("calculator", json!({"expression":"bad"}), "invalid_request", 2),
        ] {
            let mock = mock(vec![tool_body(name, arguments, "bad"), tool_body("calculator", json!({"expression":"2+2"}), "corrected"), final_body()], StatusCode::OK,
                json!({"status":status,"normalized_result":4,"trace_id":"t","error":"Invalid expression"})).await;
            assert!(run(&mock).await.is_ok());
            assert_eq!(mock.tool_requests.lock().unwrap().len(), expected_calls);
            let requests = mock.model_requests.lock().unwrap();
            assert_eq!(requests.len(), 3);
            let error: Value =
                serde_json::from_str(requests[1]["messages"][3]["content"].as_str().unwrap())
                    .unwrap();
            assert!(matches!(error["status"].as_str(), Some("invalid_request" | "error")));
            assert_eq!(requests[2]["messages"][3], requests[1]["messages"][3]);
        }
    }

    #[tokio::test]
    async fn transport_and_invalid_final_fail_without_retry() {
        for status in [
            StatusCode::UNAUTHORIZED,
            StatusCode::SERVICE_UNAVAILABLE,
            StatusCode::TEMPORARY_REDIRECT,
        ] {
            let mock = mock(
                vec![tool_body("calculator", json!({"expression":"2+2"}), "call")],
                status,
                success(),
            )
            .await;
            assert!(run(&mock).await.unwrap_err().to_string().contains("HTTP"));
            assert_eq!(mock.model_requests.lock().unwrap().len(), 1);
            assert_eq!(mock.tool_requests.lock().unwrap().len(), 1);
        }
        for change in ["length", "missing", "refusal", "error"] {
            let mut response = final_body();
            match change {
                "length" => response["choices"][0]["finish_reason"] = json!("length"),
                "missing" => {
                    response["choices"][0].as_object_mut().unwrap().remove("finish_reason");
                }
                "refusal" => response["choices"][0]["message"]["refusal"] = json!("No"),
                _ => response["error"] = json!("No"),
            }
            let mock = mock(vec![response], StatusCode::OK, success()).await;
            assert!(run(&mock).await.is_err());
            assert_eq!(mock.model_requests.lock().unwrap().len(), 1);
            assert!(mock.tool_requests.lock().unwrap().is_empty());
        }
    }

    #[tokio::test]
    async fn configured_limits_force_final_then_stop_without_extra_tools() {
        let mut mock = mock(
            vec![
                tool_body("calculator", json!({"expression":"2+2"}), "one"),
                tool_body("calculator", json!({"expression":"3+3"}), "two"),
            ],
            StatusCode::OK,
            success(),
        )
        .await;
        mock.gateway.max_rounds = 1;
        mock.gateway.max_calls = 1;
        assert!(run(&mock).await.unwrap_err().to_string().contains("round limit"));
        assert_eq!(mock.model_requests.lock().unwrap()[1]["tool_choice"], "none");
        assert_eq!(mock.tool_requests.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn missing_usage_is_unknown_and_mismatched_envelopes_fail_closed() {
        let mut response = final_body();
        response.as_object_mut().unwrap().remove("usage");
        let fixture = mock(
            vec![tool_body("calculator", json!({"expression":"2+2"}), "one"), response],
            StatusCode::OK,
            success(),
        )
        .await;
        assert_eq!(run(&fixture).await.unwrap()["usage"]["total_tokens"], Value::Null);
        for status in ["success", "error"] {
            for (key, value) in [
                ("tool_name", json!("sympy")),
                ("tool_version", json!("")),
                ("arguments", json!({"expression":"9+9"})),
                ("trace_id", Value::Null),
            ] {
                let mut result = success();
                result["status"] = json!(status);
                result["error"] = json!("Invalid expression");
                result[key] = value;
                let fixture = mock(
                    vec![tool_body("calculator", json!({"expression":"2+2"}), "one")],
                    StatusCode::OK,
                    result,
                )
                .await;
                assert!(run(&fixture).await.unwrap_err().to_string().contains("envelope"));
                assert_eq!(fixture.model_requests.lock().unwrap().len(), 1);
            }
        }
    }

    #[tokio::test]
    async fn whole_flow_deadline_cancels_without_retry() {
        let mut response = final_body();
        response["_delay_ms"] = json!(100);
        let mut fixture = mock(vec![response], StatusCode::OK, success()).await;
        fixture.gateway.flow_timeout = Duration::from_millis(20);
        assert!(run(&fixture).await.unwrap_err().to_string().contains("timed out"));
        assert!(fixture.model_requests.lock().unwrap().len() <= 1);
        assert!(fixture.tool_requests.lock().unwrap().is_empty());
    }

    #[test]
    fn tool_definitions_arguments_and_gateway_are_restricted() {
        assert_eq!(definitions().as_array().unwrap().len(), 3);
        assert!(validate_arguments(
            "sympy",
            &json!({"operation":"identity","left":"x+x","right":"2*x","symbols":["x"]})
        )
        .is_ok());
        assert!(validate_arguments("sympy", &json!({"operation":"exec","expression":"x"})).is_err());
        assert!(validate_arguments(
            "calculator",
            &json!({"expression":"2+2","url":"http://other"})
        )
        .is_err());
        for url in [
            "",
            "file:///tmp/tools",
            "http://user:pass@localhost",
            "http://localhost?url=evil",
            "http://localhost/#evil",
        ] {
            assert!(ToolGateway::new(url, "secret").is_err());
        }
        assert!(ToolGateway::new("http://localhost", "").is_err());
    }
}
