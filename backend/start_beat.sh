#!/bin/bash
# Start Celery beat scheduler for production

# Set working directory
cd "$(dirname "$0")"

# Start beat scheduler for periodic tasks
# - Checks for completed exams every 5 minutes
# - Retries failed submissions every hour
# - Cleans up old results daily

exec celery -A app.core.celery_app beat \
    --loglevel=info \
    --logfile=/app/logs/celery-beat.log \
    --pidfile=/app/logs/celery-beat.pid

