#!/bin/bash
# Start Celery worker for production

# Set working directory
cd "$(dirname "$0")"

# Start worker with low resource usage
# - concurrency=1: Only 1 worker process (low RAM usage)
# - max-tasks-per-child=5: Restart worker after 5 tasks (prevent memory leaks)
# - prefetch-multiplier=1: Take only 1 task at a time
# - Queues: grading (AI tasks) and scheduler (periodic tasks)

exec celery -A app.core.celery_app worker \
    --loglevel=info \
    --concurrency=1 \
    --max-tasks-per-child=5 \
    --prefetch-multiplier=1 \
    -Q grading,scheduler \
    --logfile=/app/logs/celery-worker.log \
    --pidfile=/app/logs/celery-worker.pid

