use axum::response::{IntoResponse, Response};
use http::StatusCode;
use tracing::error;

pub struct Report(eyre::Report);

pub type HttpResult<T, E = Report> = eyre::Result<T, E>;

impl std::fmt::Debug for Report {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl<E> From<E> for Report
where
    E: Into<eyre::Report>,
{
    fn from(err: E) -> Self {
        Self(err.into())
    }
}

impl IntoResponse for Report {
    fn into_response(self) -> Response {
        let err = self.0;

        error!(?err, "http handler error");
        (StatusCode::INTERNAL_SERVER_ERROR, format!("{err:?}")).into_response()
    }
}
