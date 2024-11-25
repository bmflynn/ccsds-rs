#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum Error {
    #[error("Not enough bytes")]
    NotEnoughData { actual: usize, minimum: usize },
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("Invalid timecode config: {0}")]
    TimecodeConfig(String),

    #[error("Overflow")]
    Overflow,
    #[error("Underflow")]
    Underflow,

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
