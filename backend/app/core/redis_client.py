"""Redis client for caching and session management"""
import redis.asyncio as aioredis
from typing import Optional, Any
import json
import logging
from app.core.config import settings

logger = logging.getLogger(__name__)


class RedisClient:
    """Redis client wrapper with caching utilities"""
    
    def __init__(self):
        self.redis: Optional[aioredis.Redis] = None
    
    async def connect(self):
        """Connect to Redis"""
        try:
            self.redis = await aioredis.from_url(
                settings.REDIS_URL,
                encoding="utf-8",
                decode_responses=True,
                max_connections=50,  # Connection pool size
                socket_keepalive=True,
                socket_connect_timeout=5,
                retry_on_timeout=True,
            )
            await self.redis.ping()
            logger.info("Successfully connected to Redis")
        except Exception as e:
            logger.error(f"Failed to connect to Redis: {e}")
            raise
    
    async def disconnect(self):
        """Disconnect from Redis"""
        if self.redis:
            await self.redis.close()
            logger.info("Disconnected from Redis")
    
    async def get(self, key: str) -> Optional[Any]:
        """Get value from cache"""
        try:
            value = await self.redis.get(key)
            if value:
                return json.loads(value)
            return None
        except Exception as e:
            logger.error(f"Redis GET error for key {key}: {e}")
            return None
    
    async def set(self, key: str, value: Any, expire: int = 3600):
        """Set value in cache with expiration (default 1 hour)"""
        try:
            await self.redis.set(
                key,
                json.dumps(value),
                ex=expire
            )
        except Exception as e:
            logger.error(f"Redis SET error for key {key}: {e}")
    
    async def delete(self, key: str):
        """Delete key from cache"""
        try:
            await self.redis.delete(key)
        except Exception as e:
            logger.error(f"Redis DELETE error for key {key}: {e}")
    
    async def exists(self, key: str) -> bool:
        """Check if key exists"""
        try:
            return await self.redis.exists(key) > 0
        except Exception as e:
            logger.error(f"Redis EXISTS error for key {key}: {e}")
            return False
    
    async def incr(self, key: str, expire: Optional[int] = None) -> int:
        """Increment counter"""
        try:
            value = await self.redis.incr(key)
            if expire and value == 1:  # Only set expiration on first increment
                await self.redis.expire(key, expire)
            return value
        except Exception as e:
            logger.error(f"Redis INCR error for key {key}: {e}")
            return 0
    
    async def rate_limit(self, key: str, limit: int, window: int) -> bool:
        """
        Check rate limit using sliding window
        
        Args:
            key: Rate limit key (e.g., "submit:{user_id}")
            limit: Maximum requests allowed
            window: Time window in seconds
            
        Returns:
            True if within limit, False if exceeded
        """
        try:
            count = await self.incr(f"ratelimit:{key}", expire=window)
            return count <= limit
        except Exception as e:
            logger.error(f"Rate limit error for key {key}: {e}")
            return True  # Fail open


# Global Redis client instance
redis_client = RedisClient()

