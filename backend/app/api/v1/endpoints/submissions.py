    
from fastapi import APIRouter, Depends, HTTPException, status, UploadFile, File, Form, Body
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy import select
from sqlalchemy.orm import selectinload
from typing import List, Dict, Any
import uuid
from datetime import datetime, timedelta, timezone
import random
import hashlib

from app.api.deps import get_db, get_current_user, get_current_teacher
from app.models.user import User
from app.models.exam import Exam, TaskType, TaskVariant, ExamStatus
from app.models.submission import ExamSession, Submission, SubmissionImage, SubmissionStatus, SessionStatus
from app.schemas.submission import (
    ExamSession as ExamSessionSchema,
    Submission as SubmissionSchema,
    SubmissionApprove,
    SubmissionOverride
)
from app.core.config import settings
from app.services.storage import storage_service
from app.core.redis_client import redis_client
import logging

router = APIRouter()
logger = logging.getLogger(__name__)


@router.get("/my-submissions")
async def get_my_submissions(
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user)
):
    """Get all submissions for current user"""
    # Get all sessions for the current user
    sessions_result = await db.execute(
        select(ExamSession)
        .where(ExamSession.student_id == current_user.id)
        .options(selectinload(ExamSession.exam).selectinload(Exam.task_types))
    )
    sessions = sessions_result.scalars().all()
    
    # Get submissions for each session
    submissions_data = []
    for session in sessions:
        # Get submission for this session
        submission_result = await db.execute(
            select(Submission)
            .where(Submission.session_id == session.id)
            .options(
                selectinload(Submission.images),
                selectinload(Submission.scores)
            )
        )
        submission = submission_result.scalar_one_or_none()
        
        if submission:
            submissions_data.append({
                "id": submission.id,
                "session_id": session.id,
                "exam_id": session.exam_id,
                "exam_title": session.exam.title if session.exam else "Unknown",
                "submitted_at": submission.submitted_at,
                "status": submission.status,
                "ai_score": submission.ai_score,
                "final_score": submission.final_score,
                "max_score": submission.max_score,
                "images": submission.images,
                "scores": submission.scores,
                "teacher_comments": submission.teacher_comments,
            })
        else:
            # Create empty submission for sessions without submissions
            exam = session.exam
            total_max_score = 100.0
            if exam and exam.task_types:
                total_max_score = sum(task_type.max_score for task_type in exam.task_types)
            
            submission = Submission(
                id=str(uuid.uuid4()),
                session_id=session.id,
                student_id=session.student_id,
                status=SubmissionStatus.UPLOADED,
                max_score=total_max_score,
                submitted_at=session.submitted_at or datetime.utcnow()
            )
            db.add(submission)
            await db.commit()
            
            submissions_data.append({
                "id": submission.id,
                "session_id": session.id,
                "exam_id": session.exam_id,
                "exam_title": session.exam.title if session.exam else "Unknown",
                "submitted_at": submission.submitted_at,
                "status": submission.status,
                "ai_score": submission.ai_score,
                "final_score": submission.final_score,
                "max_score": submission.max_score,
                "images": [],
                "scores": [],
                "teacher_comments": submission.teacher_comments,
            })
    
    return submissions_data


@router.post("/exams/{exam_id}/enter", response_model=ExamSessionSchema)
async def enter_exam(
    exam_id: str,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user)
):
    """Student enters an exam and receives variant assignment"""
    # Get exam
    result = await db.execute(
        select(Exam)
        .where(Exam.id == exam_id)
        .options(selectinload(Exam.task_types).selectinload(TaskType.variants))
    )
    exam = result.scalar_one_or_none()
    
    if not exam:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail="Exam not found"
        )
    
    # Check exam is published/active
    if exam.status not in [ExamStatus.PUBLISHED, ExamStatus.ACTIVE]:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Exam is not available"
        )
    
    # Check timing - use UTC for all comparisons
    now = datetime.utcnow()
    # Database stores naive UTC datetimes, so we compare as naive
    start_time = exam.start_time
    end_time = exam.end_time
    
    if now < start_time:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Exam has not started yet"
        )
    if now > end_time:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Exam has ended"
        )
    
    # Check if student already has an active session
    existing_session_result = await db.execute(
        select(ExamSession).where(
            ExamSession.exam_id == exam_id,
            ExamSession.student_id == current_user.id,
            ExamSession.status == SessionStatus.ACTIVE
        )
    )
    existing_session = existing_session_result.scalar_one_or_none()
    
    if existing_session:
        return existing_session
    
    # Check attempt limit
    attempt_count_result = await db.execute(
        select(ExamSession).where(
            ExamSession.exam_id == exam_id,
            ExamSession.student_id == current_user.id
        )
    )
    attempt_count = len(attempt_count_result.scalars().all())
    
    if attempt_count >= exam.max_attempts:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Maximum attempts reached"
        )
    
    # Generate variant assignments
    variant_seed = random.randint(0, 2**31 - 1)
    random.seed(variant_seed)
    
    variant_assignments = {}
    for task_type in exam.task_types:
        if task_type.variants:
            variant = random.choice(task_type.variants)
            variant_assignments[task_type.id] = variant.id
    
    # Create session with naive UTC datetimes for database storage
    # Expiration must not exceed the overall exam end time
    expires_at_candidate = now + timedelta(minutes=exam.duration_minutes)
    expires_at = min(expires_at_candidate, end_time)
    
    session = ExamSession(
        id=str(uuid.uuid4()),
        exam_id=exam_id,
        student_id=current_user.id,
        variant_seed=variant_seed,
        variant_assignments=variant_assignments,
        started_at=now,
        expires_at=expires_at,
        status=SessionStatus.ACTIVE,
        attempt_number=attempt_count + 1,
    )
    
    db.add(session)
    await db.commit()
    await db.refresh(session)
    
    return session


@router.get("/sessions/{session_id}/variant")
async def get_session_variant(
    session_id: str,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user)
):
    """Get student's assigned variant for a session"""
    result = await db.execute(
        select(ExamSession)
        .where(ExamSession.id == session_id)
        .options(selectinload(ExamSession.exam).selectinload(Exam.task_types).selectinload(TaskType.variants))
    )
    session = result.scalar_one_or_none()
    
    if not session:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail="Session not found"
        )
    
    # Check ownership
    if session.student_id != current_user.id:
        raise HTTPException(
            status_code=status.HTTP_403_FORBIDDEN,
            detail="Access denied"
        )
    
    # Build variant response
    tasks = []
    for task_type in session.exam.task_types:
        variant_id = session.variant_assignments.get(task_type.id)
        variant = next((v for v in task_type.variants if v.id == variant_id), None)
        
        if variant:
            tasks.append({
                "task_type": {
                    "id": task_type.id,
                    "title": task_type.title,
                    "description": task_type.description,
                    "order_index": task_type.order_index,
                    "max_score": task_type.max_score,
                    "formulas": task_type.formulas,
                    "units": task_type.units,
                },
                "variant": {
                    "id": variant.id,
                    "content": variant.content,
                    "parameters": variant.parameters,
                    "attachments": variant.attachments,
                }
            })
    
    # Calculate non-negative remaining time
    if session.status == SessionStatus.ACTIVE:
        remaining = (session.expires_at - datetime.utcnow()).total_seconds()
        if remaining < 0:
            remaining = 0
    else:
        remaining = 0

    return {
        "session": session,
        "tasks": tasks,
        "time_remaining": remaining
    }


@router.post("/sessions/{session_id}/presigned-upload-url")
async def get_presigned_upload_url(
    session_id: str,
    filename: str,
    content_type: str,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user)
):
    """
    Get presigned URL for direct upload from client to S3
    Client should then PUT file to this URL
    """
    # Get session
    result = await db.execute(
        select(ExamSession)
        .where(ExamSession.id == session_id)
        .options(selectinload(ExamSession.exam))
    )
    session = result.scalar_one_or_none()
    
    if not session:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail="Session not found"
        )
    
    if session.student_id != current_user.id:
        raise HTTPException(
            status_code=status.HTTP_403_FORBIDDEN,
            detail="Access denied"
        )
    
    # Enforce expiry against session.expires_at and exam.end_time
    now = datetime.utcnow()
    hard_deadline = min(session.expires_at, session.exam.end_time) if session.exam else session.expires_at
    if now >= hard_deadline and session.status == SessionStatus.ACTIVE:
        session.status = SessionStatus.EXPIRED
        await db.commit()
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Session has expired"
        )

    if session.status != SessionStatus.ACTIVE:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Session is not active"
        )
    
    # Validate content type
    if content_type not in ["image/jpeg", "image/png"]:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Only JPEG and PNG images are allowed"
        )
    
    # Check S3 configuration
    if not (settings.S3_ENDPOINT and settings.S3_ACCESS_KEY and settings.S3_SECRET_KEY):
        raise HTTPException(
            status_code=status.HTTP_503_SERVICE_UNAVAILABLE,
            detail="Direct upload not available. Use standard upload endpoint."
        )
    
    try:
        presigned_url, s3_key = await storage_service.generate_presigned_upload_url(
            session_id=session_id,
            filename=filename,
            content_type=content_type
        )
        
        return {
            "upload_url": presigned_url,
            "s3_key": s3_key,
            "method": "PUT",
            "headers": {
                "Content-Type": content_type
            }
        }
    except Exception as e:
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=f"Failed to generate upload URL: {str(e)}"
        )


@router.post("/sessions/{session_id}/upload")
async def upload_submission_image(
    session_id: str,
    file: UploadFile = File(...),
    order_index: int = Form(...),
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user)
):
    """Upload an image for submission (fallback method or for development)"""
    # Get session
    result = await db.execute(
        select(ExamSession)
        .where(ExamSession.id == session_id)
        .options(selectinload(ExamSession.exam))
    )
    session = result.scalar_one_or_none()
    
    if not session:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail="Session not found"
        )
    
    if session.student_id != current_user.id:
        raise HTTPException(
            status_code=status.HTTP_403_FORBIDDEN,
            detail="Access denied"
        )
    
    if session.status != SessionStatus.ACTIVE:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Session is not active"
        )
    
    # Validate file
    if file.content_type not in ["image/jpeg", "image/png"]:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Only JPEG and PNG images are allowed"
        )
    
    # Check file size
    content = await file.read()
    file_size = len(content)
    max_size_bytes = settings.MAX_UPLOAD_SIZE_MB * 1024 * 1024
    
    if file_size > max_size_bytes:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail=f"File size exceeds {settings.MAX_UPLOAD_SIZE_MB}MB limit"
        )
    
    # Upload to S3 (required in production)
    file_id = str(uuid.uuid4())
    
    # Check S3 configuration - required in production
    if not (settings.S3_ENDPOINT and settings.S3_ACCESS_KEY and settings.S3_SECRET_KEY):
        raise HTTPException(
            status_code=status.HTTP_503_SERVICE_UNAVAILABLE,
            detail="S3 storage is not configured. Please configure Yandex Object Storage."
        )
    
    # Upload to S3
    try:
        s3_key, file_size, file_hash = await storage_service.upload_file(
            file_content=content,
            filename=file.filename or "image.jpg",
            content_type=file.content_type,
            session_id=session_id,
            metadata={
                "student_id": current_user.id,
                "order_index": str(order_index)
            }
        )
        file_path = s3_key  # Store S3 key in file_path field
    except Exception as e:
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=f"Failed to upload file to S3: {str(e)}"
        )
    
    # Create submission if not exists
    submission_result = await db.execute(
        select(Submission).where(Submission.session_id == session_id)
    )
    submission = submission_result.scalar_one_or_none()
    
    if not submission:
        max_score_result = await db.execute(
            select(TaskType.max_score)
            .join(Exam)
            .where(Exam.id == session.exam_id)
        )
        max_scores = max_score_result.scalars().all()
        total_max_score = sum(max_scores) if max_scores else 100
        
        submission = Submission(
            id=str(uuid.uuid4()),
            session_id=session_id,
            student_id=current_user.id,
            status=SubmissionStatus.UPLOADED,
            max_score=total_max_score,
        )
        db.add(submission)
        await db.flush()
    
    # Create image record
    image = SubmissionImage(
        id=file_id,
        submission_id=submission.id,
        filename=file.filename,
        file_path=file_path,
        file_size=file_size,
        mime_type=file.content_type,
        order_index=order_index,
    )
    db.add(image)
    
    await db.commit()
    
    return {"message": "File uploaded successfully", "image_id": file_id}


@router.post("/sessions/{session_id}/auto-save")
async def auto_save_session(
    session_id: str,
    auto_save_data: Dict[str, Any] = Body(...),
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user)
):
    """
    Auto-save session data (called every 10 seconds from client)
    This ensures no data is lost during exam
    """
    # Rate limiting - max 1 save per 5 seconds per session
    rate_limit_key = f"autosave:{session_id}"
    if not await redis_client.rate_limit(rate_limit_key, limit=1, window=5):
        raise HTTPException(
            status_code=status.HTTP_429_TOO_MANY_REQUESTS,
            detail="Auto-save rate limit exceeded"
        )
    
    # Get session
    result = await db.execute(
        select(ExamSession)
        .where(ExamSession.id == session_id)
        .options(selectinload(ExamSession.exam))
    )
    session = result.scalar_one_or_none()
    
    if not session:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail="Session not found"
        )
    
    if session.student_id != current_user.id:
        raise HTTPException(
            status_code=status.HTTP_403_FORBIDDEN,
            detail="Access denied"
        )
    
    # Enforce expiry against session.expires_at and exam.end_time
    now = datetime.utcnow()
    hard_deadline = min(session.expires_at, session.exam.end_time) if session.exam else session.expires_at
    if now >= hard_deadline and session.status == SessionStatus.ACTIVE:
        session.status = SessionStatus.EXPIRED
        await db.commit()
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Session has expired"
        )

    if session.status != SessionStatus.ACTIVE:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Session is not active"
        )
    
    # Update auto-save data
    session.auto_save_data = auto_save_data
    session.last_auto_save = datetime.utcnow()
    
    await db.commit()
    
    logger.debug(f"Auto-saved data for session {session_id}")
    
    return {
        "success": True,
        "last_auto_save": session.last_auto_save,
        "message": "Data saved successfully"
    }


@router.post("/sessions/{session_id}/submit", response_model=SubmissionSchema)
async def submit_exam(
    session_id: str,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user)
):
    """
    Submit exam - saves submission synchronously, queues AI grading for later
    This ensures fast response and no data loss
    """
    result = await db.execute(
        select(ExamSession)
        .where(ExamSession.id == session_id)
        .options(
            selectinload(ExamSession.submissions)
            .selectinload(Submission.images),
            selectinload(ExamSession.submissions)
            .selectinload(Submission.scores),
            selectinload(ExamSession.exam)
        )
    )
    session = result.scalar_one_or_none()
    
    if not session:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail="Session not found"
        )
    
    if session.student_id != current_user.id:
        raise HTTPException(
            status_code=status.HTTP_403_FORBIDDEN,
            detail="Access denied"
        )
    
    # Check if session is still active or recently expired (within 5 minutes)
    now = datetime.utcnow()
    hard_deadline = min(session.expires_at, session.exam.end_time) if session.exam else session.expires_at
    recently_expired = now <= hard_deadline + timedelta(minutes=5)
    
    if session.status != SessionStatus.ACTIVE and not recently_expired:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Session is not active or has expired"
        )
    
    # Get submission
    submission = session.submissions[0] if session.submissions else None
    
    if not submission:
        # Create empty submission if none exists
        # This handles cases where student submits without uploading images
        exam = session.exam
        
        # Calculate total max score
        total_max_score = 100.0  # Default
        if exam and exam.task_types:
            total_max_score = sum(task_type.max_score for task_type in exam.task_types)
        
        submission = Submission(
            id=str(uuid.uuid4()),
            session_id=session.id,
            student_id=session.student_id,
            status=SubmissionStatus.UPLOADED,
            max_score=total_max_score,
            submitted_at=datetime.utcnow()
        )
        db.add(submission)
        await db.flush()  # Flush to get the ID
        
        logger.info(f"Created empty submission {submission.id} for session {session.id}")
    
    # Update session - mark as submitted
    session.status = SessionStatus.SUBMITTED
    session.submitted_at = datetime.utcnow()
    
    # Update submission status - will be processed after exam ends
    # This ensures fast response and no overload during exam
    submission.status = SubmissionStatus.UPLOADED
    submission.submitted_at = datetime.utcnow()
    
    await db.commit()
    
    # Re-fetch submission with all relationships eagerly loaded
    submission_result = await db.execute(
        select(Submission)
        .where(Submission.id == submission.id)
        .options(
            selectinload(Submission.images),
            selectinload(Submission.scores)
        )
    )
    submission = submission_result.scalar_one()
    
    logger.info(f"Submission {submission.id} saved successfully. Will be processed after exam ends.")
    
    return submission


@router.get("/sessions/{session_id}/result")
async def get_session_result(
    session_id: str,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user)
):
    """Get submission result with exam and session details"""
    # Get the session with exam details
    session_result = await db.execute(
        select(ExamSession)
        .where(ExamSession.id == session_id)
        .options(selectinload(ExamSession.exam).selectinload(Exam.task_types))
    )
    session = session_result.scalar_one_or_none()
    
    if not session:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail="Session not found"
        )
    
    if session.student_id != current_user.id:
        raise HTTPException(
            status_code=status.HTTP_403_FORBIDDEN,
            detail="Access denied"
        )
    
    # Get submission
    submission_result = await db.execute(
        select(Submission)
        .where(Submission.session_id == session_id)
        .options(
            selectinload(Submission.images),
            selectinload(Submission.scores)
        )
    )
    submission = submission_result.scalar_one_or_none()
    
    if not submission:
        # Create empty submission if none exists (for expired sessions)
        exam = session.exam
        total_max_score = 100.0
        if exam and exam.task_types:
            total_max_score = sum(task_type.max_score for task_type in exam.task_types)
        
        submission = Submission(
            id=str(uuid.uuid4()),
            session_id=session.id,
            student_id=session.student_id,
            status=SubmissionStatus.UPLOADED,
            max_score=total_max_score,
            submitted_at=session.submitted_at or datetime.utcnow()
        )
        db.add(submission)
        await db.commit()
        
        # Re-fetch with relationships
        submission_result = await db.execute(
            select(Submission)
            .where(Submission.id == submission.id)
            .options(
                selectinload(Submission.images),
                selectinload(Submission.scores)
            )
        )
        submission = submission_result.scalar_one()
    
    # Count total attempts for this student on this exam
    attempts_result = await db.execute(
        select(ExamSession)
        .where(
            ExamSession.exam_id == session.exam_id,
            ExamSession.student_id == current_user.id
        )
    )
    total_attempts = len(attempts_result.scalars().all())
    
    # Return submission with exam and session context
    from app.schemas.submission import Submission as SubmissionSchema
    submission_data = SubmissionSchema.model_validate(submission)
    
    return {
        **submission_data.model_dump(),
        "exam": {
            "id": session.exam.id,
            "title": session.exam.title,
            "max_attempts": session.exam.max_attempts,
        },
        "session": {
            "id": session.id,
            "attempt_number": session.attempt_number,
            "total_attempts": total_attempts,
        }
    }


@router.get("/{submission_id}")
async def get_submission(
    submission_id: str,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_teacher)
):
    """Get submission by ID (teacher only) with enriched context"""
    # Load submission with images, scores, session->exam (with task types and variants) and student
    result = await db.execute(
        select(Submission)
        .where(Submission.id == submission_id)
        .options(
            selectinload(Submission.images),
            selectinload(Submission.scores),
            selectinload(Submission.session)
            .selectinload(ExamSession.exam)
            .selectinload(Exam.task_types)
            .selectinload(TaskType.variants),
            selectinload(Submission.student)
        )
    )
    submission = result.scalar_one_or_none()
    
    if not submission:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail="Submission not found"
        )

    # Build tasks context similar to get_session_variant
    tasks = []
    session = submission.session
    exam = session.exam if session else None
    if exam and session:
        for task_type in exam.task_types:
            variant_id = session.variant_assignments.get(task_type.id)
            variant = next((v for v in task_type.variants if v.id == variant_id), None)
            if variant:
                tasks.append({
                    "task_type": {
                        "id": task_type.id,
                        "title": task_type.title,
                        "description": task_type.description,
                        "order_index": task_type.order_index,
                        "max_score": task_type.max_score,
                        "formulas": task_type.formulas,
                        "units": task_type.units,
                    },
                    "variant": {
                        "id": variant.id,
                        "content": variant.content,
                        "parameters": variant.parameters,
                        "attachments": variant.attachments,
                    }
                })

    # Serialize base submission
    submission_data = SubmissionSchema.model_validate(submission)

    # Enriched payload
    return {
        **submission_data.model_dump(),
        "student_name": submission.student.full_name if submission.student else None,
        "student_isu": submission.student.isu if submission.student else None,
        "exam": {"id": exam.id, "title": exam.title} if exam else None,
        "tasks": tasks
    }


@router.post("/{submission_id}/approve")
async def approve_submission(
    submission_id: str,
    approve_data: SubmissionApprove,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_teacher)
):
    """Approve submission (teacher only)"""
    result = await db.execute(select(Submission).where(Submission.id == submission_id))
    submission = result.scalar_one_or_none()
    
    if not submission:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail="Submission not found"
        )
    
    submission.status = SubmissionStatus.APPROVED
    submission.final_score = submission.ai_score
    submission.teacher_comments = approve_data.teacher_comments
    submission.reviewed_by = current_user.id
    submission.reviewed_at = datetime.utcnow()
    
    await db.commit()
    
    return {"message": "Submission approved"}


@router.post("/{submission_id}/override-score")
async def override_submission_score(
    submission_id: str,
    override_data: SubmissionOverride,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_teacher)
):
    """Override submission score (teacher only)"""
    result = await db.execute(select(Submission).where(Submission.id == submission_id))
    submission = result.scalar_one_or_none()
    
    if not submission:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail="Submission not found"
        )
    
    submission.final_score = override_data.final_score
    submission.teacher_comments = override_data.teacher_comments
    submission.status = SubmissionStatus.APPROVED
    submission.reviewed_by = current_user.id
    submission.reviewed_at = datetime.utcnow()
    
    await db.commit()
    
    return {"message": "Score overridden successfully"}


@router.get("/images/{image_id}/view-url")
async def get_image_view_url(
    image_id: str,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user)
):
    """
    Get presigned URL to view an uploaded image
    Works for both teachers (reviewing) and students (their own submissions)
    """
    # Get image
    result = await db.execute(
        select(SubmissionImage)
        .where(SubmissionImage.id == image_id)
        .options(selectinload(SubmissionImage.submission))
    )
    image = result.scalar_one_or_none()
    
    if not image:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail="Image not found"
        )
    
    # Check permissions
    submission = image.submission
    is_owner = submission.student_id == current_user.id
    is_teacher = current_user.role in ["teacher", "admin"]
    
    if not (is_owner or is_teacher):
        raise HTTPException(
            status_code=status.HTTP_403_FORBIDDEN,
            detail="Access denied"
        )
    
    # Check if file is in S3 (starts with submissions/)
    if image.file_path.startswith("submissions/"):
        # S3 storage - generate presigned URL
        if not (settings.S3_ENDPOINT and settings.S3_ACCESS_KEY and settings.S3_SECRET_KEY):
            raise HTTPException(
                status_code=status.HTTP_503_SERVICE_UNAVAILABLE,
                detail="S3 storage not configured"
            )
        
        try:
            presigned_url = await storage_service.generate_presigned_url(
                s3_key=image.file_path,
                expires_in=300  # 5 minutes
            )
            
            return {
                "view_url": presigned_url,
                "expires_in": 300,
                "filename": image.filename,
                "mime_type": image.mime_type
            }
        except Exception as e:
            raise HTTPException(
                status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
                detail=f"Failed to generate view URL: {str(e)}"
            )
    else:
        # Legacy local storage - should not happen in production
        raise HTTPException(
            status_code=status.HTTP_503_SERVICE_UNAVAILABLE,
            detail="Image is stored in local storage. Please migrate to S3 storage."
        )


@router.post("/{submission_id}/regrade")
async def regrade_submission(
    submission_id: str,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_teacher)
):
    """
    Manually trigger AI re-grading for a submission (teacher only)
    Queues the submission for async processing via Celery
    """
    # Get submission with all relationships
    result = await db.execute(
        select(Submission)
        .where(Submission.id == submission_id)
        .options(
            selectinload(Submission.images),
            selectinload(Submission.session).selectinload(ExamSession.exam)
        )
    )
    submission = result.scalar_one_or_none()
    
    if not submission:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail="Submission not found"
        )
    
    # Get session
    session = submission.session
    if not session:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Submission has no associated session"
        )
    
    logger.info(f"Manual regrade triggered for submission {submission.id} by teacher {current_user.id}")
    
    # Update retry count and status
    submission.ai_retry_count = (submission.ai_retry_count or 0) + 1
    submission.status = SubmissionStatus.PROCESSING
    submission.ai_error = None  # Clear previous errors
    await db.commit()
    
    # Queue for async processing with high priority
    try:
        from app.tasks.grading import grade_submission_task
        task = grade_submission_task.apply_async(
            args=[submission_id],
            queue="grading",
            priority=10  # High priority for manual regrades
        )
        
        logger.info(f"Queued submission {submission_id} for regrade with task ID: {task.id}")
        
        return {
            "message": "Re-grading queued successfully",
            "submission_id": submission_id,
            "task_id": task.id,
            "status": "processing"
        }
    except Exception as e:
        logger.error(f"Failed to queue regrade for submission {submission_id}: {e}", exc_info=True)
        submission.status = SubmissionStatus.FLAGGED
        submission.ai_error = f"Failed to queue task: {str(e)}"
        await db.commit()
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=f"Failed to queue re-grading: {str(e)}"
        )


@router.get("/grading-status/{submission_id}")
async def get_grading_status(
    submission_id: str,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user)
):
    """
    Get AI grading status for a submission
    Returns current processing status and progress
    """
    result = await db.execute(
        select(Submission)
        .where(Submission.id == submission_id)
        .options(selectinload(Submission.session))
    )
    submission = result.scalar_one_or_none()
    
    if not submission:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail="Submission not found"
        )
    
    # Check permissions
    is_owner = submission.student_id == current_user.id
    is_teacher = current_user.role in ["teacher", "admin"]
    
    if not (is_owner or is_teacher):
        raise HTTPException(
            status_code=status.HTTP_403_FORBIDDEN,
            detail="Access denied"
        )
    
    # Calculate processing progress
    progress = 0
    status_message = ""
    
    if submission.status == SubmissionStatus.UPLOADED:
        progress = 10
        status_message = "В очереди на проверку"
    elif submission.status == SubmissionStatus.PROCESSING:
        progress = 50
        status_message = "Проверяется ИИ..."
        if submission.ai_request_started_at:
            elapsed = (datetime.utcnow() - submission.ai_request_started_at).total_seconds()
            if elapsed > 120:
                progress = 70
                status_message = "Финальная обработка..."
    elif submission.status == SubmissionStatus.PRELIMINARY:
        progress = 100
        status_message = "Проверено ИИ, ожидает подтверждения преподавателя"
    elif submission.status == SubmissionStatus.APPROVED:
        progress = 100
        status_message = "Проверено и одобрено"
    elif submission.status == SubmissionStatus.FLAGGED:
        progress = 50
        status_message = "Требует ручной проверки"
    
    return {
        "submission_id": submission_id,
        "status": submission.status,
        "progress": progress,
        "status_message": status_message,
        "ai_score": submission.ai_score,
        "final_score": submission.final_score,
        "max_score": submission.max_score,
        "ai_comments": submission.ai_comments,
        "ai_error": submission.ai_error,
        "ai_retry_count": submission.ai_retry_count,
        "processing_times": {
            "started_at": submission.ai_request_started_at,
            "completed_at": submission.ai_request_completed_at,
            "duration_seconds": submission.ai_request_duration_seconds
        }
    }


