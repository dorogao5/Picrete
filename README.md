# Picrete Backend

Backend платформы Picrete (Rust + Axum) для мультикурсовой EdTech-системы с OCR/LLM пайплайном проверки работ.

## Что реализовано

- Multi-course архитектура (`/courses/:course_id/...`) и строгая изоляция данных по курсам.
- Глобальные аккаунты по `username` + course membership роли (`teacher`, `student`).
- Invite flow: join по коду при signup и после логина.
- Типы работ: `control`, `homework`.
- OCR pipeline (DataLab Marker) + обязательный student OCR review + LLM precheck.
- Task bank (Свиридов), trainer sets, additional materials PDF.

## Стек

- Rust, Axum, Tokio
- PostgreSQL + SQLx
- Redis
- S3-compatible storage (Yandex Object Storage)
- OpenAI-compatible LLM API
- DataLab Marker OCR API

## Структура

```text
src/
  api/
  schemas/
  repositories/
  services/
  tasks/
  core/
  db/
  main.rs
  lib.rs
src/bin/worker.rs
migrations/
docs/roadmaps/
```

## Быстрый старт

### 1) Подготовка

```bash
cp .env.example .env
```

Заполните минимум:
- `POSTGRES_*` / `DATABASE_URL`
- `REDIS_*`
- `SECRET_KEY`
- `OPENAI_API_KEY`, `OPENAI_BASE_URL`
- `DATALAB_API_KEY`, `DATALAB_BASE_URL`
- `S3_*`
- `FIRST_SUPERUSER_USERNAME`, `FIRST_SUPERUSER_PASSWORD`
- `COURSE_CONTEXT_MODE=route`

### 2) Запуск

```bash
cargo run --bin picrete-rust
cargo run --bin worker
```

### 3) Проверка

```bash
cargo check
cargo test
```

Примечание: `tests/migrations_smoke.rs` требует локально настроенную PostgreSQL-роль/БД (например, `picretesuperuser`).

## Основные API-группы

- `/api/v1/auth`
- `/api/v1/users`
- `/api/v1/courses`
- `/api/v1/courses/:course_id/exams`
- `/api/v1/courses/:course_id/submissions`
- `/api/v1/courses/:course_id/task-bank`
- `/api/v1/courses/:course_id/trainer`
- `/api/v1/courses/:course_id/materials`

## OCR/LLM pipeline (кратко)

1. Student submit -> submission enters OCR stage (если включен).
2. Worker OCR -> сохраняет markdown/chunks/bbox geometry.
3. Student проходит OCR review по страницам -> `Submit` или `Report`.
4. LLM precheck (если включен) -> `preliminary` для teacher review.
5. Teacher approve/override/regrade.

## Документация

- Backend architecture: `ARCHITECTURE.md`
- Детальные roadmap: `docs/roadmaps/`
- Frontend repo: `../Front-Picrete`
