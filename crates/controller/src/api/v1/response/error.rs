// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

//! Error response types for REST APIv1
use super::{
    CODE_INVALID_EMAIL, CODE_INVALID_LENGTH, CODE_INVALID_URL, CODE_INVALID_VALUE,
    CODE_MISSING_VALUE, CODE_OUT_OF_RANGE, CODE_VALUE_REQUIRED,
};
use actix_http::header::HeaderValue;
use actix_http::header::TryIntoHeaderValue;
use actix_web::error::JsonPayloadError;
use actix_web::http::{header, StatusCode};
use actix_web::HttpRequest;
use actix_web::{body::BoxBody, HttpResponse, ResponseError};
use actix_web_httpauth::headers::www_authenticate::bearer::{Bearer, Error};
use database::DatabaseError;
use diesel::result::DatabaseErrorKind;
use itertools::Itertools;
use serde::Serialize;
use std::borrow::Cow;
use std::fmt;
use validator::ValidationErrors;

/// Error handler for the actix JSON extractor
///
/// Gets called when a incoming request results in an [`JsonPayloadError`].
/// Returns a `Bad Request` [`ApiError`] error with an appropriate error code and message.
pub fn json_error_handler(err: JsonPayloadError, _: &HttpRequest) -> actix_web::error::Error {
    let error_code = match err {
        JsonPayloadError::OverflowKnownLength { .. } | JsonPayloadError::Overflow { .. } => {
            "payload_overflow"
        }
        JsonPayloadError::ContentType => "invalid_content_type",
        JsonPayloadError::Deserialize(_) | JsonPayloadError::Serialize(_) => "invalid_json",
        _ => "invalid_payload",
    };
    ApiError::bad_request()
        .with_code(error_code)
        .with_message(err.to_string())
        .into()
}

#[derive(Debug, Serialize)]
struct StandardErrorBody {
    // Machine readable error code
    code: Cow<'static, str>,
    // Human readable message
    message: Cow<'static, str>,
}

#[derive(Debug, Serialize)]
pub struct ValidationErrorEntry {
    /// The field related to the error
    /// It's a struct level error when no field is set
    #[serde(skip_serializing_if = "Option::is_none")]
    field: Option<Cow<'static, str>>,
    /// Machine readable error message
    code: Cow<'static, str>,
    /// Human readable error message
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<Cow<'static, str>>,
}

impl ValidationErrorEntry {
    pub fn new<F, C, M>(field: F, code: C, message: Option<M>) -> Self
    where
        F: Into<Cow<'static, str>>,
        C: Into<Cow<'static, str>>,
        M: Into<Cow<'static, str>>,
    {
        Self {
            field: Some(field.into()),
            code: code.into(),
            message: message.map(|m| m.into()),
        }
    }

    #[allow(dead_code)]
    pub fn new_struct_level<C, M>(code: C, message: Option<M>) -> Self
    where
        C: Into<Cow<'static, str>>,
        M: Into<Cow<'static, str>>,
    {
        Self {
            field: None,
            code: code.into(),
            message: message.map(|m| m.into()),
        }
    }
}

#[derive(Debug, Serialize)]
struct ValidationErrorBody {
    /// Machine readable error message
    code: Cow<'static, str>,
    // Human readable message
    message: Cow<'static, str>,
    // A list validation errors
    errors: Vec<ValidationErrorEntry>,
}

impl ValidationErrorBody {
    pub fn new<C, M>(code: C, message: M, errors: Vec<ValidationErrorEntry>) -> Self
    where
        C: Into<Cow<'static, str>>,
        M: Into<Cow<'static, str>>,
    {
        Self {
            code: code.into(),
            message: message.into(),
            errors,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum ErrorBody {
    /// The standard error body
    Standard(StandardErrorBody),
    /// Special error body for validation errors
    Validation(ValidationErrorBody),
}

impl ErrorBody {
    /// Get the content type for the corresponding body
    fn content_type(&self) -> HeaderValue {
        match self {
            Self::Standard(..) => HeaderValue::from_static("text/json; charset=utf-8"),
            Self::Validation(..) => header::HeaderValue::from_static("text/json; charset=utf-8"),
        }
    }
}

/// Error variants for the WWW Authenticate header
#[derive(Debug)]
pub enum AuthenticationError {
    InvalidIdToken,
    InvalidAccessToken,
    AccessTokenInactive,
    SessionExpired,
}

impl AuthenticationError {
    fn error(&self) -> Error {
        match self {
            Self::InvalidIdToken | Self::InvalidAccessToken | Self::AccessTokenInactive => {
                Error::InvalidToken
            }
            Self::SessionExpired => Error::InvalidRequest,
        }
    }

    fn message(&self) -> &'static str {
        match self {
            Self::InvalidIdToken => "The provided id token is invalid",
            Self::InvalidAccessToken => "The provided access token is invalid",
            Self::AccessTokenInactive => "The provided access token expired",
            Self::SessionExpired => "The user session expired",
        }
    }
}

/// The default REST API error
///
/// Can be build via the associated functions to represent various HTTP errors. Each
/// HTTP error has their default error code and message that get send in a JSON body.
/// The error code and message can be overwritten when creating an error.

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    www_authenticate: Option<HeaderValue>,
    body: ErrorBody,
}

impl ApiError {
    fn new_standard<T>(status: StatusCode, code: T, message: T) -> Self
    where
        T: Into<Cow<'static, str>>,
    {
        Self {
            status,
            www_authenticate: None,
            body: ErrorBody::Standard(StandardErrorBody {
                code: code.into(),
                message: message.into(),
            }),
        }
    }

    /// Override the default code for an error
    pub fn with_code<T>(mut self, code: T) -> Self
    where
        T: Into<Cow<'static, str>>,
    {
        match &mut self.body {
            ErrorBody::Standard(std) => std.code = code.into(),
            ErrorBody::Validation(val) => val.code = code.into(),
        }

        self
    }

    /// Override the default message for an error
    pub fn with_message<T>(mut self, message: T) -> Self
    where
        T: Into<Cow<'static, str>>,
    {
        match &mut self.body {
            ErrorBody::Standard(std) => std.message = message.into(),
            ErrorBody::Validation(val) => val.message = message.into(),
        }

        self
    }

    /// Add an WWW Authenticate header to a response
    pub fn with_www_authenticate(mut self, authentication_error: AuthenticationError) -> Self {
        let header_value = Bearer::build()
            .error_description(authentication_error.message())
            .error(authentication_error.error())
            .finish()
            .try_into_value()
            .expect("All error descriptions must be convertible to header value");

        self.www_authenticate = Some(header_value);

        self
    }

    /// Create a new 400 Bad Request error
    pub fn bad_request() -> Self {
        Self::new_standard(
            StatusCode::BAD_REQUEST,
            "bad_request",
            "Invalid request due to malformed syntax",
        )
    }

    /// Create a new 401 Unauthorized error
    pub fn unauthorized() -> Self {
        Self::new_standard(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "Authentication failed",
        )
    }

    /// Create a new 403 Forbidden error
    pub fn forbidden() -> Self {
        Self::new_standard(
            StatusCode::FORBIDDEN,
            "forbidden",
            "Access to the requested resource is forbidden",
        )
    }

    /// Create a new 404 Not Found error
    pub fn not_found() -> Self {
        Self::new_standard(
            StatusCode::NOT_FOUND,
            "not_found",
            "A requested resource could not be found",
        )
    }

    /// Create a new 409 Conflict error
    pub fn conflict() -> Self {
        Self::new_standard(
            StatusCode::CONFLICT,
            "conflict",
            "The request conflicts with the state of the resource",
        )
    }

    /// Create a new 422 Unprocessable Entity error
    ///
    /// see [`Self::unprocessable_entities()`]
    pub fn unprocessable_entity() -> Self {
        Self::unprocessable_entities::<ValidationErrorEntry, _>([])
    }

    /// Create a new 422 Unprocessable Entity error
    ///
    /// This error is normally created from [`ValidationErrors`] from the validator crate.
    /// The JSON body for this error additionally contains a list of errors for each invalid field.
    pub fn unprocessable_entities<T, I>(errors: I) -> Self
    where
        T: Into<ValidationErrorEntry>,
        I: IntoIterator<Item = T>,
    {
        let errors = errors.into_iter().map(|entry| entry.into()).collect();

        let validation_body = ValidationErrorBody::new(
            "validation_failed",
            "Some provided values are invalid",
            errors,
        );

        Self {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            www_authenticate: None,
            body: ErrorBody::Validation(validation_body),
        }
    }

    /// Create a new 500 Internal Server Error
    pub fn internal() -> Self {
        Self::new_standard(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_server_error",
            "An internal server error occurred",
        )
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.body {
            ErrorBody::Standard(StandardErrorBody { code, message }) => {
                write!(
                    f,
                    "status={}, code={}, message={}",
                    self.status, code, message
                )
            }
            ErrorBody::Validation(ValidationErrorBody {
                code,
                message,
                errors,
            }) => {
                write!(
                    f,
                    "status={}, code={}, message={}, errors={}",
                    self.status,
                    code,
                    message,
                    serde_json::to_string(errors)
                        .unwrap_or_else(|_| "unserializable errors".to_string())
                )
            }
        }
    }
}

impl ResponseError for ApiError {
    fn status_code(&self) -> StatusCode {
        self.status
    }

    fn error_response(&self) -> HttpResponse<BoxBody> {
        let mut response = HttpResponse::new(self.status_code());

        response
            .headers_mut()
            .insert(header::CONTENT_TYPE, self.body.content_type());

        if let Some(www_authenticate) = self.www_authenticate.clone() {
            response
                .headers_mut()
                .insert(header::WWW_AUTHENTICATE, www_authenticate);
        }

        let body = serde_json::to_string(&self.body).expect("Unable to serialize API error body");

        response.set_body(BoxBody::new(body))
    }
}

impl From<crate::BlockingError> for ApiError {
    fn from(e: crate::BlockingError) -> Self {
        log::error!("REST API threw internal error from blocking error: {}", e);
        Self::internal()
    }
}

impl From<actix_web::Error> for ApiError {
    fn from(e: actix_web::Error) -> Self {
        log::error!("REST API threw internal error from actix web error: {}", e);
        Self::internal()
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        log::error!("REST API threw internal error from anyhow error: {:?}", e);
        Self::internal()
    }
}

impl From<DatabaseError> for ApiError {
    fn from(db_error: DatabaseError) -> Self {
        match db_error {
            DatabaseError::NotFound => Self::not_found(),
            DatabaseError::DieselError(diesel::result::Error::DatabaseError(
                DatabaseErrorKind::ForeignKeyViolation,
                _,
            )) => Self::conflict(),
            e => {
                log::error!("REST API threw internal error from database error: {}", e);
                Self::internal()
            }
        }
    }
}

impl From<kustos::Error> for ApiError {
    fn from(e: kustos::Error) -> Self {
        log::error!("REST API threw internal error from kustos error: {}", e);
        Self::internal()
    }
}

impl From<ValidationErrors> for ApiError {
    /// Creates a 422 Unprocessable entity response from the [`ValidationErrors`]
    ///
    /// Note:
    ///
    /// Each validation error is mapped to a field. When we encounter a validation error on a nested struct, we
    /// assume the struct was perceived flattened in it's JSON representation and do not distinguish between nested and
    /// non-nested fields. This may lead to ambiguous field mappings when receiving invalid fields for actually
    /// nested fields.
    ///
    /// We currently have no feasible way to identify correct the JSON representation.
    ///
    /// Example for this misleading behavior:
    ///
    /// Assuming the request body has the following structure:
    /// ```json
    /// {
    ///     "name": "foo",
    ///     "age": 30,
    ///     "nested":
    ///     {
    ///         "name": "bar",
    ///         "age": 24
    ///     }
    /// }
    /// ```
    ///
    ///  Assuming one of the `name` fields is invalid, the resulting validation error would look something like this:
    ///
    /// ```json
    /// {
    ///     "code": "validation_failed",
    ///     "message": "Some provided values are invalid",
    ///     "errors":
    ///     [
    ///         {
    ///             "field": "name",
    ///             "code": "invalid_value"
    ///         }
    ///     ]
    /// }
    /// ```
    ///
    /// The sender has no way to identify which of the `name` fields is invalid, except manually reviewing the values and
    /// reading the API docs.
    fn from(validation_errors: ValidationErrors) -> Self {
        let mut entries = Vec::with_capacity(validation_errors.errors().len());

        collect_validation_errors(validation_errors, &mut entries);

        Self::unprocessable_entities(entries)
    }
}

/// Convert [`ValidationErrors`] into multiple [`ValidationErrorEntries`](ValidationErrorEntry) and collect them in `entries`
fn collect_validation_errors(
    validation_errors: ValidationErrors,
    entries: &mut Vec<ValidationErrorEntry>,
) {
    let errors = validation_errors.into_errors();

    for (field, error_kind) in errors {
        let field = match field {
            "__all__" => None,
            field => Some(field.into()),
        };

        match error_kind {
            validator::ValidationErrorsKind::Field(v) => {
                for error in v {
                    let code = convert_validation_code(&error.code);

                    entries.push(ValidationErrorEntry {
                        field: field.clone(),
                        code: Cow::Borrowed(code),
                        message: error.message,
                    });
                }
            }
            validator::ValidationErrorsKind::Struct(inner_errors) => {
                // Assume all fields were flattened when we encounter a struct level validation error
                collect_validation_errors(*inner_errors.to_owned(), entries);
            }
            validator::ValidationErrorsKind::List(list) => {
                let invalid_indexes = list.iter().map(|(idx, ..)| idx).take(15).join(", ");

                let message = format!("Invalid values at index {invalid_indexes}");

                entries.push(ValidationErrorEntry {
                    field,
                    code: "invalid_values".into(),
                    message: Some(Cow::Owned(message)),
                })
            }
        };
    }
}

fn convert_validation_code(code: &str) -> &'static str {
    match code {
        "email" => CODE_INVALID_EMAIL,
        "url" => CODE_INVALID_URL,
        "length" => CODE_INVALID_LENGTH,
        "range" => CODE_OUT_OF_RANGE,
        "required" => CODE_VALUE_REQUIRED,
        "empty" => CODE_MISSING_VALUE,
        _ => CODE_INVALID_VALUE,
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use test_util::*;
    use validator::Validate;

    #[derive(Debug, Validate)]
    struct ValidationTester {
        #[validate(email)]
        mail: String,
        #[validate(url(message = "This would be a message"))]
        url: String,
        #[validate(length(max = 5))]
        length: String,
        #[validate(range(min = 5, max = 10))]
        range: usize,
        #[validate(required)]
        required: Option<bool>,
        #[validate]
        inner_struct: InnerValidationTester,
    }

    #[derive(Debug, Validate)]
    struct InnerValidationTester {
        #[validate(range(max = 2))]
        another_range: usize,
    }

    #[test]
    fn api_validation_error() {
        let tester = ValidationTester {
            mail: "not_a_mail".into(),
            url: "not_a_url".into(),
            length: "looong".into(),
            range: 11,
            required: None,
            inner_struct: InnerValidationTester { another_range: 3 },
        };

        let mut api_error = match tester.validate() {
            Ok(_) => panic!("Validation should fail"),
            Err(err) => ApiError::from(err),
        };

        match &mut api_error.body {
            ErrorBody::Standard(_) => panic!("Expected validation error body"),
            ErrorBody::Validation(val) => val.errors.sort_by(|a, b| a.field.cmp(&b.field)),
        }

        assert_eq_json!(
            api_error.body,
            {
                "code": "validation_failed",
                "message": "Some provided values are invalid",
                "errors": [
                  {
                    "field": "another_range",
                    "code": "out_of_range"
                  },
                  {
                    "field": "length",
                    "code": "invalid_length"
                  },
                  {
                    "field": "mail",
                    "code": "invalid_email"
                  },
                  {
                    "field": "range",
                    "code": "out_of_range"
                  },
                  {
                    "field": "required",
                    "code": "value_required",
                  },
                  {
                    "field": "url",
                    "code": "invalid_url",
                    "message": "This would be a message"
                  }
                ]
            }
        );
    }

    #[test]
    fn api_error_with_code() {
        let error = ApiError::not_found().with_code("custom_code");

        assert_eq_json!(
            error.body,
            {
                "code": "custom_code",
                "message": "A requested resource could not be found"
            }
        );
    }

    #[test]
    fn api_error_with_message() {
        let error = ApiError::not_found().with_message("A custom message");

        assert_eq_json!(
            error.body,
            {
                "code": "not_found",
                "message": "A custom message"
            }
        );
    }
}
