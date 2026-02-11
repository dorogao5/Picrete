# Picrete Backend (Rust)

Бэкенд платформы Picrete: API, фоновые воркеры OCR/LLM/авто-сабмита и Telegram-бот для загрузки фото в активные работы.

## Что реализовано сейчас

- Мультикурсовая модель (`/courses/:course_id/...`) с изоляцией данных по курсу.
- Роли на membership-уровне: `teacher`, `student` (+ platform admin).
- Работы `control` и `homework`, попытки, тайминги, дедлайны.
- OCR + OCR review + LLM precheck pipeline.
- Единая загрузка изображений на сессию:
  - без привязки к номеру задачи,
  - immediate upload в S3 + БД,
  - серверный `order_index` (клиентский `order_index` игнорируется),
  - `GET /sessions/:session_id/images`,
  - `DELETE /sessions/:session_id/images/:image_id`.
- Серверный fail-safe авто-сабмит просроченных активных сессий (worker цикл каждые 30 секунд).
- Telegram-бот:
  - логин по Picrete username/password,
  - выбор уже начатой на сайте активной работы,
  - загрузка фото в ту же `submission_images` pipeline (`upload_source=telegram`).

## Бинарники (runtime)

В репозитории 3 исполняемых процесса:

- `picrete-rust` — HTTP API.
- `picrete-worker` — OCR/LLM/background maintenance.
- `picrete-telegram-bot` — Telegram polling bot.

Все 3 используют общий `Settings::load()` и общую валидацию конфига.

## Технологии

- Rust, Axum, Tokio
- PostgreSQL + SQLx
- Redis
- S3-compatible object storage
- OpenAI-compatible API
- DataLab OCR API

## Быстрый старт (локально)

### 1) Подготовьте `.env`

```bash
cp .env.example .env
```

Минимум для полноценной работы API/worker/bot:

- DB: `POSTGRES_*` или `DATABASE_URL`
- Redis: `REDIS_*`
- Security: `SECRET_KEY`
- S3: `S3_*`
- AI: `OPENAI_API_KEY`, `OPENAI_BASE_URL`
- OCR: `DATALAB_API_KEY`, `DATALAB_BASE_URL`
- Bootstrap admin: `FIRST_SUPERUSER_USERNAME`, `FIRST_SUPERUSER_PASSWORD`
- Telegram (если нужен бот): `TELEGRAM_BOT_ENABLED=true`, `TG_TOKEN=...`

### 2) Запуск в dev

```bash
cargo run --bin picrete-rust
cargo run --bin worker
cargo run --bin telegram_bot
```

### 3) Проверки

```bash
cargo check
cargo test
```

## Production build и Docker

`Dockerfile` копирует готовые release-бинарники из `target/release`, поэтому перед `docker compose ... --build` нужно собрать **все три** бинарника:

```bash
cargo build --release --bin picrete-rust --bin worker --bin telegram_bot
```

Дальше:

```bash
docker compose -f docker-compose.prod.yml up -d --build
```

Сервисы в compose:

- `api`
- `worker`
- `telegram-bot`

## API-группы

Префикс API: `API_V1_STR` (по умолчанию `/api/v1`).

- `/api/v1/auth`
- `/api/v1/users`
- `/api/v1/courses`
- `/api/v1/courses/:course_id/exams`
- `/api/v1/courses/:course_id/submissions`
- `/api/v1/courses/:course_id/task-bank`
- `/api/v1/courses/:course_id/trainer`
- `/api/v1/courses/:course_id/materials`

Ключевые новые student endpoints:

- `POST /api/v1/courses/:course_id/submissions/sessions/:session_id/upload`
- `GET /api/v1/courses/:course_id/submissions/sessions/:session_id/images`
- `DELETE /api/v1/courses/:course_id/submissions/sessions/:session_id/images/:image_id`

## Telegram-бот

Команды:

- `/start`
- `/login`
- `/works`
- `/use <номер|session_id>`
- `/logout`

Особенности:

- чувствительные действия только в `private` chat;
- login rate-limit через Redis;
- проверяется `user.is_active`;
- offset `getUpdates` сохраняется в БД (`telegram_bot_offsets`), чтобы не дублировать обработку после рестартов.

## Метрики

Примеры ключевых метрик:

- `uploads_total{source=web|telegram}`
- `telegram_auth_fail_total`
- `auto_submit_total`
- `expired_sessions_closed_total`
- `ocr_jobs_total`, `llm_precheck_jobs_total`
