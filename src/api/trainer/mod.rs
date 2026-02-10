mod handlers;

use axum::{routing::get, routing::post, Router};

use crate::core::state::AppState;

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/sets/generate", post(handlers::generate_set))
        .route("/sets/manual", post(handlers::create_manual_set))
        .route("/sets", get(handlers::list_sets))
        .route("/sets/:set_id", get(handlers::get_set).delete(handlers::delete_set))
}
