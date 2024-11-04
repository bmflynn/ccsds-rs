#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LeapsecError {
    #[error("Leap second database is not valid for the provided time")]
    Expired,
    #[error("Leap second out of range for leap second database")]
    OutOfRange,
    #[error("Leap second database parse error: {0}")]
    Parse(String),
}

#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum Error {
    #[error("Not enough bytes")]
    NotEnoughData { actual: usize, minimum: usize },
    #[error(transparent)]
    Timecode(#[from] super::timecode::Error),
    #[error(transparent)]
    Leapsec(#[from] LeapsecError),
}

pub type Result<T> = std::result::Result<T, Error>;
