from pydantic import BaseModel, Field, field_serializer
from datetime import datetime, timezone
from typing import Optional, List, Dict, Any
from app.models.submission import SessionStatus, SubmissionStatus


class ExamSessionBase(BaseModel):
    """Base exam session schema"""
    exam_id: str
    student_id: str


class ExamSessionCreate(ExamSessionBase):
    """Schema for creating exam session"""
    pass


class ExamSession(ExamSessionBase):
    """Exam session response schema"""
    id: str
    variant_seed: int
    variant_assignments: Dict[str, str]
    started_at: datetime
    submitted_at: Optional[datetime] = None
    expires_at: datetime
    status: SessionStatus
    attempt_number: int
    
    # Serialize datetime fields as UTC with 'Z' suffix
    @field_serializer('started_at', 'submitted_at', 'expires_at')
    def serialize_datetime(self, dt: Optional[datetime], _info) -> Optional[str]:
        if dt is None:
            return None
        # If naive datetime, assume it's UTC
        if dt.tzinfo is None:
            dt = dt.replace(tzinfo=timezone.utc)
        return dt.isoformat()
    
    class Config:
        from_attributes = True


class SubmissionImageBase(BaseModel):
    """Base submission image schema"""
    filename: str
    order_index: int


class SubmissionImage(SubmissionImageBase):
    """Submission image response schema"""
    id: str
    file_path: str
    file_size: int
    mime_type: str
    is_processed: bool
    quality_score: Optional[float] = None
    uploaded_at: datetime
    
    # Serialize datetime fields as UTC with 'Z' suffix
    @field_serializer('uploaded_at')
    def serialize_datetime(self, dt: Optional[datetime], _info) -> Optional[str]:
        if dt is None:
            return None
        # If naive datetime, assume it's UTC
        if dt.tzinfo is None:
            dt = dt.replace(tzinfo=timezone.utc)
        return dt.isoformat()
    
    class Config:
        from_attributes = True


class SubmissionScoreBase(BaseModel):
    """Base submission score schema"""
    task_type_id: str
    criterion_name: str
    criterion_description: Optional[str] = None
    max_score: float


class SubmissionScore(SubmissionScoreBase):
    """Submission score response schema"""
    id: str
    submission_id: str
    ai_score: Optional[float] = None
    final_score: Optional[float] = None
    ai_comment: Optional[str] = None
    teacher_comment: Optional[str] = None
    
    class Config:
        from_attributes = True


class SubmissionBase(BaseModel):
    """Base submission schema"""
    session_id: str


class SubmissionCreate(SubmissionBase):
    """Schema for creating submission"""
    pass


class Submission(SubmissionBase):
    """Submission response schema"""
    id: str
    student_id: str
    submitted_at: datetime
    status: SubmissionStatus
    ai_score: Optional[float] = None
    final_score: Optional[float] = None
    max_score: float
    ai_analysis: Optional[Dict[str, Any]] = None
    ai_comments: Optional[str] = None
    teacher_comments: Optional[str] = None
    is_flagged: bool
    flag_reasons: List[str] = []
    reviewed_by: Optional[str] = None
    reviewed_at: Optional[datetime] = None
    images: List[SubmissionImage] = []
    scores: List[SubmissionScore] = []
    
    # Serialize datetime fields as UTC with 'Z' suffix
    @field_serializer('submitted_at', 'reviewed_at')
    def serialize_datetime(self, dt: Optional[datetime], _info) -> Optional[str]:
        if dt is None:
            return None
        # If naive datetime, assume it's UTC
        if dt.tzinfo is None:
            dt = dt.replace(tzinfo=timezone.utc)
        return dt.isoformat()
    
    class Config:
        from_attributes = True


class SubmissionUpdate(BaseModel):
    """Schema for updating submission"""
    final_score: Optional[float] = None
    teacher_comments: Optional[str] = None
    status: Optional[SubmissionStatus] = None


class SubmissionApprove(BaseModel):
    """Schema for approving submission"""
    teacher_comments: Optional[str] = None


class SubmissionOverride(BaseModel):
    """Schema for overriding submission score"""
    final_score: float
    teacher_comments: str
    scores: Optional[List[Dict[str, Any]]] = None


