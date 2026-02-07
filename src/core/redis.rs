use std::sync::Arc;

use redis::aio::ConnectionManager;
use redis::{cmd, Client, RedisError};
use tokio::sync::RwLock;

#[derive(Clone)]
pub(crate) struct RedisHandle {
    url: String,
    manager: Arc<RwLock<Option<ConnectionManager>>>,
}

#[derive(Debug, Clone)]
pub(crate) enum RedisHealth {
    Healthy,
    Disconnected,
    Unhealthy(String),
}

impl RedisHandle {
    pub(crate) fn new(url: String) -> Self {
        Self { url, manager: Arc::new(RwLock::new(None)) }
    }

    pub(crate) async fn connect(&self) -> Result<(), RedisError> {
        let client = Client::open(self.url.clone())?;
        let manager = ConnectionManager::new(client).await?;
        let mut guard = self.manager.write().await;
        *guard = Some(manager);
        Ok(())
    }

    pub(crate) async fn disconnect(&self) {
        let mut guard = self.manager.write().await;
        *guard = None;
    }

    pub(crate) async fn health(&self) -> RedisHealth {
        let manager = { self.manager.read().await.clone() };
        let Some(mut manager) = manager else {
            return RedisHealth::Disconnected;
        };

        match cmd("PING").query_async::<_, String>(&mut manager).await {
            Ok(_) => RedisHealth::Healthy,
            Err(err) => RedisHealth::Unhealthy(err.to_string()),
        }
    }

    pub(crate) async fn rate_limit(
        &self,
        key: &str,
        limit: u64,
        window_seconds: u64,
    ) -> Result<bool, RedisError> {
        let manager = { self.manager.read().await.clone() };
        let Some(mut manager) = manager else {
            return Ok(true);
        };

        let script = redis::Script::new(
            r#"
            local current = redis.call("INCR", KEYS[1])
            if current == 1 then
                redis.call("EXPIRE", KEYS[1], ARGV[1])
            end
            return current
        "#,
        );

        let current: i64 =
            script.key(key).arg(window_seconds as i64).invoke_async(&mut manager).await?;

        Ok(current <= limit as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::RedisHandle;
    use crate::core::config::Settings;
    use crate::test_support;
    use uuid::Uuid;

    #[tokio::test]
    async fn rate_limit_enforces_limit() {
        let _guard = test_support::env_lock();
        test_support::set_test_env();

        let settings = Settings::load().expect("settings");
        test_support::reset_redis(settings.redis().redis_url()).await.expect("redis reset");

        let redis = RedisHandle::new(settings.redis().redis_url());
        redis.connect().await.expect("redis connect");

        let key = format!("rate-limit:{}", Uuid::new_v4());
        let first = redis.rate_limit(&key, 1, 5).await.expect("rate limit");
        let second = redis.rate_limit(&key, 1, 5).await.expect("rate limit");

        assert!(first);
        assert!(!second);
    }
}
