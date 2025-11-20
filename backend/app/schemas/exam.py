from pydantic import BaseModel, Field, field_serializer
from datetime import datetime, timezone
from typing import Optional, List, Dict, Any
from app.models.exam import ExamStatus, DifficultyLevel


class TaskVariantBase(BaseModel):
    """Base task variant schema"""
    content: str
    parameters: Dict[str, Any] = {}
    reference_solution: Optional[str] = None
    reference_answer: Optional[str] = None
    answer_tolerance: float = 0.01
    attachments: List[str] = []


class TaskVariantCreate(TaskVariantBase):
    """Schema for creating task variant"""
    pass


class TaskVariant(TaskVariantBase):
    """Task variant response schema"""
    id: str
    task_type_id: str
    created_at: datetime
    
    # Serialize datetime fields as UTC with 'Z' suffix
    @field_serializer('created_at')
    def serialize_datetime(self, dt: Optional[datetime], _info) -> Optional[str]:
        if dt is None:
            return None
        # If naive datetime, assume it's UTC
        if dt.tzinfo is None:
            dt = dt.replace(tzinfo=timezone.utc)
        return dt.isoformat()
    
    class Config:
        from_attributes = True


class TaskTypeBase(BaseModel):
    """Base task type schema"""
    title: str
    description: str
    order_index: int
    max_score: float
    rubric: Dict[str, Any]  # Grading criteria
    difficulty: DifficultyLevel = DifficultyLevel.MEDIUM
    taxonomy_tags: List[str] = []
    formulas: List[str] = []
    units: List[Dict[str, str]] = []
    validation_rules: Dict[str, Any] = {}


class TaskTypeCreate(TaskTypeBase):
    """Schema for creating task type"""
    variants: List[TaskVariantCreate] = []


class TaskType(TaskTypeBase):
    """Task type response schema"""
    id: str
    exam_id: str
    created_at: datetime
    updated_at: datetime
    variants: List[TaskVariant] = []
    
    # Serialize datetime fields as UTC with 'Z' suffix
    @field_serializer('created_at', 'updated_at')
    def serialize_datetime(self, dt: Optional[datetime], _info) -> Optional[str]:
        if dt is None:
            return None
        # If naive datetime, assume it's UTC
        if dt.tzinfo is None:
            dt = dt.replace(tzinfo=timezone.utc)
        return dt.isoformat()
    
    class Config:
        from_attributes = True


class ExamBase(BaseModel):
    """Base exam schema"""
    title: str
    description: Optional[str] = None
    start_time: datetime
    end_time: datetime
    duration_minutes: int
    timezone: str = "Europe/Moscow"  # GMT+3 - default timezone 
    max_attempts: int = 1
    allow_breaks: bool = False
    break_duration_minutes: int = 0
    auto_save_interval: int = 10
    settings: Dict[str, Any] = {}


class ExamCreate(ExamBase):
    """Schema for creating exam"""
    task_types: List[TaskTypeCreate] = []


class ExamUpdate(BaseModel):
    """Schema for updating exam"""
    title: Optional[str] = None
    description: Optional[str] = None
    start_time: Optional[datetime] = None
    end_time: Optional[datetime] = None
    duration_minutes: Optional[int] = None
    status: Optional[ExamStatus] = None
    settings: Optional[Dict[str, Any]] = None


class Exam(ExamBase):
    """Exam response schema"""
    id: str
    status: ExamStatus
    created_by: str
    created_at: datetime
    updated_at: datetime
    published_at: Optional[datetime] = None
    task_types: List[TaskType] = []
    
    # Serialize datetime fields as UTC with 'Z' suffix
    @field_serializer('start_time', 'end_time', 'created_at', 'updated_at', 'published_at')
    def serialize_datetime(self, dt: Optional[datetime], _info) -> Optional[str]:
        if dt is None:
            return None
        # If naive datetime, assume it's UTC
        if dt.tzinfo is None:
            dt = dt.replace(tzinfo=timezone.utc)
        return dt.isoformat()
    
    class Config:
        from_attributes = True


class ExamSummary(BaseModel):
    """Summary of exam for listings"""
    id: str
    title: str
    start_time: datetime
    end_time: datetime
    duration_minutes: int
    status: ExamStatus
    task_count: int
    student_count: int
    pending_count: int
    
    # Serialize datetime fields as UTC with 'Z' suffix
    @field_serializer('start_time', 'end_time')
    def serialize_datetime(self, dt: Optional[datetime], _info) -> Optional[str]:
        if dt is None:
            return None
        # If naive datetime, assume it's UTC
        if dt.tzinfo is None:
            dt = dt.replace(tzinfo=timezone.utc)
        return dt.isoformat()
    
    class Config:
        from_attributes = True


