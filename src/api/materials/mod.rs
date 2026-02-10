mod handlers;

use axum::{routing::get, Router};

use crate::core::state::AppState;

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/addition-pdf-url", get(handlers::addition_pdf_url))
        .route("/addition-pdf/view", get(handlers::view_addition_pdf))
        .route("/task-bank-image/*relative_path", get(handlers::view_task_bank_image))
}
