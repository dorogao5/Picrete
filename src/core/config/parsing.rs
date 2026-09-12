use std::env;

use axum::http::Uri;

use super::types::{ConfigError, CourseContextMode, Environment};

const DEFAULT_CORS_ORIGINS: &[&str] = &[
    "http://localhost:5173",
    "http://localhost:3000",
    "http://localhost:8080",
    "https://picrete.com",
    "https://www.picrete.com",
    "https://picrete.com:443",
    "https://www.picrete.com:443",
];

pub(super) fn env_optional(key: &str) -> Option<String> {
    env::var(key).ok().map(|value| value.trim().to_string()).filter(|value| !value.is_empty())
}

pub(super) fn env_or_default(key: &str, default: &str) -> String {
    env_optional(key).unwrap_or_else(|| default.to_string())
}

pub(super) fn parse_u16(field: &'static str, value: String) -> Result<u16, ConfigError> {
    value.parse::<u16>().map_err(|_| ConfigError::InvalidValue { field, value })
}

pub(super) fn parse_u32(field: &'static str, value: String) -> Result<u32, ConfigError> {
    value.parse::<u32>().map_err(|_| ConfigError::InvalidValue { field, value })
}

pub(super) fn parse_u64(field: &'static str, value: String) -> Result<u64, ConfigError> {
    value.parse::<u64>().map_err(|_| ConfigError::InvalidValue { field, value })
}

pub(super) fn parse_max_output_tokens_by_model(
    raw: Option<String>,
) -> Result<std::collections::HashMap<String, u64>, ConfigError> {
    let field = "LLM_MAX_OUTPUT_TOKENS_BY_MODEL";
    let values = parse_exact_model_map::<u64>(raw, field)?;
    if values.values().any(|v| *v == 0) {
        return Err(invalid_model_map(field));
    }
    Ok(values)
}

pub(super) fn parse_sampling_by_model(
    raw: Option<String>,
) -> Result<std::collections::HashMap<String, super::types::ModelSampling>, ConfigError> {
    let field = "LLM_SAMPLING_BY_MODEL";
    let values = parse_exact_model_map::<super::types::ModelSampling>(raw, field)?;
    for v in values.values() {
        if v.temperature.is_some_and(|n| !(0.0..=2.0).contains(&n))
            || v.top_p.is_some_and(|n| !(0.0..=1.0).contains(&n))
            || v.presence_penalty.is_some_and(|n| !(-2.0..=2.0).contains(&n))
        {
            return Err(invalid_model_map(field));
        }
    }
    Ok(values)
}

fn invalid_model_map(field: &'static str) -> ConfigError {
    ConfigError::InvalidValue { field, value: "<invalid model settings map>".into() }
}

fn parse_exact_model_map<T: serde::de::DeserializeOwned>(
    raw: Option<String>,
    field: &'static str,
) -> Result<std::collections::HashMap<String, T>, ConfigError> {
    use serde::de::{Error, MapAccess, Visitor};
    use std::collections::HashMap;
    struct ExactModelMap<T>(std::marker::PhantomData<T>);
    impl<'de, T: serde::Deserialize<'de>> Visitor<'de> for ExactModelMap<T> {
        type Value = HashMap<String, T>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("an exact model ID settings map")
        }
        fn visit_map<M: MapAccess<'de>>(self, mut input: M) -> Result<Self::Value, M::Error> {
            let mut values = HashMap::new();
            while let Some((key, value)) = input.next_entry::<String, T>()? {
                if key.is_empty()
                    || key.len() > 2048
                    || key.chars().any(|c| c.is_whitespace() || c.is_control())
                    || values.len() >= 256
                    || values.insert(key, value).is_some()
                {
                    return Err(M::Error::custom("invalid or duplicate model settings"));
                }
            }
            Ok(values)
        }
    }
    let Some(raw) = raw.filter(|s| !s.trim().is_empty()) else {
        return Ok(HashMap::new());
    };
    let invalid = || invalid_model_map(field);
    if raw.len() > 65536 {
        return Err(invalid());
    }
    let mut parser = serde_json::Deserializer::from_str(&raw);
    let values =
        serde::Deserializer::deserialize_map(&mut parser, ExactModelMap(std::marker::PhantomData))
            .map_err(|_| invalid())?;
    parser.end().map_err(|_| invalid())?;
    Ok(values)
}

pub(super) fn parse_cors_origins(value: Option<String>) -> Result<Vec<String>, ConfigError> {
    let Some(raw) = value else {
        return Ok(default_cors_origins());
    };

    if raw.trim().is_empty() {
        return Ok(default_cors_origins());
    }

    if raw.trim_start().starts_with('[') {
        let parsed: Vec<String> =
            serde_json::from_str(&raw).map_err(|_| ConfigError::InvalidCors(raw.clone()))?;
        if parsed.is_empty() {
            return Ok(default_cors_origins());
        }
        return validate_cors_origins(parsed, &raw);
    }

    let items: Vec<String> = raw
        .split(',')
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect();

    if items.is_empty() {
        return Ok(default_cors_origins());
    }

    validate_cors_origins(items, &raw)
}

fn validate_cors_origins(origins: Vec<String>, raw: &str) -> Result<Vec<String>, ConfigError> {
    for origin in &origins {
        let uri = origin.parse::<Uri>().map_err(|_| ConfigError::InvalidCors(raw.to_string()))?;
        let scheme = uri.scheme_str();
        let authority = uri.authority().map(|value| value.as_str());
        let has_forbidden_suffix =
            uri.path() != "" && uri.path() != "/" || uri.query().is_some() || origin.ends_with('/');
        if !matches!(scheme, Some("http" | "https"))
            || authority.is_none()
            || authority.is_some_and(|value| value.contains('@'))
            || origin == "*"
            || has_forbidden_suffix
        {
            return Err(ConfigError::InvalidCors(raw.to_string()));
        }
    }
    Ok(origins)
}

pub(super) fn parse_string_list(value: Option<String>, defaults: &[&str]) -> Vec<String> {
    match value {
        Some(raw) => raw
            .split(',')
            .map(|item| item.trim().to_ascii_lowercase())
            .filter(|item| !item.is_empty())
            .collect(),
        None => defaults.iter().map(|item| item.to_string()).collect(),
    }
}

pub(super) fn parse_bool(value: &str) -> bool {
    matches!(value, "1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON")
}

pub(super) fn parse_environment(value: Option<String>) -> Environment {
    match value.as_deref().map(|item| item.to_lowercase()) {
        Some(ref val) if val == "production" || val == "prod" => Environment::Production,
        Some(ref val) if val == "staging" => Environment::Staging,
        Some(ref val) if val == "test" || val == "testing" => Environment::Test,
        _ => Environment::Development,
    }
}

pub(super) fn parse_course_context_mode(value: Option<String>) -> CourseContextMode {
    match value.as_deref().map(|item| item.to_lowercase()) {
        Some(ref raw) if raw == "header" => CourseContextMode::Header,
        _ => CourseContextMode::Route,
    }
}

pub(super) fn is_supported_image_extension(extension: &str) -> bool {
    matches!(extension, "jpg" | "jpeg" | "png" | "webp" | "gif")
}

fn default_cors_origins() -> Vec<String> {
    DEFAULT_CORS_ORIGINS.iter().map(|item| item.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sampling_map_validates_types_ranges_duplicates_and_unknown_fields() {
        assert!(parse_sampling_by_model(None).unwrap().is_empty());
        assert!(parse_sampling_by_model(Some("{}".into())).unwrap().is_empty());
        assert!(parse_sampling_by_model(Some(r#"{"model":{}}"#.into())).unwrap()["model"]
            .temperature
            .is_none());
        for (temperature, top_p, presence) in [(0.0, 0.0, -2.0), (2.0, 1.0, 2.0), (0.6, 0.95, 0.0)]
        {
            let map = parse_sampling_by_model(Some(serde_json::json!({"gpt://folder/model":{"temperature":temperature,"top_p":top_p,"presence_penalty":presence}}).to_string())).unwrap();
            assert_eq!(map["gpt://folder/model"].temperature, Some(temperature));
        }
        for raw in [
            "null",
            "[]",
            "{} {}",
            r#"{"model":null}"#,
            r#"{"model":0}"#,
            r#"{"model":{"temperature":true}}"#,
            r#"{"model":{"temperature":null}}"#,
            r#"{"model":{"temperature":"0.6"}}"#,
            r#"{"model":{"temperature":NaN}}"#,
            r#"{"model":{"temperature":1e400}}"#,
            r#"{"model":{"top_k":20}}"#,
            r#"{"model":{"temperature":1,"temperature":0.6}}"#,
            r#"{"model":{},"model":{}}"#,
            r#"{" model":{}}"#,
            r#"{"sensitive-model":{"top_p":false}}"#,
        ] {
            let error = parse_sampling_by_model(Some(raw.into())).unwrap_err();
            assert!(matches!(
                &error,
                ConfigError::InvalidValue { field: "LLM_SAMPLING_BY_MODEL", .. }
            ));
            assert!(!format!("{error:?}").contains("sensitive-model"));
        }
        for (field, number) in [
            ("temperature", -0.1),
            ("temperature", 2.1),
            ("top_p", -0.1),
            ("top_p", 1.1),
            ("presence_penalty", -2.1),
            ("presence_penalty", 2.1),
        ] {
            assert!(parse_sampling_by_model(Some(
                serde_json::json!({"model":{field:number}}).to_string()
            ))
            .is_err());
        }
    }

    #[test]
    fn output_token_map_accepts_only_unambiguous_positive_integer_limits() {
        assert!(parse_max_output_tokens_by_model(None).unwrap().is_empty());
        assert!(parse_max_output_tokens_by_model(Some("".into())).unwrap().is_empty());
        assert!(parse_max_output_tokens_by_model(Some("{}".into())).unwrap().is_empty());
        let parsed = parse_max_output_tokens_by_model(Some(
            r#"{"gpt://folder/qwen3/latest":81920,"gpt://folder/other":1}"#.into(),
        ))
        .unwrap();
        assert_eq!(parsed["gpt://folder/qwen3/latest"], 81920);
        assert_eq!(parsed["gpt://folder/other"], 1);
        for raw in [
            "[]",
            "null",
            "true",
            "broken-json",
            "{} {}",
            r#"{"model":0}"#,
            r#"{"model":-1}"#,
            r#"{"model":1.0}"#,
            r#"{"model":true}"#,
            r#"{"model":"81920"}"#,
            r#"{"model":null}"#,
            r#"{"model":18446744073709551616}"#,
            r#"{"model":1,"model":2}"#,
            r#"{"":1}"#,
            r#"{" model":1}"#,
            r#"{"model\n":1}"#,
            r#"{"sensitive-model-key":0}"#,
        ] {
            let error = parse_max_output_tokens_by_model(Some(raw.into())).unwrap_err();
            assert!(matches!(
                &error,
                ConfigError::InvalidValue { field: "LLM_MAX_OUTPUT_TOKENS_BY_MODEL", .. }
            ));
            assert!(!format!("{error:?}").contains("sensitive-model-key"));
        }
        assert!(parse_max_output_tokens_by_model(Some(" ".repeat(65536) + "{}")).is_err());
    }

    #[test]
    fn parse_cors_origins_json() {
        let raw = "[\"http://a\",\"http://b\"]".to_string();
        let parsed = parse_cors_origins(Some(raw)).expect("cors json");
        assert_eq!(parsed, vec!["http://a".to_string(), "http://b".to_string()]);
    }

    #[test]
    fn parse_cors_origins_csv() {
        let raw = "http://a, http://b".to_string();
        let parsed = parse_cors_origins(Some(raw)).expect("cors csv");
        assert_eq!(parsed, vec!["http://a".to_string(), "http://b".to_string()]);
    }

    #[test]
    fn parse_cors_origins_defaults_on_empty() {
        let parsed = parse_cors_origins(Some(" ".to_string())).expect("cors empty");
        assert_eq!(parsed, default_cors_origins());
    }

    #[test]
    fn parse_cors_origins_rejects_wildcards_paths_and_non_http_schemes() {
        for value in ["*", "https://example.com/path", "file://example.com"] {
            assert!(parse_cors_origins(Some(value.to_string())).is_err(), "accepted {value}");
        }
    }

    #[test]
    fn parse_bool_variants() {
        assert!(parse_bool("1"));
        assert!(parse_bool("true"));
        assert!(parse_bool("TRUE"));
        assert!(parse_bool("yes"));
        assert!(parse_bool("on"));
        assert!(!parse_bool("false"));
        assert!(!parse_bool("0"));
    }

    #[test]
    fn parse_environment_variants() {
        assert_eq!(parse_environment(Some("prod".to_string())), Environment::Production);
        assert_eq!(parse_environment(Some("production".to_string())), Environment::Production);
        assert_eq!(parse_environment(Some("staging".to_string())), Environment::Staging);
        assert_eq!(parse_environment(Some("testing".to_string())), Environment::Test);
        assert_eq!(parse_environment(None), Environment::Development);
    }

    #[test]
    fn parse_course_context_mode_variants() {
        assert_eq!(parse_course_context_mode(Some("route".to_string())), CourseContextMode::Route);
        assert_eq!(
            parse_course_context_mode(Some("header".to_string())),
            CourseContextMode::Header
        );
        assert_eq!(parse_course_context_mode(None), CourseContextMode::Route);
    }
}
