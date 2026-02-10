mod handlers;

use axum::{routing::get, Router};

use crate::core::state::AppState;

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/sources", get(handlers::list_sources))
        .route("/items", get(handlers::list_items))
        .route("/items/:item_id/images/:image_id/view", get(handlers::view_item_image))
}
