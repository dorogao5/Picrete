# Picrete Backend

**Backend платформы автоматизированной проверки контрольных работ по химии с использованием искусственного интеллекта**

Picrete Backend — это FastAPI приложение с AI логикой для автоматической проверки работ студентов. Система обеспечивает API для фронтенда и обрабатывает задачи проверки через Celery.
Сервис доступен по адресу https://picrete.com

## Технологический стек

### Backend
- **FastAPI** — асинхронный Python веб-фреймворк
- **SQLAlchemy** (async) — ORM для работы с БД
- **PostgreSQL** — реляционная база данных
- **Redis** — кэширование и очереди задач
- **Celery** — фоновые задачи (автоматическая проверка)
- **OpenAI API** — AI для проверки работ
- **Yandex Object Storage** — хранение изображений решений
- **Pillow** — обработка изображений

### Инфраструктура
- **Docker** — контейнеризация
- **Nginx** — reverse proxy и статика
- **Gunicorn** — WSGI сервер для production

## Структура проекта

```
Picrete/
├── backend/
│   ├── app/
│   │   ├── api/         # API эндпоинты
│   │   ├── core/        # Конфигурация, безопасность
│   │   ├── db/          # Подключение к БД
│   │   ├── models/      # SQLAlchemy модели
│   │   ├── schemas/     # Pydantic схемы
│   │   ├── services/    # Бизнес-логика (AI grading, storage)
│   │   └── tasks/       # Celery задачи
│   ├── migrations/      # SQL миграции
│   ├── main.py          # Точка входа FastAPI
│   ├── Dockerfile       # Docker образ
│   ├── docker-compose.prod.yml  # Docker Compose конфигурация
│   ├── requirements.txt
│   ├── setup-nginx.sh   # Скрипт настройки Nginx
│   └── nginx-config-example.conf  # Пример конфигурации Nginx
├── README.md
└── DEPLOYMENT_BACKEND.md
```

## Быстрый старт

### Предварительные требования

- Python 3.11+
- PostgreSQL 14+
- Redis 6+
- Docker и Docker Compose (для production)

### Установка и запуск

1. **Клонирование репозитория**

```bash
git clone https://github.com/dorogao5/Picrete.git
cd Picrete/backend
```

2. **Настройка Backend**

```bash
python -m venv venv
source venv/bin/activate  # Windows: venv\Scripts\activate
pip install -r requirements.txt
```

3. **Настройка переменных окружения**

Создайте файл `backend/.env`:

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
BACKEND_CORS_ORIGINS=["http://localhost:5173","http://localhost:3000","https://picrete.com"]
```

4. **Инициализация базы данных**

```bash
# Создайте базу данных PostgreSQL
createdb picrete_db

# Применение миграций (при наличии) или автоматическое создание таблиц
python -m uvicorn main:app --reload
```

5. **Запуск Celery Worker (для фоновых задач)**

```bash
cd backend
celery -A app.core.celery_app worker --loglevel=info
```

6. **Запуск приложения**

```bash
cd backend
uvicorn main:app --reload --host 0.0.0.0 --port 8000
```

API будет доступно по адресу `http://localhost:8000`

## Деплой

Для production развертывания используйте Docker Compose:

```bash
cd backend
docker compose -f docker-compose.prod.yml build
docker compose -f docker-compose.prod.yml up -d
```

Подробные инструкции по деплою см. в `DEPLOYMENT_BACKEND.md`

## API Документация

После запуска backend, API документация доступна по адресу:
- Swagger UI: `http://localhost:8000/api/v1/docs`
- ReDoc: `http://localhost:8000/api/v1/redoc`

## Конфигурация

Основные настройки находятся в `backend/app/core/config.py`. Ключевые параметры:

- `AI_MODEL` — модель OpenAI для проверки (по умолчанию: gpt-5)
- `MAX_UPLOAD_SIZE_MB` — максимальный размер загружаемого файла (10 MB)
- `MAX_CONCURRENT_EXAMS` — максимальное количество одновременных экзаменов (150)
- `AUTO_SAVE_INTERVAL_SECONDS` — интервал автосохранения (10 сек)

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

Copyright (c) 2024
