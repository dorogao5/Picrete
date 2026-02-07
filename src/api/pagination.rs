use serde::Serialize;

pub(crate) const fn default_limit() -> i64 {
    100
}

#[derive(Debug, Serialize)]
pub(crate) struct PaginatedResponse<T> {
    pub(crate) items: Vec<T>,
    pub(crate) total_count: i64,
    pub(crate) skip: i64,
    pub(crate) limit: i64,
}
