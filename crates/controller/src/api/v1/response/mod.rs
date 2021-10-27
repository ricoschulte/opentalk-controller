//! Response types for REST APIv1
//!
//! These all implement the [`Responder`] trait.
use actix_web::{HttpResponse, Responder};

mod error;
mod ok;

pub use error::{ApiError, DefaultApiError, StandardErrorKind};
pub use ok::ApiResponse;

/// The default API Result
pub type DefaultApiResult<T> = Result<ApiResponse<T>, DefaultApiError>;

// Represents a 204 No Content HTTP Response
pub struct NoContent;

impl Responder for NoContent {
    fn respond_to(self, _: &actix_web::HttpRequest) -> HttpResponse {
        HttpResponse::NoContent().finish()
    }
}
