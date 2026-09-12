mod catalog;
mod sessions;
#[cfg(test)]
mod tests;
use crate::core::state::AppState;
use axum::{
    extract::DefaultBodyLimit,
    routing::{get, post},
    Router,
};
pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/catalog", get(catalog::list).post(catalog::save_new))
        .route("/catalog/:id", get(catalog::get).put(catalog::save))
        .route("/catalog/:id/publish", post(catalog::publish))
        .route("/catalog/:id/unpublish", post(catalog::unpublish))
        .route("/attempts", post(sessions::start))
        .route("/attempts/:id", get(sessions::get).put(sessions::edit))
        .route("/attempts/:id/actions", post(sessions::action))
        .route(
            "/attempts/:id/photos",
            post(sessions::upload).layer(DefaultBodyLimit::max(12 * 1024 * 1024)),
        )
}
