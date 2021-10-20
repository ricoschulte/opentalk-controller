use controller::prelude::*;
use validator::ValidationErrors;

/// A legal vote error
#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    /// A vote error caused by invalid requests
    #[error("{0}")]
    Vote(ErrorKind),
    /// A fatal error
    #[error("{0}")]
    Fatal(#[from] anyhow::Error),
}

/// A non critical vote error caused by invalid requests
#[derive(Debug, thiserror::Error, PartialEq)]
pub(crate) enum ErrorKind {
    #[error("A vote is already active")]
    VoteAlreadyActive,
    #[error("No vote is currently taking place")]
    NoVoteActive,
    #[error("The provided vote id is invalid")]
    InvalidVoteId,
    #[error("The requesting user is ineligible")]
    Ineligible,
    #[error("The given allowlist contains guests")]
    AllowlistContainsGuests(Vec<ParticipantId>),
    #[error("The vote results are inconsistent")]
    Inconsistency,
    #[error("Failed to validate request. Invalid fields: {0:?}")]
    BadRequest(Vec<String>),
}

impl From<ErrorKind> for Error {
    fn from(e: ErrorKind) -> Self {
        Self::Vote(e)
    }
}

impl From<ValidationErrors> for Error {
    fn from(errors: ValidationErrors) -> Self {
        let errors = errors
            .errors()
            .iter()
            .map(|(field, ..)| field.to_string())
            .collect();

        Self::Vote(ErrorKind::BadRequest(errors))
    }
}
