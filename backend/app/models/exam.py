from sqlalchemy import Column, String, DateTime, Integer, Float, Boolean, JSON, ForeignKey, Text, Enum as SQLEnum
from sqlalchemy.orm import relationship
from datetime import datetime
import enum

from app.db.base import Base


class ExamStatus(str, enum.Enum):
    """Exam status enumeration"""
    DRAFT = "draft"
    PUBLISHED = "published"
    ACTIVE = "active"
    COMPLETED = "completed"
    ARCHIVED = "archived"


class DifficultyLevel(str, enum.Enum):
    """Task difficulty level"""
    EASY = "easy"
    MEDIUM = "medium"
    HARD = "hard"


class Exam(Base):
    """Exam model"""
    __tablename__ = "exams"
    
    id = Column(String, primary_key=True, index=True)  # UUID
    title = Column(String, nullable=False)
    description = Column(Text, nullable=True)
    
    # Timing
    start_time = Column(DateTime, nullable=False)  # Stored in UTC
    end_time = Column(DateTime, nullable=False)    # Stored in UTC
    duration_minutes = Column(Integer, nullable=False)
    timezone = Column(String, default="Europe/Moscow")  # GMT+3 - university timezone
    
    # Settings
    max_attempts = Column(Integer, default=1)
    allow_breaks = Column(Boolean, default=False)
    break_duration_minutes = Column(Integer, default=0)
    auto_save_interval = Column(Integer, default=10)
    
    # Metadata
    status = Column(SQLEnum(ExamStatus), default=ExamStatus.DRAFT)
    created_by = Column(String, ForeignKey("users.id"), nullable=False)
    created_at = Column(DateTime, default=datetime.utcnow)
    updated_at = Column(DateTime, default=datetime.utcnow, onupdate=datetime.utcnow)
    published_at = Column(DateTime, nullable=True)
    
    # Settings JSON
    settings = Column(JSON, default={})
    
    # Relationships
    task_types = relationship("TaskType", back_populates="exam", cascade="all, delete-orphan")
    sessions = relationship("ExamSession", back_populates="exam", cascade="all, delete-orphan")


class TaskType(Base):
    """Task type (problem template) model"""
    __tablename__ = "task_types"
    
    id = Column(String, primary_key=True, index=True)  # UUID
    exam_id = Column(String, ForeignKey("exams.id"), nullable=False)
    
    # Basic info
    title = Column(String, nullable=False)
    description = Column(Text, nullable=False)
    order_index = Column(Integer, nullable=False)
    
    # Grading
    max_score = Column(Float, nullable=False)
    rubric = Column(JSON, nullable=False)  # Grading criteria
    
    # Metadata
    difficulty = Column(SQLEnum(DifficultyLevel), default=DifficultyLevel.MEDIUM)
    taxonomy_tags = Column(JSON, default=[])  # e.g., ["thermodynamics", "acid-base"]
    
    # LaTeX formulas and units
    formulas = Column(JSON, default=[])
    units = Column(JSON, default=[])
    
    # Validation rules for chemistry
    validation_rules = Column(JSON, default={})  # Balance equations, dimensional analysis, etc.
    
    created_at = Column(DateTime, default=datetime.utcnow)
    updated_at = Column(DateTime, default=datetime.utcnow, onupdate=datetime.utcnow)
    
    # Relationships
    exam = relationship("Exam", back_populates="task_types")
    variants = relationship("TaskVariant", back_populates="task_type", cascade="all, delete-orphan")


class TaskVariant(Base):
    """Specific variant of a task"""
    __tablename__ = "task_variants"
    
    id = Column(String, primary_key=True, index=True)  # UUID
    task_type_id = Column(String, ForeignKey("task_types.id"), nullable=False)
    
    # Variant content
    content = Column(Text, nullable=False)  # Can include LaTeX
    parameters = Column(JSON, default={})  # For numerical generation
    
    # Solution
    reference_solution = Column(Text, nullable=True)
    reference_answer = Column(String, nullable=True)
    answer_tolerance = Column(Float, default=0.01)  # For numerical answers
    
    # Images/attachments
    attachments = Column(JSON, default=[])
    
    created_at = Column(DateTime, default=datetime.utcnow)
    
    # Relationships
    task_type = relationship("TaskType", back_populates="variants")


