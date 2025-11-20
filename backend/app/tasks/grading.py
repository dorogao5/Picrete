"""Celery tasks for AI grading"""
from celery import Task
from celery.utils.log import get_task_logger
from sqlalchemy import select
from sqlalchemy.orm import selectinload
from datetime import datetime, timedelta
from typing import Dict, Any
import asyncio

from app.core.celery_app import celery_app
from app.db.session import AsyncSessionLocal
from app.models.submission import Submission, SubmissionStatus, ExamSession, SessionStatus
from app.models.exam import Exam, TaskType, ExamStatus
from app.services.ai_grading import grade_submission
from app.services.storage import storage_service

logger = get_task_logger(__name__)


class DatabaseTask(Task):
    """Base task with database session handling"""
    
    _db = None
    
    def after_return(self, *args, **kwargs):
        """Close database session after task completes"""
        if self._db is not None:
            self._db.close()


@celery_app.task(
    bind=True,
    base=DatabaseTask,
    name="app.tasks.grading.grade_submission_task",
    max_retries=3,
    default_retry_delay=120,  # 2 minutes
    acks_late=True
)
def grade_submission_task(self, submission_id: str):
    """
    Async task to grade a submission using AI
    
    Args:
        submission_id: Submission ID to grade
    """
    logger.info(f"Starting AI grading task for submission {submission_id}")
    
    # Run async code in event loop
    loop = asyncio.get_event_loop()
    result = loop.run_until_complete(_grade_submission_async(submission_id, self))
    
    return result


async def _grade_submission_async(submission_id: str, task: Task) -> Dict[str, Any]:
    """Async implementation of grading task"""
    async with AsyncSessionLocal() as db:
        try:
            # Get submission with all relationships
            result = await db.execute(
                select(Submission)
                .where(Submission.id == submission_id)
                .options(
                    selectinload(Submission.images),
                    selectinload(Submission.session).selectinload(ExamSession.exam)
                    .selectinload(Exam.task_types).selectinload(TaskType.variants)
                )
            )
            submission = result.scalar_one_or_none()
            
            if not submission:
                logger.error(f"Submission {submission_id} not found")
                return {"error": "Submission not found"}
            
            # Check if already processed
            if submission.status not in [SubmissionStatus.PROCESSING, SubmissionStatus.FLAGGED]:
                logger.info(f"Submission {submission_id} already processed with status {submission.status}")
                return {"status": "already_processed", "submission_status": submission.status}
            
            # Update status
            submission.status = SubmissionStatus.PROCESSING
            submission.ai_request_started_at = datetime.utcnow()
            await db.commit()
            
            # Get session and exam
            session = submission.session
            exam = session.exam
            
            # Get image URLs
            image_urls = []
            for image in sorted(submission.images, key=lambda x: x.order_index):
                if image.file_path.startswith("submissions/"):
                    # S3 storage - generate presigned URL
                    try:
                        presigned_url = await storage_service.generate_presigned_url(
                            s3_key=image.file_path,
                            expires_in=3600  # 1 hour for AI processing
                        )
                        image_urls.append(presigned_url)
                    except Exception as e:
                        logger.error(f"Failed to generate presigned URL for image {image.id}: {e}")
                        raise
                else:
                    # Legacy local storage - should not happen in production
                    logger.error(f"Image {image.id} is stored in local storage (not S3). Skipping AI processing.")
                    raise ValueError(f"Image {image.id} is not stored in S3. All images must be in S3 for AI processing.")
            
            if not image_urls:
                raise ValueError("No accessible images found for AI processing")
            
            # Build task description from variants
            task_descriptions = []
            reference_solutions = []
            rubric_items = []
            total_max_score = 0
            
            for task_type in exam.task_types:
                variant_id = session.variant_assignments.get(task_type.id)
                variant = next((v for v in task_type.variants if v.id == variant_id), None)
                
                if variant:
                    task_descriptions.append(f"""
Задача {task_type.order_index + 1}: {task_type.title}
{task_type.description}

Вариант:
{variant.content}
""")
                    
                    # Add reference solution if available
                    if variant.reference_solution:
                        reference_solutions.append(f"""
Эталонное решение для задачи {task_type.order_index + 1}:
{variant.reference_solution}
""")
                    
                    rubric_items.append({
                        "task_type": task_type.title,
                        "max_score": task_type.max_score,
                        "criteria": "Оценивать по критериям в системном промпте"
                    })
                    total_max_score += task_type.max_score
            
            task_description = "\n".join(task_descriptions)
            reference_solution_text = "\n".join(reference_solutions) if reference_solutions else "См. критерии оценивания"
            rubric = {"criteria": rubric_items, "total_max_score": total_max_score}
            
            # Call AI grading service
            logger.info(f"Calling AI grading for submission {submission_id}")
            ai_result = await grade_submission(
                images=image_urls,
                task_description=task_description,
                reference_solution=reference_solution_text,
                rubric=rubric,
                max_score=total_max_score,
                chemistry_rules={},
                submission_id=submission_id
            )
            
            # Record completion time
            submission.ai_request_completed_at = datetime.utcnow()
            
            # Extract metadata
            metadata = ai_result.pop("_metadata", {})
            if metadata:
                submission.ai_request_duration_seconds = metadata.get("duration_seconds")
            
            # Update submission based on result
            if "error" in ai_result:
                # AI processing failed - retry
                submission.status = SubmissionStatus.FLAGGED
                submission.ai_error = ai_result["error"]
                submission.flag_reasons = ["ai_processing_error"]
                logger.error(f"AI grading failed for submission {submission_id}: {ai_result['error']}")
                
                # Retry if not exceeded max retries
                if submission.ai_retry_count < 3:
                    submission.ai_retry_count += 1
                    await db.commit()
                    raise task.retry(countdown=120)  # Retry in 2 minutes
                
            elif ai_result.get("unreadable"):
                # Images are unreadable
                submission.status = SubmissionStatus.FLAGGED
                submission.ai_error = ai_result.get("unreadable_reason")
                submission.flag_reasons = ["unreadable_images"]
                logger.warning(f"Submission {submission_id} marked as unreadable: {ai_result.get('unreadable_reason')}")
            else:
                # Success
                submission.status = SubmissionStatus.PRELIMINARY
                submission.ai_score = ai_result.get("total_score")
                submission.ai_analysis = ai_result
                submission.ai_comments = ai_result.get("feedback")
                submission.ai_processed_at = datetime.utcnow()

                # Store transcriptions if present
                per_page = ai_result.get("per_page_transcriptions")
                if per_page:
                    # Map by order_index
                    sorted_images = sorted(submission.images, key=lambda x: x.order_index)
                    for idx, image in enumerate(sorted_images):
                        if idx < len(per_page):
                            image.ocr_text = per_page[idx]
                            image.processed_at = datetime.utcnow()
                logger.info(f"AI grading successful for submission {submission_id}: {submission.ai_score}/{total_max_score}")
            
            await db.commit()
            
            return {
                "status": "success",
                "submission_id": submission_id,
                "submission_status": submission.status,
                "ai_score": submission.ai_score,
                "max_score": total_max_score
            }
            
        except Exception as e:
            logger.error(f"Error in grading task for submission {submission_id}: {e}", exc_info=True)
            
            # Update submission with error
            try:
                result = await db.execute(
                    select(Submission).where(Submission.id == submission_id)
                )
                submission = result.scalar_one_or_none()
                if submission:
                    submission.status = SubmissionStatus.FLAGGED
                    submission.ai_error = str(e)
                    submission.flag_reasons = ["task_error"]
                    submission.ai_retry_count = (submission.ai_retry_count or 0) + 1
                    await db.commit()
            except Exception as commit_error:
                logger.error(f"Failed to update submission after error: {commit_error}")
            
            # Retry if possible
            if task.request.retries < task.max_retries:
                raise task.retry(exc=e, countdown=120)
            
            return {"error": str(e), "submission_id": submission_id}


@celery_app.task(name="app.tasks.grading.process_completed_exams")
def process_completed_exams():
    """
    Scheduled task to process submissions from completed exams
    Runs every 5 minutes
    """
    logger.info("Starting scheduled task: process_completed_exams")
    
    loop = asyncio.get_event_loop()
    result = loop.run_until_complete(_process_completed_exams_async())
    
    return result


async def _process_completed_exams_async():
    """Find and queue submissions from completed exams"""
    async with AsyncSessionLocal() as db:
        try:
            now = datetime.utcnow()
            
            # Find exams that ended in the last hour and are still active
            result = await db.execute(
                select(Exam)
                .where(
                    Exam.status.in_([ExamStatus.ACTIVE, ExamStatus.PUBLISHED]),
                    Exam.end_time <= now,
                    Exam.end_time >= now - timedelta(hours=1)
                )
            )
            completed_exams = result.scalars().all()
            
            if not completed_exams:
                logger.info("No recently completed exams found")
                return {"processed_exams": 0, "queued_submissions": 0}
            
            logger.info(f"Found {len(completed_exams)} recently completed exams")
            
            queued_count = 0
            for exam in completed_exams:
                # Find unprocessed submissions for this exam
                submissions_result = await db.execute(
                    select(Submission)
                    .join(ExamSession)
                    .where(
                        ExamSession.exam_id == exam.id,
                        Submission.status == SubmissionStatus.UPLOADED
                    )
                    .options(selectinload(Submission.images))
                )
                submissions = submissions_result.scalars().all()
                
                logger.info(f"Exam {exam.id} ({exam.title}): found {len(submissions)} unprocessed submissions")
                
                # Queue each submission for grading
                for submission in submissions:
                    # Skip empty submissions (no images)
                    if not submission.images or len(submission.images) == 0:
                        continue
                    try:
                        # Update status to processing
                        submission.status = SubmissionStatus.PROCESSING
                        await db.commit()
                        
                        # Queue task
                        grade_submission_task.apply_async(
                            args=[submission.id],
                            queue="grading",
                            priority=5  # Normal priority
                        )
                        queued_count += 1
                        logger.info(f"Queued submission {submission.id} for grading")
                    except Exception as e:
                        logger.error(f"Failed to queue submission {submission.id}: {e}")
                
                # Update exam status to completed
                exam.status = ExamStatus.COMPLETED
                await db.commit()
            
            logger.info(f"Processed {len(completed_exams)} exams, queued {queued_count} submissions")
            return {
                "processed_exams": len(completed_exams),
                "queued_submissions": queued_count
            }
            
        except Exception as e:
            logger.error(f"Error in process_completed_exams: {e}", exc_info=True)
            return {"error": str(e)}


@celery_app.task(name="app.tasks.grading.close_expired_sessions")
def close_expired_sessions():
    """
    Close ACTIVE sessions past their expiry and create empty submissions if missing.
    Runs every 5 minutes (configure in Celery beat).
    """
    logger.info("Starting scheduled task: close_expired_sessions")
    loop = asyncio.get_event_loop()
    return loop.run_until_complete(_close_expired_sessions_async())


async def _close_expired_sessions_async():
    async with AsyncSessionLocal() as db:
        import uuid
        try:
            now = datetime.utcnow()
            # Find active sessions past expiry
            result = await db.execute(
                select(ExamSession)
                .where(ExamSession.status == SessionStatus.ACTIVE)
                .options(
                    selectinload(ExamSession.submissions), 
                    selectinload(ExamSession.exam).selectinload(Exam.task_types)
                )
            )
            sessions = result.scalars().all()

            closed = 0
            created_empty = 0
            for session in sessions:
                hard_deadline = session.expires_at
                if session.exam and session.exam.end_time:
                    hard_deadline = min(hard_deadline, session.exam.end_time)
                if now >= hard_deadline:
                    session.status = SessionStatus.EXPIRED
                    session.submitted_at = session.submitted_at or hard_deadline
                    closed += 1
                    # Ensure a submission exists
                    submission = session.submissions[0] if session.submissions else None
                    if not submission:
                        # Compute total max score
                        exam = session.exam
                        total_max = 100.0
                        if exam and exam.task_types:
                            total_max = sum(task_type.max_score for task_type in exam.task_types)
                        submission = Submission(
                            id=str(uuid.uuid4()),
                            session_id=session.id,
                            student_id=session.student_id,
                            status=SubmissionStatus.UPLOADED,
                            max_score=total_max,
                            submitted_at=hard_deadline
                        )
                        db.add(submission)
                        created_empty += 1

            await db.commit()
            logger.info(f"Closed {closed} expired sessions, created {created_empty} empty submissions")
            return {"closed_sessions": closed, "created_empty_submissions": created_empty}
        except Exception as e:
            logger.error(f"Error in close_expired_sessions: {e}", exc_info=True)
            return {"error": str(e)}


@celery_app.task(name="app.tasks.grading.retry_failed_submissions")
def retry_failed_submissions():
    """
    Retry submissions that failed AI processing
    Runs every hour
    """
    logger.info("Starting scheduled task: retry_failed_submissions")
    
    loop = asyncio.get_event_loop()
    result = loop.run_until_complete(_retry_failed_submissions_async())
    
    return result


async def _retry_failed_submissions_async():
    """Retry failed submissions with limited retry count"""
    async with AsyncSessionLocal() as db:
        try:
            # Find flagged submissions with retries left
            result = await db.execute(
                select(Submission)
                .where(
                    Submission.status == SubmissionStatus.FLAGGED,
                    Submission.ai_retry_count < 3,
                    Submission.ai_error.isnot(None)
                )
            )
            failed_submissions = result.scalars().all()
            
            if not failed_submissions:
                logger.info("No failed submissions to retry")
                return {"retried_submissions": 0}
            
            logger.info(f"Found {len(failed_submissions)} failed submissions to retry")
            
            retried_count = 0
            for submission in failed_submissions:
                try:
                    # Reset status
                    submission.status = SubmissionStatus.PROCESSING
                    submission.ai_error = None
                    await db.commit()
                    
                    # Queue task with lower priority
                    grade_submission_task.apply_async(
                        args=[submission.id],
                        queue="grading",
                        priority=3  # Lower priority than new submissions
                    )
                    retried_count += 1
                    logger.info(f"Queued failed submission {submission.id} for retry")
                except Exception as e:
                    logger.error(f"Failed to retry submission {submission.id}: {e}")
            
            return {"retried_submissions": retried_count}
            
        except Exception as e:
            logger.error(f"Error in retry_failed_submissions: {e}", exc_info=True)
            return {"error": str(e)}


@celery_app.task(name="app.tasks.grading.cleanup_old_results")
def cleanup_old_results():
    """
    Cleanup old task results from Redis
    Runs daily at 3 AM
    """
    logger.info("Starting scheduled task: cleanup_old_results")
    
    try:
        # Celery automatically handles result expiration
        # This task is just a placeholder for any additional cleanup
        logger.info("Task results cleanup completed")
        return {"status": "completed"}
    except Exception as e:
        logger.error(f"Error in cleanup_old_results: {e}", exc_info=True)
        return {"error": str(e)}

