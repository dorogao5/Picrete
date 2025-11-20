"""Celery application configuration"""
from celery import Celery
from celery.schedules import crontab
from app.core.config import settings

# Create Celery app with Redis as broker and result backend
celery_app = Celery(
    "picrete_worker",
    broker=settings.REDIS_URL,
    backend=settings.REDIS_URL,
    include=["app.tasks.grading"]
)

# Configure Celery for production use
celery_app.conf.update(
    # Task execution settings
    task_serializer="json",
    result_serializer="json",
    accept_content=["json"],
    timezone="UTC",
    enable_utc=True,
    
    # Performance settings for low-resource environment
    worker_prefetch_multiplier=1,  # One task at a time per worker
    worker_max_tasks_per_child=10,  # Restart worker after 10 tasks to prevent memory leaks
    task_acks_late=True,  # Acknowledge task after completion
    task_reject_on_worker_lost=True,  # Re-queue if worker dies
    
    # Task timeout settings
    task_time_limit=3600,  # 1 hour hard limit
    task_soft_time_limit=3300,  # 55 minutes soft limit (raise exception)
    
    # Result backend settings
    result_expires=86400,  # Results expire after 24 hours
    result_persistent=True,  # Persist results to Redis
    
    # Retry settings
    task_default_max_retries=3,
    task_default_retry_delay=60,  # 1 minute between retries
    
    # Rate limiting for AI calls (protect against rate limits)
    task_default_rate_limit="10/m",  # Max 10 AI grading tasks per minute
    
    # Beat schedule for periodic tasks
    beat_schedule={
        # Process completed exams every 5 minutes
        "process-completed-exams": {
            "task": "app.tasks.grading.process_completed_exams",
            "schedule": crontab(minute="*/5"),  # Every 5 minutes
        },
        # Close expired sessions every 5 minutes
        "close-expired-sessions": {
            "task": "app.tasks.grading.close_expired_sessions",
            "schedule": crontab(minute="*/5"),  # Every 5 minutes
        },
        # Retry failed submissions once per hour
        "retry-failed-submissions": {
            "task": "app.tasks.grading.retry_failed_submissions",
            "schedule": crontab(minute="0"),  # Every hour
        },
        # Cleanup old task results daily at 3 AM
        "cleanup-old-results": {
            "task": "app.tasks.grading.cleanup_old_results",
            "schedule": crontab(hour=3, minute=0),
        },
    },
)

# Task routing
celery_app.conf.task_routes = {
    "app.tasks.grading.grade_submission_task": {"queue": "grading"},
    "app.tasks.grading.process_completed_exams": {"queue": "scheduler"},
    "app.tasks.grading.close_expired_sessions": {"queue": "scheduler"},
    "app.tasks.grading.retry_failed_submissions": {"queue": "scheduler"},
}

