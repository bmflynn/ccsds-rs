#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum Error {
    #[error("Not enough bytes")]
    NotEnoughData { actual: usize, minimum: usize },
    #[error(transparent)]
    Io(std::io::Error),

    #[error(transparent)]
    Timecode(#[from] super::timecode::Error),

    /// Integrity check or correct error executing the algorithm.
    #[error("integrity algorithm error: {0}")]
    IntegrityAlgorithm(String),
}

pub type Result<T> = std::result::Result<T, Error>;
