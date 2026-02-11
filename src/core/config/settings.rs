use super::parsing::{
    env_optional, env_or_default, is_supported_image_extension, parse_bool, parse_cors_origins,
    parse_course_context_mode, parse_environment, parse_string_list, parse_u16, parse_u32,
    parse_u64,
};
use super::secret::load_or_create_secret_key;
use super::types::{
    AdminSettings, AiSettings, ApiSettings, ConfigError, CorsSettings, CourseSettings,
    DatabaseSettings, DatalabSettings, ExamSettings, RedisSettings, RuntimeSettings, S3Settings,
    SecuritySettings, ServerHost, ServerPort, ServerSettings, Settings, StorageSettings,
    TaskBankSettings, TelegramSettings, TelemetrySettings,
};

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

        let datalab_api_key = env_or_default("DATALAB_API_KEY", "");
        let datalab_base_url = env_or_default("DATALAB_BASE_URL", "https://www.datalab.to/api/v1");
        let datalab_mode = env_or_default("DATALAB_MODE", "accurate").to_ascii_lowercase();
        let datalab_output_format =
            env_or_default("DATALAB_OUTPUT_FORMAT", "chunks,markdown").to_ascii_lowercase();
        let datalab_timeout_seconds =
            parse_u64("DATALAB_TIMEOUT_SECONDS", env_or_default("DATALAB_TIMEOUT_SECONDS", "120"))?;
        let datalab_poll_interval_seconds = parse_u64(
            "DATALAB_POLL_INTERVAL_SECONDS",
            env_or_default("DATALAB_POLL_INTERVAL_SECONDS", "2"),
        )?;
        let datalab_max_poll_attempts = parse_u32(
            "DATALAB_MAX_POLL_ATTEMPTS",
            env_or_default("DATALAB_MAX_POLL_ATTEMPTS", "120"),
        )?;
        let datalab_max_submit_retries = parse_u32(
            "DATALAB_MAX_SUBMIT_RETRIES",
            env_or_default("DATALAB_MAX_SUBMIT_RETRIES", "3"),
        )?;

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

        let first_superuser_username = env_or_default("FIRST_SUPERUSER_USERNAME", "admin");
        let first_superuser_password = env_or_default("FIRST_SUPERUSER_PASSWORD", "");

        let course_context_mode = parse_course_context_mode(env_optional("COURSE_CONTEXT_MODE"));

        let task_bank_root = env_or_default("TASK_BANK_ROOT", "tasks/Sviridov_tasks");
        let task_bank_media_root = env_or_default(
            "TASK_BANK_MEDIA_ROOT",
            "tasks/Sviridov_tasks/ocr_output/Sviridov_tasks",
        );
        let additional_materials_pdf = env_or_default(
            "ADDITIONAL_MATERIALS_PDF",
            "tasks/Sviridov_tasks/ocr_output/Sviridov_tasks/addition.pdf",
        );
        let telegram_enabled =
            env_optional("TELEGRAM_BOT_ENABLED").map(|value| parse_bool(&value)).unwrap_or(false);
        let telegram_token = env_or_default("TG_TOKEN", "");
        let telegram_poll_timeout_seconds = parse_u64(
            "TELEGRAM_POLL_TIMEOUT_SECONDS",
            env_or_default("TELEGRAM_POLL_TIMEOUT_SECONDS", "30"),
        )?;

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
            datalab: DatalabSettings {
                api_key: datalab_api_key,
                base_url: datalab_base_url,
                mode: datalab_mode,
                output_format: datalab_output_format,
                timeout_seconds: datalab_timeout_seconds,
                poll_interval_seconds: datalab_poll_interval_seconds,
                max_poll_attempts: datalab_max_poll_attempts,
                max_submit_retries: datalab_max_submit_retries,
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
            admin: AdminSettings { first_superuser_username, first_superuser_password },
            course: CourseSettings { context_mode: course_context_mode },
            task_bank: TaskBankSettings {
                root: task_bank_root,
                media_root: task_bank_media_root,
                additional_materials_pdf,
            },
            telegram: TelegramSettings {
                enabled: telegram_enabled,
                token: telegram_token,
                poll_timeout_seconds: telegram_poll_timeout_seconds,
            },
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

    pub(crate) fn datalab(&self) -> &DatalabSettings {
        &self.datalab
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

    pub(crate) fn course(&self) -> &CourseSettings {
        &self.course
    }

    pub(crate) fn task_bank(&self) -> &TaskBankSettings {
        &self.task_bank
    }

    pub(crate) fn telegram(&self) -> &TelegramSettings {
        &self.telegram
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

        if self.datalab.mode != "accurate" {
            return Err(ConfigError::InvalidValue {
                field: "DATALAB_MODE",
                value: self.datalab.mode.clone(),
            });
        }

        if self.datalab.output_format != "chunks,markdown" {
            return Err(ConfigError::InvalidValue {
                field: "DATALAB_OUTPUT_FORMAT",
                value: self.datalab.output_format.clone(),
            });
        }

        if self.datalab.poll_interval_seconds == 0 {
            return Err(ConfigError::InvalidValue {
                field: "DATALAB_POLL_INTERVAL_SECONDS",
                value: "0".to_string(),
            });
        }

        if self.datalab.max_poll_attempts == 0 {
            return Err(ConfigError::InvalidValue {
                field: "DATALAB_MAX_POLL_ATTEMPTS",
                value: "0".to_string(),
            });
        }

        if self.runtime.strict_config || self.runtime.environment.is_production() {
            let task_bank_root = std::path::Path::new(&self.task_bank.root);
            if !task_bank_root.exists() || !task_bank_root.is_dir() {
                return Err(ConfigError::InvalidValue {
                    field: "TASK_BANK_ROOT",
                    value: self.task_bank.root.clone(),
                });
            }

            let task_bank_media_root = std::path::Path::new(&self.task_bank.media_root);
            if !task_bank_media_root.exists() || !task_bank_media_root.is_dir() {
                return Err(ConfigError::InvalidValue {
                    field: "TASK_BANK_MEDIA_ROOT",
                    value: self.task_bank.media_root.clone(),
                });
            }

            let additional_pdf = std::path::Path::new(&self.task_bank.additional_materials_pdf);
            if !additional_pdf.exists() || !additional_pdf.is_file() {
                return Err(ConfigError::InvalidValue {
                    field: "ADDITIONAL_MATERIALS_PDF",
                    value: self.task_bank.additional_materials_pdf.clone(),
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
        if self.datalab.api_key.is_empty() {
            return Err(ConfigError::MissingSecret("DATALAB_API_KEY"));
        }
        if self.datalab.base_url.is_empty() {
            return Err(ConfigError::MissingSecret("DATALAB_BASE_URL"));
        }
        if self.s3.access_key.is_empty() || self.s3.secret_key.is_empty() {
            return Err(ConfigError::MissingSecret("S3_ACCESS_KEY/S3_SECRET_KEY"));
        }
        if self.admin.first_superuser_password.is_empty() {
            return Err(ConfigError::MissingSecret("FIRST_SUPERUSER_PASSWORD"));
        }
        if self.telegram.enabled && self.telegram.token.is_empty() {
            return Err(ConfigError::MissingSecret("TG_TOKEN"));
        }

        Ok(())
    }
}
