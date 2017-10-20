use serde;
use std::convert::From;
use std::error::Error;
use std::fmt;

// Error that can occur while serializing a standard rust type or struct into a SQL parameter
pub enum SerializationError {
    GeneralError(String),
    InvalidValue(String),
    StructuralMismatch(&'static str),
    TypeMismatch(&'static str, u8),
    RangeErr(&'static str, u8),
}

impl Error for SerializationError {
    fn description(&self) -> &str {
        match *self {
            SerializationError::GeneralError(_) => "error from framework",
            SerializationError::InvalidValue(_) => "incorrect value",
            SerializationError::StructuralMismatch(_) => "structural mismatch",
            SerializationError::TypeMismatch(_, _) => "type mismatch",
            SerializationError::RangeErr(_, _) => "range exceeded",
        }
    }
}

impl From<&'static str> for SerializationError {
    fn from(error: &'static str) -> SerializationError {
        SerializationError::StructuralMismatch(error)
    }
}

impl serde::ser::Error for SerializationError {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        SerializationError::GeneralError(msg.to_string())
    }

    // fn invalid_value(msg: &str) -> Self {
    //     SerializationError::InvalidValue(msg.into())
    // }
}

impl fmt::Debug for SerializationError {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            SerializationError::GeneralError(ref s) => write!(fmt, "{}: {}", self.description(), s),
            SerializationError::InvalidValue(ref s) => write!(fmt, "{}: {}", self.description(), s),
            SerializationError::StructuralMismatch(s) => {
                write!(fmt, "{}: {}", self.description(), s)
            }
            SerializationError::TypeMismatch(s, tc) => {
                write!(
                    fmt,
                    "{}: given value of type \"{}\" cannot be converted into value of type code {}",
                    self.description(),
                    s,
                    tc
                )
            }
            SerializationError::RangeErr(s, tc) => {
                write!(
                    fmt,
                    "{}: given value of type \"{}\" does not fit into supported range of SQL type \
                     (type code {})",
                    self.description(),
                    s,
                    tc
                )
            }
        }
    }
}
impl fmt::Display for SerializationError {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self, fmt)
    }
}
pub type SerializationResult<T> = Result<T, SerializationError>;
