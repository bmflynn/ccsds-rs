#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum TimecodeError {
    #[error("Invalid timecode config: {0}")]
    Config(String),
}

#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum Error {
    #[error("Not enough bytes; wanted {wanted}, got {got}")]
    NotEnoughData { got: usize, wanted: usize },

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Timecode(#[from] TimecodeError),

    /// Integrity check or correct error executing the algorithm.
    #[error("integrity algorithm error: {0}")]
    IntegrityAlgorithm(String),
}

#[cfg(feature = "python")]
use pyo3::{exceptions::PyValueError, PyErr};

#[cfg(feature = "python")]
impl From<Error> for PyErr {
    fn from(value: Error) -> Self {
        PyValueError::new_err(value.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
