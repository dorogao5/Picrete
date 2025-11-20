from fastapi import APIRouter, Depends, HTTPException, status, Query
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy import select, func
from sqlalchemy.orm import selectinload
from typing import List
import uuid
from datetime import datetime

from app.api.deps import get_db, get_current_user, get_current_teacher
from app.models.user import User, UserRole
from app.models.exam import Exam, ExamStatus, TaskType, TaskVariant
from app.models.submission import ExamSession, Submission, SessionStatus, SubmissionStatus
from app.schemas.exam import (
    Exam as ExamSchema,
    ExamCreate,
    ExamUpdate,
    ExamSummary,
    TaskTypeCreate,
    TaskVariantCreate
)

router = APIRouter()


@router.post("/", response_model=ExamSchema, status_code=status.HTTP_201_CREATED)
async def create_exam(
    exam_in: ExamCreate,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_teacher)
):
    """Create a new exam (teacher/admin only)"""
    # Convert timezone-aware datetimes to naive UTC datetimes for database storage
    start_time = exam_in.start_time.replace(tzinfo=None) if exam_in.start_time.tzinfo else exam_in.start_time
    end_time = exam_in.end_time.replace(tzinfo=None) if exam_in.end_time.tzinfo else exam_in.end_time
    
    exam = Exam(
        id=str(uuid.uuid4()),
        title=exam_in.title,
        description=exam_in.description,
        start_time=start_time,
        end_time=end_time,
        duration_minutes=exam_in.duration_minutes,
        timezone=exam_in.timezone,
        max_attempts=exam_in.max_attempts,
        allow_breaks=exam_in.allow_breaks,
        break_duration_minutes=exam_in.break_duration_minutes,
        auto_save_interval=exam_in.auto_save_interval,
        settings=exam_in.settings,
        status=ExamStatus.DRAFT,
        created_by=current_user.id,
    )
    
    db.add(exam)
    
    # Add task types and variants
    for task_type_data in exam_in.task_types:
        task_type = TaskType(
            id=str(uuid.uuid4()),
            exam_id=exam.id,
            title=task_type_data.title,
            description=task_type_data.description,
            order_index=task_type_data.order_index,
            max_score=task_type_data.max_score,
            rubric=task_type_data.rubric,
            difficulty=task_type_data.difficulty,
            taxonomy_tags=task_type_data.taxonomy_tags,
            formulas=task_type_data.formulas,
            units=task_type_data.units,
            validation_rules=task_type_data.validation_rules,
        )
        db.add(task_type)
        
        for variant_data in task_type_data.variants:
            variant = TaskVariant(
                id=str(uuid.uuid4()),
                task_type_id=task_type.id,
                content=variant_data.content,
                parameters=variant_data.parameters,
                reference_solution=variant_data.reference_solution,
                reference_answer=variant_data.reference_answer,
                answer_tolerance=variant_data.answer_tolerance,
                attachments=variant_data.attachments,
            )
            db.add(variant)
    
    await db.commit()
    await db.refresh(exam)
    
    # Load relationships
    result = await db.execute(
        select(Exam)
        .where(Exam.id == exam.id)
        .options(selectinload(Exam.task_types).selectinload(TaskType.variants))
    )
    exam = result.scalar_one()
    
    return exam


@router.get("/", response_model=List[ExamSummary])
async def list_exams(
    skip: int = 0,
    limit: int = 100,
    status: ExamStatus = None,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user)
):
    """List exams"""
    query = select(Exam)
    
    # Teachers see all exams, students only published/active
    if current_user.role == UserRole.STUDENT:
        query = query.where(Exam.status.in_([ExamStatus.PUBLISHED, ExamStatus.ACTIVE]))
    
    if status:
        query = query.where(Exam.status == status)
    
    query = query.offset(skip).limit(limit).order_by(Exam.start_time.desc())
    
    result = await db.execute(query)
    exams = result.scalars().all()
    
    # Get statistics for each exam
    summaries = []
    for exam in exams:
        # Count tasks
        task_count_result = await db.execute(
            select(func.count(TaskType.id)).where(TaskType.exam_id == exam.id)
        )
        task_count = task_count_result.scalar()
        
        # Count students (unique)
        student_count_result = await db.execute(
            select(func.count(func.distinct(ExamSession.student_id)))
            .where(ExamSession.exam_id == exam.id)
        )
        student_count = student_count_result.scalar()
        
        # Count pending submissions
        pending_count_result = await db.execute(
            select(func.count(Submission.id))
            .join(ExamSession)
            .where(
                ExamSession.exam_id == exam.id,
                Submission.status == SubmissionStatus.PRELIMINARY
            )
        )
        pending_count = pending_count_result.scalar()
        
        summaries.append(ExamSummary(
            id=exam.id,
            title=exam.title,
            start_time=exam.start_time,
            end_time=exam.end_time,
            duration_minutes=exam.duration_minutes,
            status=exam.status,
            task_count=task_count or 0,
            student_count=student_count or 0,
            pending_count=pending_count or 0,
        ))
    
    return summaries


@router.get("/{exam_id}", response_model=ExamSchema)
async def get_exam(
    exam_id: str,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_user)
):
    """Get exam by ID"""
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
    
    # Students can only see published exams
    if current_user.role == UserRole.STUDENT and exam.status not in [ExamStatus.PUBLISHED, ExamStatus.ACTIVE]:
        raise HTTPException(
            status_code=status.HTTP_403_FORBIDDEN,
            detail="Access denied"
        )
    
    return exam


@router.patch("/{exam_id}", response_model=ExamSchema)
async def update_exam(
    exam_id: str,
    exam_update: ExamUpdate,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_teacher)
):
    """Update exam (teacher/admin only)"""
    result = await db.execute(select(Exam).where(Exam.id == exam_id))
    exam = result.scalar_one_or_none()
    
    if not exam:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail="Exam not found"
        )
    
    update_data = exam_update.dict(exclude_unset=True)
    
    # Convert timezone-aware datetimes to naive UTC datetimes for database storage
    if "start_time" in update_data and update_data["start_time"] is not None:
        if update_data["start_time"].tzinfo:
            update_data["start_time"] = update_data["start_time"].replace(tzinfo=None)
    if "end_time" in update_data and update_data["end_time"] is not None:
        if update_data["end_time"].tzinfo:
            update_data["end_time"] = update_data["end_time"].replace(tzinfo=None)
    
    for field, value in update_data.items():
        setattr(exam, field, value)
    
    await db.commit()
    await db.refresh(exam)
    
    # Load relationships for response serialization
    result = await db.execute(
        select(Exam)
        .where(Exam.id == exam_id)
        .options(selectinload(Exam.task_types).selectinload(TaskType.variants))
    )
    exam = result.scalar_one()
    
    return exam


@router.delete("/{exam_id}", status_code=status.HTTP_204_NO_CONTENT)
async def delete_exam(
    exam_id: str,
    force_delete: bool = Query(False, description="Force delete even with existing submissions"),
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_teacher)
):
    """Delete exam (teacher/admin only)
    
    If force_delete is True, will delete the exam along with all student sessions,
    submissions, grades, and uploaded files. This action is irreversible.
    """
    result = await db.execute(select(Exam).where(Exam.id == exam_id))
    exam = result.scalar_one_or_none()
    
    if not exam:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail="Exam not found"
        )
    
    # Check if exam has submissions
    submissions_result = await db.execute(
        select(func.count(ExamSession.id)).where(ExamSession.exam_id == exam_id)
    )
    submissions_count = submissions_result.scalar()
    
    if submissions_count > 0 and not force_delete:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail=f"Cannot delete exam with {submissions_count} existing submission(s). Use force_delete=true to delete anyway."
        )
    
    # Delete exam (cascade will delete all related sessions, submissions, and images)
    await db.delete(exam)
    await db.commit()
    
    return None


@router.post("/{exam_id}/publish", response_model=ExamSchema)
async def publish_exam(
    exam_id: str,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_teacher)
):
    """Publish exam (teacher/admin only)"""
    result = await db.execute(select(Exam).where(Exam.id == exam_id))
    exam = result.scalar_one_or_none()
    
    if not exam:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail="Exam not found"
        )
    
    if exam.status != ExamStatus.DRAFT:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Exam is not in draft status"
        )
    
    # Validate exam has tasks
    task_count_result = await db.execute(
        select(func.count(TaskType.id)).where(TaskType.exam_id == exam.id)
    )
    task_count = task_count_result.scalar()
    
    if task_count == 0:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Exam must have at least one task type"
        )
    
    exam.status = ExamStatus.PUBLISHED
    exam.published_at = datetime.utcnow()
    
    await db.commit()
    await db.refresh(exam)
    
    # Load relationships for response serialization
    result = await db.execute(
        select(Exam)
        .where(Exam.id == exam_id)
        .options(selectinload(Exam.task_types).selectinload(TaskType.variants))
    )
    exam = result.scalar_one()
    
    return exam


@router.post("/{exam_id}/task-types", status_code=status.HTTP_201_CREATED)
async def add_task_type(
    exam_id: str,
    task_type_in: TaskTypeCreate,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_teacher)
):
    """Add task type to exam (teacher/admin only)"""
    result = await db.execute(select(Exam).where(Exam.id == exam_id))
    exam = result.scalar_one_or_none()
    
    if not exam:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail="Exam not found"
        )
    
    task_type = TaskType(
        id=str(uuid.uuid4()),
        exam_id=exam_id,
        title=task_type_in.title,
        description=task_type_in.description,
        order_index=task_type_in.order_index,
        max_score=task_type_in.max_score,
        rubric=task_type_in.rubric,
        difficulty=task_type_in.difficulty,
        taxonomy_tags=task_type_in.taxonomy_tags,
        formulas=task_type_in.formulas,
        units=task_type_in.units,
        validation_rules=task_type_in.validation_rules,
    )
    db.add(task_type)
    
    # Add variants
    for variant_data in task_type_in.variants:
        variant = TaskVariant(
            id=str(uuid.uuid4()),
            task_type_id=task_type.id,
            content=variant_data.content,
            parameters=variant_data.parameters,
            reference_solution=variant_data.reference_solution,
            reference_answer=variant_data.reference_answer,
            answer_tolerance=variant_data.answer_tolerance,
            attachments=variant_data.attachments,
        )
        db.add(variant)
    
    await db.commit()
    
    return {"message": "Task type added successfully", "task_type_id": task_type.id}


@router.get("/{exam_id}/submissions")
async def list_exam_submissions(
    exam_id: str,
    status: SubmissionStatus = Query(None),
    skip: int = 0,
    limit: int = 100,
    db: AsyncSession = Depends(get_db),
    current_user: User = Depends(get_current_teacher)
):
    """List submissions for an exam (teacher/admin only)"""
    query = (
        select(Submission, User)
        .select_from(Submission)
        .join(ExamSession, Submission.session_id == ExamSession.id)
        .join(User, User.id == Submission.student_id)
        .where(ExamSession.exam_id == exam_id)
    )
    
    if status:
        query = query.where(Submission.status == status)
    
    query = query.offset(skip).limit(limit).order_by(Submission.submitted_at.desc())
    
    result = await db.execute(query)
    rows = result.all()
    
    # Format response with student information
    submissions_data = []
    for submission, user in rows:
        submissions_data.append({
            "id": submission.id,
            "student_id": submission.student_id,
            "student_isu": user.isu,
            "student_name": user.full_name,
            "submitted_at": submission.submitted_at,
            "status": submission.status,
            "ai_score": submission.ai_score,
            "final_score": submission.final_score,
            "max_score": submission.max_score,
        })
    
    return submissions_data


