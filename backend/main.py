from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware
from contextlib import asynccontextmanager
import logging

from app.api.v1.api import api_router
from app.core.config import settings
from app.db.session import engine
from app.db.base import Base
from app.core.redis_client import redis_client

# Configure logging
logging.basicConfig(
    level=logging.INFO,
    format='%(asctime)s - %(name)s - %(levelname)s - %(message)s'
)
logger = logging.getLogger(__name__)


@asynccontextmanager
async def lifespan(app: FastAPI):
    """Application lifespan events"""
    # Startup
    logger.info("Starting up Picrete API...")
    
    # Initialize Redis connection
    try:
        await redis_client.connect()
        logger.info("Redis connected successfully")
    except Exception as e:
        logger.error(f"Failed to connect to Redis: {e}")
        # Continue without Redis (graceful degradation)
    
    # Create database tables (in production, use Alembic migrations)
    # Note: Multiple workers may try to create ENUM types simultaneously,
    # so we catch and ignore "already exists" errors
    try:
        async with engine.begin() as conn:
            await conn.run_sync(Base.metadata.create_all)
        logger.info("Database tables created")
    except Exception as e:
        # Ignore errors about existing ENUM types (race condition between workers)
        error_str = str(e)
        if "already exists" in error_str.lower() or "duplicate key" in error_str.lower():
            logger.warning(f"Database objects may already exist (this is normal with multiple workers): {e}")
        else:
            logger.error(f"Failed to create database tables: {e}")
            raise
    
    yield
    
    # Shutdown
    logger.info("Shutting down Picrete API...")
    
    # Disconnect Redis
    try:
        await redis_client.disconnect()
        logger.info("Redis disconnected")
    except Exception as e:
        logger.error(f"Error disconnecting Redis: {e}")


app = FastAPI(
    title=settings.PROJECT_NAME,
    version=settings.VERSION,
    description="Picrete - AI-powered chemistry exam grading platform",
    lifespan=lifespan,
    openapi_url=f"{settings.API_V1_STR}/openapi.json",
)

# Configure CORS - MUST be first middleware
app.add_middleware(
    CORSMiddleware,
    allow_origins=settings.BACKEND_CORS_ORIGINS,
    allow_credentials=True,
    allow_methods=["GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"],
    allow_headers=["*"],
    expose_headers=["*"],
    max_age=3600,  # Cache preflight requests for 1 hour
)

# Include API router
app.include_router(api_router, prefix=settings.API_V1_STR)


@app.api_route("/healthz", methods=["GET", "HEAD"])
async def health_check():
    """
    Health check endpoint for Docker and monitoring
    Checks API, Redis, and Database connectivity
    Supports both GET and HEAD methods for compatibility with monitoring tools
    """
    health_status = {
        "service": "picrete-api",
        "status": "healthy",
        "components": {}
    }
    
    # Check Redis
    try:
        if redis_client.redis:
            await redis_client.redis.ping()
            health_status["components"]["redis"] = "healthy"
        else:
            health_status["components"]["redis"] = "disconnected"
    except Exception as e:
        health_status["components"]["redis"] = f"unhealthy: {str(e)}"
        health_status["status"] = "degraded"
    
    # Check Database
    try:
        async with engine.connect() as conn:
            await conn.execute("SELECT 1")
        health_status["components"]["database"] = "healthy"
    except Exception as e:
        health_status["components"]["database"] = f"unhealthy: {str(e)}"
        health_status["status"] = "unhealthy"
    
    return health_status


@app.get("/")
async def root():
    """Root endpoint"""
    return {
        "message": "Picrete API",
        "version": settings.VERSION,
        "docs": f"{settings.API_V1_STR}/docs"
    }


