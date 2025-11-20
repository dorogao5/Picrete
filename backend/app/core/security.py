from datetime import datetime, timedelta
from typing import Any, Dict, Optional, Union
from jose import jwt, JWTError
from passlib.context import CryptContext
from app.core.config import settings

# Use Argon2 for modern, fast password hashing without byte limitations
# Argon2 won the Password Hashing Competition and is recommended for new applications
pwd_context = CryptContext(
    schemes=["argon2"],
    deprecated="auto",
    argon2__memory_cost=102400,  # 100 MB
    argon2__time_cost=2,          # 2 iterations
    argon2__parallelism=8,        # 8 parallel threads
)

ALGORITHM = settings.ALGORITHM


def create_access_token(subject: Union[str, Any], expires_delta: timedelta = None) -> str:
    """Create JWT access token"""
    if expires_delta:
        expire = datetime.utcnow() + expires_delta
    else:
        expire = datetime.utcnow() + timedelta(
            minutes=settings.ACCESS_TOKEN_EXPIRE_MINUTES
        )
    
    to_encode = {"exp": expire, "sub": str(subject)}
    encoded_jwt = jwt.encode(to_encode, settings.SECRET_KEY, algorithm=ALGORITHM)
    return encoded_jwt


def verify_password(plain_password: str, hashed_password: str) -> bool:
    """Verify password against hash"""
    return pwd_context.verify(plain_password, hashed_password)


def get_password_hash(password: str) -> str:
    """Hash password using Argon2"""
    return pwd_context.hash(password)


def verify_token(token: str) -> Optional[Dict[str, Any]]:
    """Decode and validate JWT access token.

    Returns payload dict if token is valid, otherwise None.
    """
    try:
        payload = jwt.decode(token, settings.SECRET_KEY, algorithms=[ALGORITHM])
        # Ensure required claims exist
        if not isinstance(payload, dict):
            return None
        if "sub" not in payload or "exp" not in payload:
            return None
        return payload
    except JWTError:
        return None

