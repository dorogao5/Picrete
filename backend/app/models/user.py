from sqlalchemy import Column, String, Boolean, Enum as SQLEnum, DateTime
from sqlalchemy.orm import relationship
from datetime import datetime
import enum

from app.db.base import Base


class UserRole(str, enum.Enum):
    """User role enumeration"""
    ADMIN = "admin"
    TEACHER = "teacher"
    ASSISTANT = "assistant"
    STUDENT = "student"


class User(Base):
    """User model"""
    __tablename__ = "users"
    
    id = Column(String, primary_key=True, index=True)  # UUID
    isu = Column(String(6), unique=True, index=True, nullable=False)
    hashed_password = Column(String, nullable=False)
    full_name = Column(String, nullable=False)
    role = Column(SQLEnum(UserRole), nullable=False, default=UserRole.STUDENT)
    is_active = Column(Boolean, default=True)
    is_verified = Column(Boolean, default=False)
    pd_consent = Column(Boolean, nullable=False, default=False)
    pd_consent_at = Column(DateTime, nullable=True)
    pd_consent_version = Column(String, nullable=True)
    terms_accepted_at = Column(DateTime, nullable=True)
    terms_version = Column(String, nullable=True)
    privacy_version = Column(String, nullable=True)
    
    created_at = Column(DateTime, default=datetime.utcnow)
    updated_at = Column(DateTime, default=datetime.utcnow, onupdate=datetime.utcnow)
    
    # Relationships
    exam_sessions = relationship("ExamSession", back_populates="student", foreign_keys="ExamSession.student_id")
    submissions = relationship("Submission", back_populates="student", foreign_keys="Submission.student_id")


