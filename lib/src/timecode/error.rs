#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Other(#[from] crate::Error),

    #[error("Unsupported configuration for timecode: {0}")]
    Invalid(String),

    #[error("Leap second database is not valid for the provided time")]
    LeapsecsExpired,
    #[error("Leap second out of range for leap second database")]
    LeapsecOutOfRange,
    #[error("Leap second database parse error: {0}")]
    LeapsecParse(String),
}

pub type Result<T> = std::result::Result<T, Error>;
