mod handlers;
mod helpers;
mod queries;

use axum::{routing::get, routing::post, Router};

use crate::core::state::AppState;

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/", post(handlers::create_exam).get(handlers::list_exams))
        .route(
            "/:exam_id",
            get(handlers::get_exam).patch(handlers::update_exam).delete(handlers::delete_exam),
        )
        .route("/:exam_id/publish", post(handlers::publish_exam))
        .route("/:exam_id/task-types", post(handlers::add_task_type))
        .route("/:exam_id/task-types/from-bank", post(handlers::add_task_types_from_bank))
        .route("/:exam_id/submissions", get(handlers::list_exam_submissions))
}

#[cfg(test)]
mod tests;
