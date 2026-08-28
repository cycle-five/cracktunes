//! The sleevenote error taxonomy, kept distinct on purpose.
//!
//! Every non-2xx sleevenote response carries the same JSON body --
//! `{ error, id, message }` -- and the `error` string, paired with the HTTP
//! status, names exactly what went wrong:
//!
//! | HTTP | `error`                 | meaning                                          |
//! |------|-------------------------|--------------------------------------------------|
//! | 400  | `invalid_id`            | the id failed the pattern; nothing was fetched   |
//! | 404  | `not_found`             | the entity does not exist (negative-cached)      |
//! | 502  | `extraction_empty`      | navigated fine, zero tracks -- extraction broke  |
//! | 502  | `extraction_incomplete` | recovered fewer items than Spotify declared      |
//! | 504  | `timeout`               | exceeded the whole-call budget                   |
//! | 502  | `internal`              | anything else                                    |
//!
//! **These do not collapse.** Three of them share HTTP 502, and flattening any
//! pair of them into one condition is precisely the failure mode sleevenote
//! exists to prevent. "This id does not exist" is a permanent fact about the
//! world; "our extraction stopped matching Spotify's page" is a bug on our
//! side that a retry will not fix but a deploy will; "it timed out" is
//! transient. A caller that cannot tell them apart will retry the unretryable,
//! give up on the recoverable, and report a scraper regression to the user as
//! a missing song. So this client keeps one [`Error`] variant per code, and
//! deliberately does not offer a single `is_retryable()` shortcut that would
//! quietly re-merge them.

use serde::{Deserialize, Serialize};
use thiserror::Error as ThisError;

/// A convenient alias for results from this crate.
pub type Result<T> = std::result::Result<T, Error>;

/// The `error` string of a sleevenote error body, as a closed set.
///
/// [`ErrorCode::Unrecognized`] catches a code added by a newer service than
/// this client knows about: forward compatibility without ever silently
/// mapping an unknown failure onto a known one.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// HTTP 400. The id failed `^[A-Za-z0-9]{1,64}$`; nothing was fetched.
    InvalidId,
    /// HTTP 404. The entity does not exist. Negative-cached by the service.
    NotFound,
    /// HTTP 502. Navigation succeeded but yielded zero tracks: extraction broke.
    ExtractionEmpty,
    /// HTTP 502. Fewer items recovered than Spotify declared.
    ExtractionIncomplete,
    /// HTTP 504. The service exceeded its own whole-call budget.
    Timeout,
    /// HTTP 502. Anything else.
    Internal,
    /// A code this client version does not know. Preserved verbatim.
    #[serde(untagged)]
    Unrecognized(String),
}

/// The body every non-2xx sleevenote response carries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorBody {
    /// Which failure this was.
    pub error: ErrorCode,
    /// The id the request was for, echoed back.
    pub id: String,
    /// Human-readable explanation, safe to log.
    pub message: String,
}

/// What the service said, alongside the status it said it with.
///
/// The status is carried rather than derived from the code so that a service
/// that ever changes a status is observable instead of silently normalised.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ErrorDetail {
    /// The HTTP status the body arrived with.
    pub status: u16,
    /// The id the request was for, echoed back by the service.
    pub id: String,
    /// Human-readable explanation from the service.
    pub message: String,
}

/// Everything that can go wrong talking to sleevenote.
///
/// The first six variants are the service's documented taxonomy, one apiece.
/// See the [module docs](self) for why they stay separate.
#[derive(Debug, ThisError)]
#[non_exhaustive]
pub enum Error {
    /// HTTP 400 `invalid_id`: the id failed the pattern, so nothing was fetched.
    ///
    /// Also produced locally, without a request, when an id fails the same
    /// pattern client-side.
    #[error("invalid id `{}`: {} (HTTP {})", .0.id, .0.message, .0.status)]
    InvalidId(ErrorDetail),

    /// HTTP 404 `not_found`: the entity does not exist. Permanent.
    #[error("no such entity `{}`: {} (HTTP {})", .0.id, .0.message, .0.status)]
    NotFound(ErrorDetail),

    /// HTTP 502 `extraction_empty`: navigation worked, zero tracks came back.
    ///
    /// This means our extraction stopped matching Spotify's page. It is our
    /// bug, not the caller's, and retrying the same id will keep failing until
    /// the service is fixed.
    #[error("extraction returned nothing for `{}`: {} (HTTP {})", .0.id, .0.message, .0.status)]
    ExtractionEmpty(ErrorDetail),

    /// HTTP 502 `extraction_incomplete`: fewer items recovered than declared.
    ///
    /// Distinct from [`Error::ExtractionEmpty`]: something was recovered, so a
    /// caller willing to accept a partial answer has a different decision to
    /// make than one facing a total extraction failure.
    #[error("extraction incomplete for `{}`: {} (HTTP {})", .0.id, .0.message, .0.status)]
    ExtractionIncomplete(ErrorDetail),

    /// HTTP 504 `timeout`: the service exceeded its whole-call budget.
    ///
    /// Transient. Distinct from a client-side timeout, which arrives as
    /// [`Error::Transport`] with [`reqwest::Error::is_timeout`] set.
    #[error("sleevenote timed out on `{}`: {} (HTTP {})", .0.id, .0.message, .0.status)]
    Timeout(ErrorDetail),

    /// HTTP 502 `internal`: anything else the service classified itself.
    #[error("sleevenote internal error on `{}`: {} (HTTP {})", .0.id, .0.message, .0.status)]
    Internal(ErrorDetail),

    /// The service returned the documented error shape with a code this client
    /// does not know -- a newer sleevenote than this crate was built against.
    #[error("unrecognized sleevenote code `{code}` on `{}`: {} (HTTP {})", .detail.id, .detail.message, .detail.status)]
    Unrecognized {
        /// The `error` string, verbatim.
        code: String,
        /// The rest of the body.
        detail: ErrorDetail,
    },

    /// A non-2xx response whose body was not the documented error shape.
    ///
    /// Usually something in front of the service answering instead of it: a
    /// proxy error page, an auth gateway, a 413 from an ingress.
    #[error("HTTP {status} from sleevenote with an unrecognized body: {body}")]
    UnexpectedStatus {
        /// The HTTP status.
        status: u16,
        /// The raw body, truncated by nothing -- log it, do not parse it.
        body: String,
    },

    /// A 2xx response whose body did not deserialize into the expected type.
    ///
    /// This is wire drift: the service changed a shape this client models.
    #[error("could not decode sleevenote response: {source}")]
    Decode {
        /// The underlying serde failure, including the path that failed.
        source: serde_json::Error,
        /// The raw body, so the drift can be diagnosed from a log line.
        body: String,
    },

    /// The request never produced a response: connection, TLS, or a
    /// client-side timeout. Check [`reqwest::Error::is_timeout`] to tell the
    /// last apart from the rest.
    #[error("sleevenote request failed: {0}")]
    Transport(#[from] reqwest::Error),

    /// The configured base URL is not a usable HTTP base.
    #[error("invalid sleevenote base url `{url}`: {message}")]
    InvalidBaseUrl {
        /// The offending value.
        url: String,
        /// Why it was rejected.
        message: String,
    },
}

impl Error {
    /// Interpret one non-2xx sleevenote response.
    ///
    /// Exposed because it is the whole mapping from wire to taxonomy, and a
    /// caller driving its own HTTP stack should not have to reimplement it.
    /// A body that is not the documented shape becomes
    /// [`Error::UnexpectedStatus`] rather than being coerced.
    #[must_use]
    pub fn from_response(status: u16, body: &str) -> Self {
        let Ok(parsed) = serde_json::from_str::<ErrorBody>(body) else {
            return Error::UnexpectedStatus {
                status,
                body: body.to_owned(),
            };
        };
        let detail = ErrorDetail {
            status,
            id: parsed.id,
            message: parsed.message,
        };
        match parsed.error {
            ErrorCode::InvalidId => Error::InvalidId(detail),
            ErrorCode::NotFound => Error::NotFound(detail),
            ErrorCode::ExtractionEmpty => Error::ExtractionEmpty(detail),
            ErrorCode::ExtractionIncomplete => Error::ExtractionIncomplete(detail),
            ErrorCode::Timeout => Error::Timeout(detail),
            ErrorCode::Internal => Error::Internal(detail),
            ErrorCode::Unrecognized(code) => Error::Unrecognized { code, detail },
        }
    }

    /// The service-reported code, when this error came from the service at all.
    ///
    /// `None` for transport, decode, and configuration failures.
    #[must_use]
    pub fn code(&self) -> Option<ErrorCode> {
        match self {
            Error::InvalidId(_) => Some(ErrorCode::InvalidId),
            Error::NotFound(_) => Some(ErrorCode::NotFound),
            Error::ExtractionEmpty(_) => Some(ErrorCode::ExtractionEmpty),
            Error::ExtractionIncomplete(_) => Some(ErrorCode::ExtractionIncomplete),
            Error::Timeout(_) => Some(ErrorCode::Timeout),
            Error::Internal(_) => Some(ErrorCode::Internal),
            Error::Unrecognized { code, .. } => Some(ErrorCode::Unrecognized(code.clone())),
            _ => None,
        }
    }

    /// The `{ status, id, message }` the service supplied, when it supplied one.
    #[must_use]
    pub fn detail(&self) -> Option<&ErrorDetail> {
        match self {
            Error::InvalidId(detail)
            | Error::NotFound(detail)
            | Error::ExtractionEmpty(detail)
            | Error::ExtractionIncomplete(detail)
            | Error::Timeout(detail)
            | Error::Internal(detail)
            | Error::Unrecognized { detail, .. } => Some(detail),
            _ => None,
        }
    }
}
