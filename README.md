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
- `worker` — OCR/LLM/background maintenance.
- `telegram_bot` — Telegram polling bot.

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

Для student-facing ассистента используется отдельный дедлайн
`ASSISTANT_AI_REQUEST_TIMEOUT` (по умолчанию 110 секунд). У reverse proxy для
`/api/v1` должны быть `proxy_read_timeout` и `proxy_send_timeout` не меньше
150 секунд: это оставляет запас на ограниченное ожидание слота и блокировки
диалога, а приложение успевает вернуть контролируемую ошибку до proxy timeout.
Число одновременных запросов ограничивает `ASSISTANT_CHAT_MAX_CONCURRENT`
(по умолчанию 12); повышать его выше 24 конфигурация не позволит, чтобы не
исчерпать PostgreSQL pool.

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


## Согласованный выпуск

Релиз собирается из чистых коммитов `main` Picrete, Studio-Picrete и Front-Picrete.
SHA всех трёх репозиториев записываются в общий `release-manifest.json` рядом с
каталогами выпуска на сервере. Собранные файлы не редактируются вручную.
API публикуют свой SHA через `/version`, frontend — через `/build-info.json`.
Изменения контракта Studio → Picrete проверяются вместе с обоими интерфейсами.

Перед публикацией проходят проверки `.github/workflows/verify.yml`.
Секреты, пользовательские данные, каталоги банка и артефакты проверки не коммитятся.

Production Compose использует `RELEASE_SHA` как тег образа; образ содержит API,
worker и Telegram-бот из одной сборки. Компиляция Linux: Rust 1.88, `BUILD_REVISION`
равен SHA, `cargo build --release --locked --bins`; затем `docker compose -f
docker-compose.prod.yml build` с тем же `RELEASE_SHA`.
Бот работает на solid; профиль `external-bot` на основной ВМ не запускается.
При обновлении бота переносится тот же образ и соответствующий набор миграций.
PostgreSQL, Redis и API слушают loopback; доступ к API идёт через nginx.

Проверки оценивания принимают критерии с `max_score` и старый формат `weight`
(доля от максимума). Пустая рубрика требует проверки преподавателем. Числа,
извлечённые из общего OCR, используются как подсказка; автоматический предел
балла применяется только к однозначному ответу одной задачи с явным правилом
преподавателя `max_score_on_mismatch`.

## Решения и разметка банка Свиридова

Миграция `20260908000000_task_bank_solutions.sql` добавляет отдельные поля
`solution`, `task_type`, `difficulty`, `volume`. Старые JSON-файлы совместимы:
отсутствующие поля не стирают уже импортированные решения и разметку.
`answer` остаётся кратким ответом; `has_solution` не зависит от `has_answer`.
Банк выдаёт текст решения только преподавателям курса и администраторам.
При добавлении в работу решение переносится в `reference_solution`, сложность —
в тип задачи. Достаточно полноценного решения, даже если краткого ответа нет.
Существующие работы остаются сохранёнными снимками и не меняются при импорте.

Подготовка CSV из подпапок (по умолчанию только проверка):

```bash
python3 scripts/enrich_sviridov.py \
  --csv-root '/path/to/Задания Свиридова' \
  --bank-root tasks/Sviridov_tasks \
  --output /private/import/Sviridov_tasks.json
```

`--apply` записывает итоговый JSON и отчёт, копирует проверенные изображения
в `ocr_output/Sviridov_tasks/csv_images`, сохраняя резервную копию прежнего
выходного файла. Остальные задания, старые ответы и теория сохраняются;
конфликтующие дубликаты и отсутствующие изображения прерывают подготовку.
Старые ответы-отсылки при наличии нового полного решения сохраняются в
`legacy_answer`, а не используются как эталон для отдельного задания.
Для читаемости длинный обычный текст выносится из LaTeX-блоков без изменения
формул. CSV, подготовленный банк и отчёты хранятся вне Git.

Перед установкой сделать резервную копию JSON и БД, добавить новые изображения
к существующим, заменить `Sviridov_tasks.json` в `TASK_BANK_ROOT`, затем запустить
новую версию API/worker. Миграции и импорт выполняются при запуске в транзакциях.
Синхронизировать набор миграций у Telegram-бота на отдельном хосте.

`GET /courses/:course_id/task-bank/facets?source=sviridov` возвращает значения
фильтров только из источников текущего курса. `/items` и генератор тренажёров
поддерживают `q`, `task_type`, `difficulty`, `volume`, `has_solution` вместе с
прежними фильтрами. Полный номер вида `7.61` ищется точно; другой текст — по
номеру, условию и теме. Выбор задания в интерфейсе сохраняется между страницами;
преподаватель может перенести подборку в новую работу.

Проверка подготовки CSV: `python3 -m unittest discover -s scripts -p test_enrich_sviridov.py`.
