# Picrete Backend

Backend платформы автоматизированной проверки контрольных работ по химии с использованием ИИ.

Rust API (Axum) + фоновый worker для AI-проверки работ студентов. PostgreSQL, Redis, S3-совместимое хранилище.

Продакшен: https://picrete.com

## Стек

| Компонент | Технология |
|-----------|-----------|
| Web-фреймворк | Axum 0.7 |
| Async runtime | Tokio |
| База данных | PostgreSQL 14+ (SQLx 0.7) |
| Очереди/кэш | Redis 6+ |
| AI | OpenAI-совместимый API |
| Хранение файлов | Yandex Object Storage (S3) |
| Аутентификация | JWT (jsonwebtoken) + Argon2 |
| Метрики | Prometheus |
| Контейнеризация | Docker |
| Reverse proxy | Nginx |

## Структура проекта

```
src/
├── api/                # HTTP-слой
│   ├── router.rs       # Маршрутизация
│   ├── auth.rs         # Аутентификация
│   ├── guards.rs       # Middleware (роли, права)
│   ├── handlers.rs     # Общие хендлеры
│   ├── exams/          # Эндпоинты экзаменов
│   ├── submissions/    # Эндпоинты работ (студент/преподаватель)
│   ├── users.rs        # Эндпоинты пользователей
│   ├── pagination.rs   # Пагинация
│   ├── validation.rs   # Валидация запросов
│   └── errors.rs       # Обработка ошибок
├── core/               # Инфраструктура
│   ├── config.rs       # Загрузка конфигурации из ENV
│   ├── state.rs        # AppState (DI)
│   ├── security.rs     # JWT, хеширование паролей
│   ├── redis.rs        # Redis-клиент
│   ├── bootstrap.rs    # Инициализация (суперпользователь)
│   ├── metrics.rs      # Prometheus метрики
│   ├── telemetry.rs    # Логирование (tracing)
│   └── shutdown.rs     # Graceful shutdown
├── db/                 # Слой базы данных
│   ├── mod.rs          # Пул соединений, миграции
│   ├── models.rs       # Модели таблиц
│   └── types.rs        # Enum-типы PostgreSQL
├── repositories/       # SQL-запросы
├── schemas/            # DTO (запросы/ответы)
├── services/           # Бизнес-логика
│   ├── ai_grading.rs   # AI-проверка работ
│   └── storage.rs      # S3-хранилище
├── tasks/              # Фоновые задачи
│   ├── scheduler.rs    # Планировщик (worker)
│   └── grading.rs      # Задача проверки
├── bin/
│   └── worker.rs       # Точка входа worker
├── main.rs             # Точка входа API
└── lib.rs              # Корневой модуль
```

## Запуск для разработки

### Требования

- Rust 1.88+ (см. `rust-toolchain.toml`)
- PostgreSQL 14+
- Redis 6+

### Настройка

```bash
cp .env.example .env
# Отредактировать .env — заполнить пароли, ключи
```

### Запуск

```bash
# API-сервер (порт 8000)
cargo run --bin picrete-rust

# Worker (в отдельном терминале)
cargo run --bin worker
```

### Тесты

```bash
cargo test
```

## Сборка и деплой (Production)

Бинарники собираются локально на сервере, Docker используется только как runtime-контейнер (без компиляции внутри).

### 1. Сборка

```bash
cd /srv/picrete/app
cargo build --release --bin picrete-rust --bin worker
```

### 2. Запуск в Docker

```bash
docker compose -f docker-compose.prod.yml up -d --build
```

### 3. Проверка

```bash
docker compose -f docker-compose.prod.yml ps
docker compose -f docker-compose.prod.yml logs api --tail=30
curl -fsS http://127.0.0.1:8000/healthz
```

### Обновление

```bash
cd /srv/picrete/app
git pull origin main
cargo build --release --bin picrete-rust --bin worker
docker compose -f docker-compose.prod.yml up -d --build
```

Подробные инструкции по настройке сервера: `DEPLOYMENT_BACKEND.md`

## Переменные окружения

См. `.env.example`. Основные группы:

| Группа | Переменные | Описание |
|--------|-----------|----------|
| Database | `POSTGRES_*` | Подключение к PostgreSQL |
| Redis | `REDIS_*` | Подключение к Redis |
| Security | `SECRET_KEY` | Ключ для JWT-токенов |
| AI | `OPENAI_*`, `AI_*` | Настройки AI-модели |
| S3 | `S3_*` | Yandex Object Storage |
| Admin | `FIRST_SUPERUSER_*` | ISU и пароль первого администратора |
| Telemetry | `PICRETE_LOG_*`, `PROMETHEUS_ENABLED` | Логирование, метрики |

## API

После запуска документация доступна:
- Swagger UI: `http://localhost:8000/api/v1/docs`
- Продакшен: `https://picrete.com/api/v1/docs`

## Frontend

Репозиторий фронтенда: https://github.com/dorogao5/Front-Picrete
