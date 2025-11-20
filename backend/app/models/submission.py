from sqlalchemy import Column, String, DateTime, Integer, Float, Boolean, JSON, ForeignKey, Text, Enum as SQLEnum
from sqlalchemy.orm import relationship
from datetime import datetime
import enum

from app.db.base import Base


class SessionStatus(str, enum.Enum):
    """Exam session status"""
    ACTIVE = "active"
    SUBMITTED = "submitted"
    EXPIRED = "expired"
    GRADED = "graded"


class SubmissionStatus(str, enum.Enum):
    """Submission status"""
    UPLOADED = "uploaded"
    PROCESSING = "processing"
    PRELIMINARY = "preliminary"
    APPROVED = "approved"
    FLAGGED = "flagged"
    REJECTED = "rejected"


class ExamSession(Base):
    """Student exam session"""
    __tablename__ = "exam_sessions"
    
    id = Column(String, primary_key=True, index=True)  # UUID
    exam_id = Column(String, ForeignKey("exams.id"), nullable=False)
    student_id = Column(String, ForeignKey("users.id"), nullable=False)
    
    # Session details
    variant_seed = Column(Integer, nullable=False)  # Random seed for variant selection
    variant_assignments = Column(JSON, nullable=False)  # {task_type_id: variant_id}
    
    # Timing
    started_at = Column(DateTime, nullable=False, default=datetime.utcnow)
    submitted_at = Column(DateTime, nullable=True)
    expires_at = Column(DateTime, nullable=False)
    
    # Status
    status = Column(SQLEnum(SessionStatus), default=SessionStatus.ACTIVE)
    attempt_number = Column(Integer, default=1)
    
    # Security
    ip_address = Column(String, nullable=True)
    user_agent = Column(String, nullable=True)
    
    # Auto-save data
    last_auto_save = Column(DateTime, nullable=True)
    auto_save_data = Column(JSON, default={})
    
    created_at = Column(DateTime, default=datetime.utcnow)
    updated_at = Column(DateTime, default=datetime.utcnow, onupdate=datetime.utcnow)
    
    # Relationships
    exam = relationship("Exam", back_populates="sessions")
    student = relationship("User", back_populates="exam_sessions", foreign_keys=[student_id])
    submissions = relationship("Submission", back_populates="session", cascade="all, delete-orphan")


class Submission(Base):
    """Student submission for exam"""
    __tablename__ = "submissions"
    
    id = Column(String, primary_key=True, index=True)  # UUID
    session_id = Column(String, ForeignKey("exam_sessions.id"), nullable=False)
    student_id = Column(String, ForeignKey("users.id"), nullable=False)
    
    # Submission details
    submitted_at = Column(DateTime, nullable=False, default=datetime.utcnow)
    status = Column(SQLEnum(SubmissionStatus), default=SubmissionStatus.UPLOADED)
    
    # Grading
    ai_score = Column(Float, nullable=True)
    final_score = Column(Float, nullable=True)
    max_score = Column(Float, nullable=False)
    
    # AI analysis
    ai_analysis = Column(JSON, nullable=True)  # Detailed breakdown from GPT
    ai_comments = Column(Text, nullable=True)
    ai_processed_at = Column(DateTime, nullable=True)
    ai_request_started_at = Column(DateTime, nullable=True)  # When AI request was sent
    ai_request_completed_at = Column(DateTime, nullable=True)  # When AI response received
    ai_request_duration_seconds = Column(Float, nullable=True)  # Duration of AI request
    ai_error = Column(Text, nullable=True)  # Error message if AI processing failed
    ai_retry_count = Column(Integer, default=0)  # Number of retries attempted
    
    # Teacher review
    teacher_comments = Column(Text, nullable=True)
    reviewed_by = Column(String, ForeignKey("users.id"), nullable=True)
    reviewed_at = Column(DateTime, nullable=True)
    
    # Flags and anomalies
    is_flagged = Column(Boolean, default=False)
    flag_reasons = Column(JSON, default=[])  # ["plagiarism", "unreadable", "suspicious_timing"]
    anomaly_scores = Column(JSON, default={})
    
    # Metadata
    files_hash = Column(String, nullable=True)  # Hash of all submitted files
    
    created_at = Column(DateTime, default=datetime.utcnow)
    updated_at = Column(DateTime, default=datetime.utcnow, onupdate=datetime.utcnow)
    
    # Relationships
    session = relationship("ExamSession", back_populates="submissions")
    student = relationship("User", back_populates="submissions", foreign_keys=[student_id])
    images = relationship("SubmissionImage", back_populates="submission", cascade="all, delete-orphan")
    scores = relationship("SubmissionScore", back_populates="submission", cascade="all, delete-orphan")


class SubmissionImage(Base):
    """Uploaded image for submission"""
    __tablename__ = "submission_images"
    
    id = Column(String, primary_key=True, index=True)  # UUID
    submission_id = Column(String, ForeignKey("submissions.id"), nullable=False)
    
    # File details
    filename = Column(String, nullable=False)
    file_path = Column(String, nullable=False)  # S3 key or local path
    file_size = Column(Integer, nullable=False)
    mime_type = Column(String, nullable=False)
    
    # Processing
    is_processed = Column(Boolean, default=False)
    ocr_text = Column(Text, nullable=True)
    quality_score = Column(Float, nullable=True)  # Readability score
    
    # Order
    order_index = Column(Integer, nullable=False)
    
    # Image hash for plagiarism detection
    perceptual_hash = Column(String, nullable=True)
    
    uploaded_at = Column(DateTime, default=datetime.utcnow)
    processed_at = Column(DateTime, nullable=True)
    
    # Relationships
    submission = relationship("Submission", back_populates="images")


class SubmissionScore(Base):
    """Detailed score breakdown per criterion"""
    __tablename__ = "submission_scores"
    
    id = Column(String, primary_key=True, index=True)  # UUID
    submission_id = Column(String, ForeignKey("submissions.id"), nullable=False)
    
    # Criterion details
    task_type_id = Column(String, ForeignKey("task_types.id"), nullable=False)
    criterion_name = Column(String, nullable=False)
    criterion_description = Column(Text, nullable=True)
    
    # Scores
    ai_score = Column(Float, nullable=True)
    final_score = Column(Float, nullable=True)
    max_score = Column(Float, nullable=False)
    
    # Comments
    ai_comment = Column(Text, nullable=True)
    teacher_comment = Column(Text, nullable=True)
    
    created_at = Column(DateTime, default=datetime.utcnow)
    updated_at = Column(DateTime, default=datetime.utcnow, onupdate=datetime.utcnow)
    
    # Relationships
    submission = relationship("Submission", back_populates="scores")


