use std::collections::HashMap;

use serde::Serialize;

pub(crate) mod auth;
pub(crate) mod exam;
pub(crate) mod submission;
pub(crate) mod user;

#[derive(Debug, Serialize)]
pub(crate) struct HealthResponse {
    pub(crate) service: String,
    pub(crate) status: String,
    pub(crate) components: HashMap<String, String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct RootResponse {
    pub(crate) message: String,
    pub(crate) version: String,
    pub(crate) docs_url: String,
}
