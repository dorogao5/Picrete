# Picrete Backend

**Backend платформы автоматизированной проверки контрольных работ по химии с использованием искусственного интеллекта**

Picrete Backend — это Rust API (Axum) с AI логикой для автоматической проверки работ студентов. Система обеспечивает API для фронтенда и обрабатывает задачи проверки через Redis и фоновый worker.
Сервис доступен по адресу https://picrete.com

## Технологический стек

### Backend
- **Axum** — асинхронный Rust веб-фреймворк
- **SQLx** — асинхронная работа с PostgreSQL
- **PostgreSQL** — реляционная база данных
- **Redis** — кэширование и очереди задач
- **Tokio** — асинхронная среда выполнения
- **OpenAI API** — AI для проверки работ
- **Yandex Object Storage (S3)** — хранение изображений решений

### Инфраструктура
- **Docker** — контейнеризация
- **Nginx** — reverse proxy и статика

## Структура проекта

```
app/
├── src/               # Исходный код
│   ├── api/           # API эндпоинты
│   ├── core/          # Конфигурация, безопасность
│   ├── db/            # Подключение к БД
│   ├── models/        # Модели данных
│   ├── schemas/       # Схемы валидации
│   ├── services/      # Бизнес-логика (AI grading, storage)
│   └── tasks/         # Фоновые задачи (grading, scheduler)
├── migrations/        # SQL миграции (SQLx)
├── scripts/           # Вспомогательные скрипты
├── tests/             # Тесты
├── Dockerfile         # Docker образ
├── docker-compose.prod.yml  # Docker Compose для production
├── Cargo.toml
└── README.md
```

## Быстрый старт

### Предварительные требования

- Rust 1.88+ (или используйте rust-toolchain.toml)
- PostgreSQL 14+
- Redis 6+
- Docker и Docker Compose (для production)

### Установка и запуск

1. **Клонирование репозитория**

```bash
git clone https://github.com/dorogao5/Picrete.git
cd Picrete/app
```

2. **Настройка переменных окружения**

Создайте файл `.env` в корне app:

```env
# Database
POSTGRES_SERVER=localhost
POSTGRES_PORT=5432
POSTGRES_USER=picretesuperuser
POSTGRES_PASSWORD=your_password
POSTGRES_DB=picrete_db

# Redis
REDIS_HOST=localhost
REDIS_PORT=6379
REDIS_PASSWORD=your_redis_password

# Security
SECRET_KEY=your_secret_key_here

# OpenAI
OPENAI_API_KEY=your_openai_api_key
OPENAI_BASE_URL=your_base_url
AI_MODEL=gpt-5

# Yandex Object Storage (опционально)
S3_ENDPOINT=https://storage.yandexcloud.net
S3_ACCESS_KEY=your_access_key
S3_SECRET_KEY=your_secret_key
S3_BUCKET=picrete-data-storage
S3_REGION=ru-central1

# CORS
BACKEND_CORS_ORIGINS=http://localhost:5173,http://localhost:3000,https://picrete.com,https://www.picrete.com
```

3. **Запуск приложения**

```bash
cargo run --bin picrete-rust
```

4. **Запуск worker (в отдельном терминале)**

```bash
cargo run --bin worker
```

API будет доступно по адресу `http://localhost:8000`

## Деплой

Для production развертывания используйте Docker Compose:

```bash
cd app
docker compose -f docker-compose.prod.yml build
docker compose -f docker-compose.prod.yml up -d
```

Подробные инструкции по деплою см. в `DEPLOYMENT_BACKEND.md`

## API Документация

После запуска backend, API документация доступна по адресу:
- Swagger UI: `http://localhost:8000/api/v1/docs`
- ReDoc: `http://localhost:8000/api/v1/redoc`

## Безопасность

- JWT токены для аутентификации
- Хеширование паролей с использованием Argon2
- Валидация файлов при загрузке
- CORS настройки для защиты от несанкционированных запросов
- Rate limiting (настраивается через Nginx)

## Связь с Frontend

Backend API доступен по адресу `/api/v1` и ожидает запросы от frontend приложения.

Frontend репозиторий: https://github.com/dorogao5/Front-Picrete

## Лицензия

Copyright (c) 2025
