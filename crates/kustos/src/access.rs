use std::{fmt::Display, str::FromStr};

/// Permission access variants
///
/// Get, Put, Post, Delete are the respective HTTP methods.
/// The request middlewares are limited to these methods.
/// Read and Write can be used for more granular access when used with direct enforce calls.
#[derive(Clone, Debug, Copy, PartialEq, Eq, Hash)]
pub enum AccessMethod {
    Read,
    Write,
    Get,
    Post,
    Put,
    Delete,
}

impl AccessMethod {
    pub const GET: AccessMethod = AccessMethod::Get;
    pub const POST: AccessMethod = AccessMethod::Post;
    pub const PUT: AccessMethod = AccessMethod::Put;
    pub const DELETE: AccessMethod = AccessMethod::Delete;

    pub fn all_http() -> [AccessMethod; 4] {
        [Self::GET, Self::POST, Self::PUT, Self::DELETE]
    }
}

impl FromStr for AccessMethod {
    type Err = crate::ParsingError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "read" => AccessMethod::Read,
            "write" => AccessMethod::Write,
            "GET" => AccessMethod::Get,
            "POST" => AccessMethod::Post,
            "PUT" => AccessMethod::Put,
            "DELETE" => AccessMethod::Delete,
            _ => return Err(crate::ParsingError::InvalidAccessMethod(s.to_owned())),
        })
    }
}

impl AsRef<str> for AccessMethod {
    fn as_ref(&self) -> &str {
        match self {
            AccessMethod::Read => "read",
            AccessMethod::Write => "write",
            AccessMethod::Get => "GET",
            AccessMethod::Post => "POST",
            AccessMethod::Put => "PUT",
            AccessMethod::Delete => "DELETE",
        }
    }
}

impl Display for AccessMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_ref())
    }
}

// TODO(r.floren) does this trait impl make any sense now?
impl From<AccessMethod> for [AccessMethod; 1] {
    fn from(val: AccessMethod) -> Self {
        [val]
    }
}

impl From<AccessMethod> for Vec<AccessMethod> {
    fn from(method: AccessMethod) -> Self {
        vec![method]
    }
}

impl From<http::Method> for AccessMethod {
    fn from(method: http::Method) -> Self {
        match method {
            http::Method::GET => Self::GET,
            http::Method::POST => Self::POST,
            http::Method::PUT => Self::PUT,
            http::Method::DELETE => Self::DELETE,
            _ => unimplemented!(),
        }
    }
}
