// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

//! Response types for REST APIv1
//!
//! These all implement the [`Responder`] trait.
use actix_web::{body::BoxBody, HttpResponse, Responder};

pub mod error;
mod ok;

pub use error::ApiError;
pub use ok::ApiResponse;

/// The default API Result
pub type DefaultApiResult<T> = Result<ApiResponse<T>, ApiError>;

/// Represents a 201 Created HTTP Response
pub struct Created;

impl Responder for Created {
    type Body = BoxBody;

    fn respond_to(self, _: &actix_web::HttpRequest) -> HttpResponse {
        HttpResponse::Created().finish()
    }
}

/// Represents a 204 No Content HTTP Response
pub struct NoContent;

impl Responder for NoContent {
    type Body = BoxBody;

    fn respond_to(self, _: &actix_web::HttpRequest) -> HttpResponse {
        HttpResponse::NoContent().finish()
    }
}

/// Represents a 304 Not Modified HTTP Response
pub struct NotModified;

impl Responder for NotModified {
    type Body = BoxBody;

    fn respond_to(self, _: &actix_web::HttpRequest) -> HttpResponse {
        HttpResponse::NotModified().finish()
    }
}

pub const CODE_INVALID_URL: &str = "invalid_url";
pub const CODE_INVALID_EMAIL: &str = "invalid_email";
pub const CODE_INVALID_LENGTH: &str = "invalid_length";
pub const CODE_OUT_OF_RANGE: &str = "out_of_range";
pub const CODE_VALUE_REQUIRED: &str = "value_required";
pub const CODE_IGNORED_VALUE: &str = "ignored_value";
pub const CODE_MISSING_VALUE: &str = "missing_values";
pub const CODE_INVALID_VALUE: &str = "invalid_value";
