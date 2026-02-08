use thiserror::Error;

#[derive(Debug, Clone)]
pub(crate) struct Settings {
    pub(super) server: ServerSettings,
    pub(super) runtime: RuntimeSettings,
    pub(super) api: ApiSettings,
    pub(super) security: SecuritySettings,
    pub(super) cors: CorsSettings,
    pub(super) database: DatabaseSettings,
    pub(super) redis: RedisSettings,
    pub(super) ai: AiSettings,
    pub(super) storage: StorageSettings,
    pub(super) s3: S3Settings,
    pub(super) exam: ExamSettings,
    pub(super) admin: AdminSettings,
    pub(super) telemetry: TelemetrySettings,
}

#[derive(Debug, Clone)]
pub(crate) struct ServerSettings {
    pub(super) host: ServerHost,
    pub(super) port: ServerPort,
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
            Self::Development => "development",
            Self::Production => "production",
            Self::Staging => "staging",
            Self::Test => "test",
        }
    }

    pub(super) fn is_production(self) -> bool {
        matches!(self, Self::Production)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ServerHost(pub(super) String);

#[derive(Debug, Clone, Copy)]
pub(crate) struct ServerPort(pub(super) u16);

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
    pub(super) fn parse(value: String) -> Result<Self, ConfigError> {
        if value.trim().is_empty() {
            return Err(ConfigError::InvalidHost(value));
        }

        Ok(Self(value))
    }
}

impl ServerPort {
    pub(super) fn parse(value: String) -> Result<Self, ConfigError> {
        let parsed: u16 = value.parse().map_err(|_| ConfigError::InvalidPort(value.clone()))?;
        if parsed == 0 {
            return Err(ConfigError::InvalidPort(value));
        }

        Ok(Self(parsed))
    }
}
