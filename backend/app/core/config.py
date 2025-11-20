from typing import List, Union
from pydantic import field_validator, PostgresDsn
from pydantic_settings import BaseSettings, SettingsConfigDict
import secrets


class Settings(BaseSettings):
    """Application settings"""
    
    model_config = SettingsConfigDict(
        env_file=".env",
        env_ignore_empty=True,
        extra="ignore",
    )
    
    # API Settings
    PROJECT_NAME: str = "Picrete API"
    VERSION: str = "1.0.0"
    API_V1_STR: str = "/api/v1"
    
    # Security
    SECRET_KEY: str = secrets.token_urlsafe(32)
    ACCESS_TOKEN_EXPIRE_MINUTES: int = 60 * 24 * 7  # 7 days
    ALGORITHM: str = "HS256"
    
    # CORS
    BACKEND_CORS_ORIGINS: List[str] = [
        "http://localhost:5173",
        "http://localhost:3000",
        "http://localhost:8080",
        "https://picrete.com",
        "https://www.picrete.com",
        "https://picrete.com:443",
        "https://www.picrete.com:443",
    ]
    
    @field_validator("BACKEND_CORS_ORIGINS", mode="before")
    @classmethod
    def assemble_cors_origins(cls, v: Union[str, List[str]]) -> Union[List[str], str]:
        if isinstance(v, str):
            if v.startswith("["):
                # Это JSON массив в строке
                import json
                try:
                    return json.loads(v)
                except json.JSONDecodeError:
                    # Если JSON не парсится, попробуем как строку с запятыми
                    return [i.strip() for i in v.split(",")]
            else:
                # Это строка, разделенная запятыми
                return [i.strip() for i in v.split(",")]
        elif isinstance(v, list):
            return v
        raise ValueError(v)
    
    # Database
    POSTGRES_SERVER: str = "localhost"
    POSTGRES_PORT: int = 5432
    POSTGRES_USER: str = "picretesuperuser"
    POSTGRES_PASSWORD: str = ""  # Set via .env file
    POSTGRES_DB: str = "picrete_db"
    DATABASE_URL: str | None = None
    
    @property
    def SQLALCHEMY_DATABASE_URI(self) -> str:
        if self.DATABASE_URL:
            return self.DATABASE_URL
        return f"postgresql+asyncpg://{self.POSTGRES_USER}:{self.POSTGRES_PASSWORD}@{self.POSTGRES_SERVER}:{self.POSTGRES_PORT}/{self.POSTGRES_DB}"
    
    # Redis
    REDIS_HOST: str = "localhost"
    REDIS_PORT: int = 6379
    REDIS_DB: int = 0
    REDIS_PASSWORD: str = ""  # Set via .env file
    
    @property
    def REDIS_URL(self) -> str:
        if self.REDIS_PASSWORD:
            return f"redis://:{self.REDIS_PASSWORD}@{self.REDIS_HOST}:{self.REDIS_PORT}/{self.REDIS_DB}"
        return f"redis://{self.REDIS_HOST}:{self.REDIS_PORT}/{self.REDIS_DB}"
    
    # AI/ML Settings
    OPENAI_API_KEY: str = ""  # Set via .env file
    OPENAI_BASE_URL: str = "http://188.213.0.226:8082/v1"  # Proxy server for requests from Moscow
    AI_MODEL: str = "gpt-5"  # Will use GPT-5 when available
    AI_MAX_TOKENS: int = 10000
    AI_TEMPERATURE: float = 0.3
    AI_REQUEST_TIMEOUT: int = 600  # Request timeout in seconds
    
    # File Storage
    UPLOAD_DIR: str = "./uploads"
    MAX_UPLOAD_SIZE_MB: int = 10
    ALLOWED_IMAGE_EXTENSIONS: List[str] = ["jpg", "jpeg", "png"]
    MAX_IMAGES_PER_SUBMISSION: int = 10
    
    # S3 Storage (Yandex Object Storage)
    S3_ENDPOINT: str = "https://storage.yandexcloud.net"
    S3_ACCESS_KEY: str = ""  # Set via .env file
    S3_SECRET_KEY: str = ""  # Set via .env file
    S3_BUCKET: str = "picrete-data-storage"  # Bucket name in Yandex Object Storage
    S3_REGION: str = "ru-central1"  # Yandex Cloud region (ru-central1, ru-central2, etc.)
    
    # Exam Settings
    MAX_CONCURRENT_EXAMS: int = 150
    AUTO_SAVE_INTERVAL_SECONDS: int = 10
    PRESIGNED_URL_EXPIRE_MINUTES: int = 5
    
    # Default admin credentials
    FIRST_SUPERUSER_ISU: str = "000000"
    FIRST_SUPERUSER_PASSWORD: str = ""  # Set via .env file


settings = Settings()


