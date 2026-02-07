use std::{env, fs, path::PathBuf};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::rngs::OsRng;
use rand::RngCore;
use thiserror::Error;

const DEFAULT_CORS_ORIGINS: &[&str] = &[
    "http://localhost:5173",
    "http://localhost:3000",
    "http://localhost:8080",
    "https://picrete.com",
    "https://www.picrete.com",
    "https://picrete.com:443",
    "https://www.picrete.com:443",
];

#[derive(Debug, Clone)]
pub(crate) struct Settings {
    server: ServerSettings,
    runtime: RuntimeSettings,
    api: ApiSettings,
    security: SecuritySettings,
    cors: CorsSettings,
    database: DatabaseSettings,
    redis: RedisSettings,
    ai: AiSettings,
    storage: StorageSettings,
    s3: S3Settings,
    exam: ExamSettings,
    admin: AdminSettings,
    telemetry: TelemetrySettings,
}

#[derive(Debug, Clone)]
pub(crate) struct ServerSettings {
    host: ServerHost,
    port: ServerPort,
}

#[derive(Debug, Clone)]
pub(crate) struct ApiSettings {
    pub(crate) project_name: String,
    pub(crate) version: String,
    pub(crate) api_v1_str: String,
    pub(crate) terms_version: String,
    pub(crate) privacy_version: String,
    pub(crate) pd_consent_version: String,
}

#[derive(Debug, Clone)]
pub(crate) struct SecuritySettings {
    pub(crate) secret_key: String,
    pub(crate) access_token_expire_minutes: u64,
    pub(crate) algorithm: String,
}

#[derive(Debug, Clone)]
pub(crate) struct CorsSettings {
    pub(crate) origins: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct DatabaseSettings {
    pub(crate) postgres_server: String,
    pub(crate) postgres_port: u16,
    pub(crate) postgres_user: String,
    pub(crate) postgres_password: String,
    pub(crate) postgres_db: String,
    pub(crate) database_url: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct RedisSettings {
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) db: u16,
    pub(crate) password: String,
}

#[derive(Debug, Clone)]
pub(crate) struct AiSettings {
    pub(crate) openai_api_key: String,
    pub(crate) openai_base_url: String,
    pub(crate) ai_model: String,
    pub(crate) ai_max_tokens: u32,
    pub(crate) ai_request_timeout: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct StorageSettings {
    pub(crate) max_upload_size_mb: u64,
    pub(crate) allowed_image_extensions: Vec<String>,
    pub(crate) max_images_per_submission: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct S3Settings {
    pub(crate) endpoint: String,
    pub(crate) access_key: String,
    pub(crate) secret_key: String,
    pub(crate) bucket: String,
    pub(crate) region: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ExamSettings {
    pub(crate) max_concurrent_exams: u64,
    pub(crate) auto_save_interval_seconds: u64,
    pub(crate) presigned_url_expire_minutes: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct AdminSettings {
    pub(crate) first_superuser_isu: String,
    pub(crate) first_superuser_password: String,
}

#[derive(Debug, Clone)]
pub(crate) struct TelemetrySettings {
    pub(crate) log_level: String,
    pub(crate) json: bool,
    pub(crate) prometheus_enabled: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct RuntimeSettings {
    pub(crate) environment: Environment,
    pub(crate) strict_config: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Environment {
    Development,
    Production,
    Staging,
    Test,
}

impl Environment {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Environment::Development => "development",
            Environment::Production => "production",
            Environment::Staging => "staging",
            Environment::Test => "test",
        }
    }

    fn is_production(self) -> bool {
        matches!(self, Environment::Production)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ServerHost(String);

#[derive(Debug, Clone, Copy)]
pub(crate) struct ServerPort(u16);

#[derive(Debug, Error)]
pub(crate) enum ConfigError {
    #[error("invalid server host: {0}")]
    InvalidHost(String),
    #[error("invalid server port: {0}")]
    InvalidPort(String),
    #[error("invalid value for {field}: {value}")]
    InvalidValue { field: &'static str, value: String },
    #[error("invalid cors origins: {0}")]
    InvalidCors(String),
    #[error("missing required secret for {0}")]
    MissingSecret(&'static str),
}

impl Settings {
    pub(crate) fn load() -> Result<Self, ConfigError> {
        let host = env_or_default("PICRETE_HOST", "0.0.0.0");
        let port = env_or_default("PICRETE_PORT", "8000");

        let environment =
            parse_environment(env_optional("PICRETE_ENV").or_else(|| env_optional("ENVIRONMENT")));
        let strict_config =
            env_optional("PICRETE_STRICT_CONFIG").map(|value| parse_bool(&value)).unwrap_or(false)
                || environment.is_production();

        let project_name = env_or_default("PROJECT_NAME", "Picrete API");
        let version = env_or_default("VERSION", env!("CARGO_PKG_VERSION"));
        let api_v1_str = env_or_default("API_V1_STR", "/api/v1");
        let terms_version = env_or_default("TERMS_VERSION", "2025-12-09");
        let privacy_version = env_or_default("PRIVACY_VERSION", "2025-12-09");
        let pd_consent_version = env_or_default("PD_CONSENT_VERSION", "2025-12-09");

        let secret_key = match env_optional("SECRET_KEY") {
            Some(value) => value,
            None => load_or_create_secret_key(),
        };

        let access_token_expire_minutes = parse_u64(
            "ACCESS_TOKEN_EXPIRE_MINUTES",
            env_or_default("ACCESS_TOKEN_EXPIRE_MINUTES", "10080"),
        )?;
        let algorithm = env_or_default("ALGORITHM", "HS256");

        let cors_origins = parse_cors_origins(env_optional("BACKEND_CORS_ORIGINS"))?;

        let postgres_server = env_or_default("POSTGRES_SERVER", "localhost");
        let postgres_port = parse_u16("POSTGRES_PORT", env_or_default("POSTGRES_PORT", "5432"))?;
        let postgres_user = env_or_default("POSTGRES_USER", "picretesuperuser");
        let postgres_password = env_or_default("POSTGRES_PASSWORD", "");
        let postgres_db = env_or_default("POSTGRES_DB", "picrete_db");
        let database_url = env_optional("DATABASE_URL");

        let redis_host = env_or_default("REDIS_HOST", "localhost");
        let redis_port = parse_u16("REDIS_PORT", env_or_default("REDIS_PORT", "6379"))?;
        let redis_db = parse_u16("REDIS_DB", env_or_default("REDIS_DB", "0"))?;
        let redis_password = env_or_default("REDIS_PASSWORD", "");

        let openai_api_key = env_or_default("OPENAI_API_KEY", "");
        let openai_base_url = env_or_default("OPENAI_BASE_URL", "");
        let ai_model = env_or_default("AI_MODEL", "gpt-5");
        let ai_max_tokens = parse_u32("AI_MAX_TOKENS", env_or_default("AI_MAX_TOKENS", "10000"))?;
        let ai_request_timeout =
            parse_u64("AI_REQUEST_TIMEOUT", env_or_default("AI_REQUEST_TIMEOUT", "600"))?;

        let max_upload_size_mb =
            parse_u64("MAX_UPLOAD_SIZE_MB", env_or_default("MAX_UPLOAD_SIZE_MB", "10"))?;
        let allowed_image_extensions =
            parse_string_list(env_optional("ALLOWED_IMAGE_EXTENSIONS"), &["jpg", "jpeg", "png"]);
        let max_images_per_submission = parse_u64(
            "MAX_IMAGES_PER_SUBMISSION",
            env_or_default("MAX_IMAGES_PER_SUBMISSION", "10"),
        )?;

        let s3_endpoint = env_or_default("S3_ENDPOINT", "https://storage.yandexcloud.net");
        let s3_access_key = env_or_default("S3_ACCESS_KEY", "");
        let s3_secret_key = env_or_default("S3_SECRET_KEY", "");
        let s3_bucket = env_or_default("S3_BUCKET", "picrete-data-storage");
        let s3_region = env_or_default("S3_REGION", "ru-central1");

        let max_concurrent_exams =
            parse_u64("MAX_CONCURRENT_EXAMS", env_or_default("MAX_CONCURRENT_EXAMS", "150"))?;
        let auto_save_interval_seconds = parse_u64(
            "AUTO_SAVE_INTERVAL_SECONDS",
            env_or_default("AUTO_SAVE_INTERVAL_SECONDS", "10"),
        )?;
        let presigned_url_expire_minutes = parse_u64(
            "PRESIGNED_URL_EXPIRE_MINUTES",
            env_or_default("PRESIGNED_URL_EXPIRE_MINUTES", "5"),
        )?;

        let first_superuser_isu = env_or_default("FIRST_SUPERUSER_ISU", "000000");
        let first_superuser_password = env_or_default("FIRST_SUPERUSER_PASSWORD", "");

        let log_level = env_or_default("PICRETE_LOG_LEVEL", "info");
        let json = env_optional("PICRETE_LOG_JSON")
            .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
            .unwrap_or(false);
        let prometheus_enabled = env_optional("PROMETHEUS_ENABLED")
            .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
            .unwrap_or(false);

        let settings = Self {
            server: ServerSettings {
                host: ServerHost::parse(host)?,
                port: ServerPort::parse(port)?,
            },
            runtime: RuntimeSettings { environment, strict_config },
            api: ApiSettings {
                project_name,
                version,
                api_v1_str,
                terms_version,
                privacy_version,
                pd_consent_version,
            },
            security: SecuritySettings { secret_key, access_token_expire_minutes, algorithm },
            cors: CorsSettings { origins: cors_origins },
            database: DatabaseSettings {
                postgres_server,
                postgres_port,
                postgres_user,
                postgres_password,
                postgres_db,
                database_url,
            },
            redis: RedisSettings {
                host: redis_host,
                port: redis_port,
                db: redis_db,
                password: redis_password,
            },
            ai: AiSettings {
                openai_api_key,
                openai_base_url,
                ai_model,
                ai_max_tokens,
                ai_request_timeout,
            },
            storage: StorageSettings {
                max_upload_size_mb,
                allowed_image_extensions,
                max_images_per_submission,
            },
            s3: S3Settings {
                endpoint: s3_endpoint,
                access_key: s3_access_key,
                secret_key: s3_secret_key,
                bucket: s3_bucket,
                region: s3_region,
            },
            exam: ExamSettings {
                max_concurrent_exams,
                auto_save_interval_seconds,
                presigned_url_expire_minutes,
            },
            admin: AdminSettings { first_superuser_isu, first_superuser_password },
            telemetry: TelemetrySettings { log_level, json, prometheus_enabled },
        };

        settings.validate()?;

        Ok(settings)
    }

    pub(crate) fn server_addr(&self) -> String {
        format!("{}:{}", self.server.host.0, self.server.port.0)
    }

    pub(crate) fn server_host(&self) -> &str {
        &self.server.host.0
    }

    pub(crate) fn server_port(&self) -> u16 {
        self.server.port.0
    }

    pub(crate) fn api(&self) -> &ApiSettings {
        &self.api
    }

    pub(crate) fn security(&self) -> &SecuritySettings {
        &self.security
    }

    pub(crate) fn cors(&self) -> &CorsSettings {
        &self.cors
    }

    pub(crate) fn database(&self) -> &DatabaseSettings {
        &self.database
    }

    pub(crate) fn redis(&self) -> &RedisSettings {
        &self.redis
    }

    pub(crate) fn ai(&self) -> &AiSettings {
        &self.ai
    }

    pub(crate) fn storage(&self) -> &StorageSettings {
        &self.storage
    }

    pub(crate) fn s3(&self) -> &S3Settings {
        &self.s3
    }

    pub(crate) fn exam(&self) -> &ExamSettings {
        &self.exam
    }

    pub(crate) fn admin(&self) -> &AdminSettings {
        &self.admin
    }

    pub(crate) fn telemetry(&self) -> &TelemetrySettings {
        &self.telemetry
    }

    pub(crate) fn runtime(&self) -> &RuntimeSettings {
        &self.runtime
    }

    fn validate(&self) -> Result<(), ConfigError> {
        if self.storage.allowed_image_extensions.is_empty() {
            return Err(ConfigError::InvalidValue {
                field: "ALLOWED_IMAGE_EXTENSIONS",
                value: String::from("<empty>"),
            });
        }
        for extension in &self.storage.allowed_image_extensions {
            if !is_supported_image_extension(extension) {
                return Err(ConfigError::InvalidValue {
                    field: "ALLOWED_IMAGE_EXTENSIONS",
                    value: extension.clone(),
                });
            }
        }

        if !(self.runtime.strict_config || self.runtime.environment.is_production()) {
            return Ok(());
        }

        if self.database.database_url.is_none() && self.database.postgres_password.is_empty() {
            return Err(ConfigError::MissingSecret("POSTGRES_PASSWORD"));
        }

        if self.ai.openai_api_key.is_empty() {
            return Err(ConfigError::MissingSecret("OPENAI_API_KEY"));
        }

        if self.ai.openai_base_url.is_empty() {
            return Err(ConfigError::MissingSecret("OPENAI_BASE_URL"));
        }

        if self.s3.access_key.is_empty() || self.s3.secret_key.is_empty() {
            return Err(ConfigError::MissingSecret("S3_ACCESS_KEY/S3_SECRET_KEY"));
        }

        if self.admin.first_superuser_password.is_empty() {
            return Err(ConfigError::MissingSecret("FIRST_SUPERUSER_PASSWORD"));
        }

        Ok(())
    }
}

impl DatabaseSettings {
    pub(crate) fn database_url(&self) -> String {
        if let Some(url) = &self.database_url {
            return url.clone();
        }
        format!(
            "postgresql://{}:{}@{}:{}/{}",
            self.postgres_user,
            self.postgres_password,
            self.postgres_server,
            self.postgres_port,
            self.postgres_db
        )
    }
}

impl RedisSettings {
    pub(crate) fn redis_url(&self) -> String {
        if self.password.is_empty() {
            format!("redis://{}:{}/{}", self.host, self.port, self.db)
        } else {
            format!("redis://:{}@{}:{}/{}", self.password, self.host, self.port, self.db)
        }
    }
}

impl ServerHost {
    fn parse(value: String) -> Result<Self, ConfigError> {
        if value.trim().is_empty() {
            return Err(ConfigError::InvalidHost(value));
        }
        Ok(Self(value))
    }
}

impl ServerPort {
    fn parse(value: String) -> Result<Self, ConfigError> {
        let parsed: u16 = value.parse().map_err(|_| ConfigError::InvalidPort(value.clone()))?;
        if parsed == 0 {
            return Err(ConfigError::InvalidPort(value));
        }
        Ok(Self(parsed))
    }
}

fn env_optional(key: &str) -> Option<String> {
    env::var(key).ok().map(|value| value.trim().to_string()).filter(|value| !value.is_empty())
}

fn env_or_default(key: &str, default: &str) -> String {
    env_optional(key).unwrap_or_else(|| default.to_string())
}

fn parse_u16(field: &'static str, value: String) -> Result<u16, ConfigError> {
    value.parse::<u16>().map_err(|_| ConfigError::InvalidValue { field, value })
}

fn parse_u32(field: &'static str, value: String) -> Result<u32, ConfigError> {
    value.parse::<u32>().map_err(|_| ConfigError::InvalidValue { field, value })
}

fn parse_u64(field: &'static str, value: String) -> Result<u64, ConfigError> {
    value.parse::<u64>().map_err(|_| ConfigError::InvalidValue { field, value })
}

fn parse_cors_origins(value: Option<String>) -> Result<Vec<String>, ConfigError> {
    let Some(raw) = value else {
        return Ok(DEFAULT_CORS_ORIGINS.iter().map(|item| item.to_string()).collect());
    };

    if raw.trim().is_empty() {
        return Ok(DEFAULT_CORS_ORIGINS.iter().map(|item| item.to_string()).collect());
    }

    if raw.trim_start().starts_with('[') {
        let parsed: Vec<String> =
            serde_json::from_str(&raw).map_err(|_| ConfigError::InvalidCors(raw.clone()))?;
        if parsed.is_empty() {
            return Ok(DEFAULT_CORS_ORIGINS.iter().map(|item| item.to_string()).collect());
        }
        return Ok(parsed);
    }

    let items: Vec<String> = raw
        .split(',')
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect();

    if items.is_empty() {
        return Ok(DEFAULT_CORS_ORIGINS.iter().map(|item| item.to_string()).collect());
    }

    Ok(items)
}

fn parse_string_list(value: Option<String>, defaults: &[&str]) -> Vec<String> {
    match value {
        Some(raw) => raw
            .split(',')
            .map(|item| item.trim().to_ascii_lowercase())
            .filter(|item| !item.is_empty())
            .collect(),
        None => defaults.iter().map(|item| item.to_string()).collect(),
    }
}

fn parse_bool(value: &str) -> bool {
    matches!(value, "1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON")
}

fn parse_environment(value: Option<String>) -> Environment {
    match value.as_deref().map(|val| val.to_lowercase()) {
        Some(ref val) if val == "production" || val == "prod" => Environment::Production,
        Some(ref val) if val == "staging" => Environment::Staging,
        Some(ref val) if val == "test" || val == "testing" => Environment::Test,
        _ => Environment::Development,
    }
}

fn is_supported_image_extension(extension: &str) -> bool {
    matches!(extension, "jpg" | "jpeg" | "png" | "webp" | "gif")
}

fn load_or_create_secret_key() -> String {
    let path = secret_file_path();

    if let Ok(value) = fs::read_to_string(&path) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }

    let new_key = generate_secret_key();

    if let Some(parent) = path.parent() {
        if let Err(err) = fs::create_dir_all(parent) {
            tracing::warn!(error = %err, path = %parent.display(), "Failed to create secret key directory");
        }
    }

    match fs::OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(mut file) => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Err(err) = file.set_permissions(fs::Permissions::from_mode(0o600)) {
                    tracing::warn!(error = %err, path = %path.display(), "Failed to set secret key file permissions");
                }
            }
            if let Err(err) = std::io::Write::write_all(&mut file, new_key.as_bytes()) {
                tracing::warn!(error = %err, path = %path.display(), "Failed to write secret key file");
            }
            return new_key;
        }
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
            if let Ok(value) = fs::read_to_string(&path) {
                let trimmed = value.trim();
                if !trimmed.is_empty() {
                    return trimmed.to_string();
                }
            }
        }
        Err(err) => {
            tracing::warn!(error = %err, path = %path.display(), "Failed to create secret key file");
        }
    }

    new_key
}

fn generate_secret_key() -> String {
    let mut bytes = [0u8; 64];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn secret_file_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".secret_key")
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
        let defaults: Vec<String> =
            DEFAULT_CORS_ORIGINS.iter().map(|item| item.to_string()).collect();
        assert_eq!(parsed, defaults);
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
