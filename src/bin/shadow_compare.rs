use std::collections::HashMap;
use std::env;
use std::fs;

use anyhow::{anyhow, Context, Result};
use reqwest::{Method, StatusCode};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
struct ShadowRequest {
    method: String,
    path: String,
    #[serde(default)]
    headers: HashMap<String, String>,
    #[serde(default)]
    body: Option<Value>,
}

#[derive(Debug)]
struct ShadowResponse {
    status: StatusCode,
    body: Vec<u8>,
    json: Option<Value>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let (requests_path, python_base, rust_base) = parse_args()?;

    let payload = fs::read_to_string(&requests_path)
        .with_context(|| format!("Failed to read {requests_path}"))?;
    let requests: Vec<ShadowRequest> = serde_json::from_str(&payload)
        .with_context(|| format!("Invalid JSON in {requests_path}"))?;

    let client = reqwest::Client::new();
    let mut failures = 0;

    for request in requests {
        let python = send_request(&client, &python_base, &request).await?;
        let rust = send_request(&client, &rust_base, &request).await?;

        if python.status != rust.status || python.json != rust.json || python.body != rust.body {
            failures += 1;
            eprintln!(
                "Mismatch for {} {}: python={} rust={}",
                request.method, request.path, python.status, rust.status
            );
            if python.json != rust.json {
                eprintln!("JSON diff: python={:?} rust={:?}", python.json, rust.json);
            } else if python.body != rust.body {
                eprintln!(
                    "Body diff: python={} bytes rust={} bytes",
                    python.body.len(),
                    rust.body.len()
                );
            }
        } else {
            println!("OK {} {}", request.method, request.path);
        }
    }

    if failures > 0 {
        Err(anyhow!("shadow compare failed for {failures} request(s)"))
    } else {
        Ok(())
    }
}

fn parse_args() -> Result<(String, String, String)> {
    let mut requests_path = env::var("PICRETE_SHADOW_REQUESTS")
        .unwrap_or_else(|_| "scripts/shadow_requests.json".to_string());
    let mut python_base = env::var("PICRETE_SHADOW_PYTHON_BASE")
        .unwrap_or_else(|_| "http://localhost:8000".to_string());
    let mut rust_base = env::var("PICRETE_SHADOW_RUST_BASE")
        .unwrap_or_else(|_| "http://localhost:8001".to_string());

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--requests" => {
                requests_path = args.next().ok_or_else(|| anyhow!("--requests missing value"))?;
            }
            "--python" => {
                python_base = args.next().ok_or_else(|| anyhow!("--python missing value"))?;
            }
            "--rust" => {
                rust_base = args.next().ok_or_else(|| anyhow!("--rust missing value"))?;
            }
            _ => return Err(anyhow!("Unknown argument: {arg}")),
        }
    }

    Ok((requests_path, python_base, rust_base))
}

async fn send_request(
    client: &reqwest::Client,
    base: &str,
    request: &ShadowRequest,
) -> Result<ShadowResponse> {
    let url = format!("{}{}", base.trim_end_matches('/'), request.path.as_str());
    let method = Method::from_bytes(request.method.as_bytes())?;

    let mut builder = client.request(method, url);
    for (key, value) in &request.headers {
        builder = builder.header(key, value);
    }

    if let Some(body) = &request.body {
        builder = builder.json(body);
    }

    let response = builder.send().await?;
    let status = response.status();
    let body = response.bytes().await?.to_vec();
    let json =
        if let Ok(value) = serde_json::from_slice::<Value>(&body) { Some(value) } else { None };

    Ok(ShadowResponse { status, body, json })
}
