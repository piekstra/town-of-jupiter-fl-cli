//! Error type for the SDK.

use thiserror::Error;

/// Result alias used throughout the SDK.
pub type Result<T> = std::result::Result<T, Error>;

/// Everything that can go wrong talking to the utility portal.
#[derive(Debug, Error)]
pub enum Error {
    /// The HTTP layer failed (DNS, TLS, timeout, connection reset, ...).
    #[error("network error: {0}")]
    Http(#[from] reqwest::Error),

    /// We reached the portal but authentication was rejected.
    #[error("authentication failed: {0}")]
    Auth(String),

    /// An operation requires an authenticated session but none is present.
    #[error("not logged in — run `tojfl auth login` first")]
    NotAuthenticated,

    /// The portal returned a page we could not parse into the expected shape.
    /// The DNN/eCARE markup drifts over time; this points at what broke.
    #[error("could not parse portal response: {0}")]
    Parse(String),

    /// A required ASP.NET/DNN hidden form field was missing from a page.
    #[error("missing form field `{0}` — the portal markup may have changed")]
    MissingFormField(String),

    /// The portal reported a validation/business error back to us.
    #[error("portal rejected the request: {0}")]
    Portal(String),

    /// Local configuration problem (bad file, missing credentials, ...).
    #[error("configuration error: {0}")]
    Config(String),

    /// Credential store (OS keychain) problem.
    #[error("keychain error: {0}")]
    Keychain(String),

    /// Local persistence (session/cache files) problem.
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON (de)serialization problem for persisted state.
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    /// A value the caller supplied was invalid before we ever hit the network.
    #[error("invalid input: {0}")]
    Invalid(String),
}

impl Error {
    pub(crate) fn parse(msg: impl Into<String>) -> Self {
        Error::Parse(msg.into())
    }
    pub(crate) fn invalid(msg: impl Into<String>) -> Self {
        Error::Invalid(msg.into())
    }
}
