#![allow(clippy::used_underscore_binding)]

use crate::protocol::parts::{ExecutionResult, ServerError};
// use std::backtrace::Backtrace;
use thiserror::Error;

/// A list specifying categories of [`HdbError`](struct.HdbError.html).
///
/// This list may grow over time and it is not recommended to exhaustively
/// match against it.
#[derive(Error, Debug)] //Copy, Clone, Eq, PartialEq,
pub enum HdbError {
    /// Error occured in deserialization.
    #[error("Error occured in deserialization.")]
    Deserialization {
        /// The causing Error.
        #[from]
        source: serde_db::de::DeserializationError,
        // backtrace: Backtrace,
    },

    /// Error occured in serialization.
    #[error("Error occured in serialization.")]
    Serialization {
        /// The causing Error.
        #[from]
        source: serde_db::ser::SerializationError,
        // backtrace: Backtrace,
    },

    /// Some error occured while decoding CESU-8.
    #[error("Some error occured while decoding CESU-8.")]
    Cesu8 {
        /// The causing Error.
        #[from]
        source: cesu8::Cesu8DecodingError,
        // backtrace: Backtrace,
    },

    /// Erroneous Connection Parameters.
    #[error("Erroneous Connection Parameters")]
    ConnParams {
        /// The causing Error.
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
        // backtrace: Backtrace,
    },

    /// Database server responded with an error.
    #[error("Database server responded with an error.")]
    DbError {
        /// The causing Error.
        #[from]
        source: ServerError,
        // backtrace: Backtrace,
    },

    /// Error occured while evaluating a HdbResponse or an HdbReturnValue.
    #[error("Error occured while evaluating a HdbResponse or an HdbReturnValue.")]
    Evaluation(&'static str),

    /// Database server responded with at least one error.
    #[error("Database server responded with at least one error.")]
    ExecutionResults(Vec<ExecutionResult>),

    /// Error occured while streaming a LOB.
    #[error("Error occured while streaming a LOB.")]
    LobStreaming(std::io::Error),

    /// Implementation error.
    #[error("Implementation error: {}", _0)]
    Impl(&'static str),

    /// Implementation error.
    #[error("Implementation error: {}", _0)]
    ImplDetailed(String),

    /// Error occured in thread synchronization.
    #[error("Error occured in thread synchronization.")]
    Poison,

    /// An error occurred on the server that requires the session to be terminated.
    #[error("An error occurred on the server that requires the session to be terminated.")]
    SessionClosingTransactionError,

    /// Error occured in communication with the database.
    #[error("Error occured in communication with the database")]
    Tcp {
        /// The causing Error.
        #[from]
        source: std::io::Error,
        // backtrace: Backtrace,
    },

    /// Error caused by wrong usage.
    #[error("Wrong usage: {}", _0)]
    Usage(&'static str),

    /// Error caused by wrong usage.
    #[error("Wrong usage: {}", _0)]
    UsageDetailed(String),
}

/// Abbreviation of `Result<T, HdbError>`.
pub type HdbResult<T> = std::result::Result<T, HdbError>;

impl HdbError {
    /// Returns the contained `ServerError`, if any.
    ///
    /// This method helps in case you need programmatic access to e.g. the error code.
    ///
    /// Example:
    ///
    /// ```rust,no_run
    /// # use hdbconnect::{Connection, HdbError, HdbResult};
    /// # use hdbconnect::IntoConnectParams;
    /// # fn main() -> HdbResult<()> {
    ///     # let hdb_result: HdbResult<()> = Err(HdbError::Usage("test"));
    ///     # let mut connection = Connection::new("".into_connect_params()?)?;
    ///     if let Err(hdberror) = hdb_result {
    ///         if let Some(server_error) = hdberror.server_error() {
    ///             let sys_m_error_code: (i32, String, String) = connection
    ///                 .query(&format!(
    ///                     "select * from SYS.M_ERROR_CODES where code = {}",
    ///                     server_error.code()
    ///                 ))?.try_into()?;
    ///             println!("sys_m_error_code: {:?}", sys_m_error_code);
    ///         }
    ///     }
    ///     # Ok(())
    /// # }
    /// ```
    pub fn server_error(&self) -> Option<&ServerError> {
        match self {
            Self::DbError {
                source: server_error,
            } => Some(&server_error),
            _ => None,
        }
    }

    pub(crate) fn conn_params(error: Box<dyn std::error::Error + Send + Sync + 'static>) -> Self {
        Self::ConnParams { source: error }
    }
}

impl<G> From<std::sync::PoisonError<G>> for HdbError {
    fn from(_error: std::sync::PoisonError<G>) -> Self {
        Self::Poison
    }
}
