//! Thin reqwest helpers: a single place that maps HTTP statuses to [`Error`].

use reqwest::StatusCode;

use crate::error::{Error, Result};

/// Map a non-success status to the corresponding error.
pub(crate) fn status_error(status: StatusCode) -> Error {
    match status {
        StatusCode::UNAUTHORIZED => Error::Unauthorized,
        StatusCode::CONFLICT => Error::Conflict,
        StatusCode::PRECONDITION_FAILED => Error::WrongGeneration,
        StatusCode::NOT_FOUND => Error::NotFound,
        other => Error::Http(format!("request failed: {other}")),
    }
}

/// Return `Ok(resp)` for 2xx, else the mapped error.
pub(crate) fn check(resp: reqwest::Response) -> Result<reqwest::Response> {
    let status = resp.status();
    if status.is_success() {
        Ok(resp)
    } else {
        Err(status_error(status))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_mapping() {
        assert!(matches!(
            status_error(StatusCode::UNAUTHORIZED),
            Error::Unauthorized
        ));
        assert!(matches!(
            status_error(StatusCode::CONFLICT),
            Error::Conflict
        ));
        assert!(matches!(
            status_error(StatusCode::PRECONDITION_FAILED),
            Error::WrongGeneration
        ));
        assert!(matches!(
            status_error(StatusCode::NOT_FOUND),
            Error::NotFound
        ));
        assert!(matches!(
            status_error(StatusCode::INTERNAL_SERVER_ERROR),
            Error::Http(_)
        ));
    }
}
