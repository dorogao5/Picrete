use std::sync::Arc;

use sqlx::PgPool;

use crate::core::{config::Settings, redis::RedisHandle};
use crate::services::storage::StorageService;

#[derive(Clone)]
pub(crate) struct AppState {
    inner: Arc<InnerState>,
}

struct InnerState {
    settings: Settings,
    db: PgPool,
    redis: RedisHandle,
    storage: Option<StorageService>,
}

impl AppState {
    pub(crate) fn new(
        settings: Settings,
        db: PgPool,
        redis: RedisHandle,
        storage: Option<StorageService>,
    ) -> Self {
        Self { inner: Arc::new(InnerState { settings, db, redis, storage }) }
    }

    pub(crate) fn settings(&self) -> &Settings {
        &self.inner.settings
    }

    pub(crate) fn db(&self) -> &PgPool {
        &self.inner.db
    }

    pub(crate) fn redis(&self) -> &RedisHandle {
        &self.inner.redis
    }

    pub(crate) fn storage(&self) -> Option<&StorageService> {
        self.inner.storage.as_ref()
    }
}
