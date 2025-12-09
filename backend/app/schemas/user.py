from pydantic import BaseModel, Field, field_serializer
from datetime import datetime, timezone
from typing import Optional
from app.models.user import UserRole


class UserBase(BaseModel):
    """Base user schema"""
    isu: str = Field(..., min_length=6, max_length=6, pattern=r"^\d{6}$")
    full_name: str


class UserCreate(UserBase):
    """Schema for creating a user"""
    password: str = Field(..., min_length=6, max_length=128)
    role: UserRole = UserRole.STUDENT
    is_active: bool = True
    is_verified: bool = False
    pd_consent: bool = False
    pd_consent_version: Optional[str] = None
    terms_version: Optional[str] = None
    privacy_version: Optional[str] = None


class UserLogin(BaseModel):
    """Schema for user login"""
    isu: str = Field(..., min_length=6, max_length=6)
    password: str


class User(UserBase):
    """User response schema"""
    id: str
    role: UserRole
    is_active: bool
    is_verified: bool
    created_at: datetime
    pd_consent: bool
    pd_consent_at: Optional[datetime] = None
    pd_consent_version: Optional[str] = None
    terms_accepted_at: Optional[datetime] = None
    terms_version: Optional[str] = None
    privacy_version: Optional[str] = None
    
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


class Token(BaseModel):
    """JWT token response"""
    access_token: str
    token_type: str = "bearer"
    user: User


class UserUpdate(BaseModel):
    """Schema for updating user"""
    full_name: Optional[str] = None
    password: Optional[str] = Field(None, min_length=6, max_length=128)


class AdminUserCreate(UserBase):
    """Schema for admin-created user"""
    password: str = Field(..., min_length=6, max_length=128)
    role: UserRole = UserRole.STUDENT
    is_active: bool = True
    is_verified: bool = False


class AdminUserUpdate(BaseModel):
    """Schema for admin updating user"""
    full_name: Optional[str] = None
    password: Optional[str] = Field(None, min_length=6, max_length=128)
    role: Optional[UserRole] = None
    is_active: Optional[bool] = None
    is_verified: Optional[bool] = None


