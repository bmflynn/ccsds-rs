#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// IO error reading or decoding packets
    #[error(transparent)]
    IO(#[from] std::io::Error),

    #[error("Not enough bytes")]
    NotEnoughData {
        /// Number of bytes we got
        actual: usize,
        /// Minimum number of expected bytes
        minimum: usize,
    },

    /// Error handling or decoding a timecode
    #[error(transparent)]
    Timecode(#[from] crate::timecode::Error),
}
