pub mod cds;
pub mod cuc;

#[derive(Clone, Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("invalid timecode config: {0}")]
    Config(String),
    #[error("invalid p-field: {0}")]
    InvalidPField(String),
    #[error("{0}")]
    Other(String),
    #[error("not enough data")]
    NotEnoughData,
}

type Result<T> = std::result::Result<T, Error>;

// Decode CCSDS timecodes.
pub trait TimecodeDecoder {
    // Decode bytes into milliseconds since Jan 1, 1970 UTC.
    fn decode_unix_millis(&self, buf: &[u8]) -> Result<f64>;
}
