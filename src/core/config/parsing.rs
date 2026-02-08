use std::env;

use super::types::{ConfigError, Environment};

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
        return Ok(parsed);
    }

    let items: Vec<String> = raw
        .split(',')
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect();

    if items.is_empty() {
        return Ok(default_cors_origins());
    }

    Ok(items)
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
}
