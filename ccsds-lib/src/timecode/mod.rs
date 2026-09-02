use hifitime::Epoch;
use spacecrafts::TimecodeConfig;

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

// Create a boxed [TimecodeDecoder] from a spacecraft timecode configuration.
pub fn decoder_with_config(tc: &TimecodeConfig) -> Result<Box<dyn TimecodeDecoder>> {
    match tc {
        TimecodeConfig::CDS {
            epoch,
            day_length,
            submillis_length,
            self_identifying,
        } => {
            let epoch = Epoch::from_format_str(&epoch, "%Y-%m-%dT%H:%M:%SZ")
                .map_err(|_| Error::Config("invalid timestamp".to_string()))?;
            Ok(if self_identifying.unwrap_or_default() {
                Box::new(cds::PFieldDecoder::default().with_epoch(epoch.to_unix_seconds()))
            } else {
                Box::new(
                    cds::Decoder::new(
                        day_length.unwrap_or_default(),
                        submillis_length.unwrap_or_default(),
                    )
                    .with_epoch(epoch.to_unix_seconds()),
                )
            })
        }
        TimecodeConfig::CUC {
            epoch,
            basic_length,
            fine_length,
            fine_nanos,
            self_identifying,
        } => {
            let epoch = Epoch::from_format_str(&epoch, "%Y-%m-%dT%H:%M:%SZ")
                .map_err(|_| Error::Config("invalid timestamp".to_string()))?;
            Ok(if self_identifying.unwrap_or_default() {
                Box::new(cuc::PFieldDecoder::default().with_epoch(epoch.to_unix_seconds()))
            } else {
                let len = basic_length.unwrap_or_default();
                if len == 0 {
                    return Err(Error::Config(format!(
                        "CUC timecode basic length must be >= 1"
                    )));
                }
                let mut decoder = cuc::Decoder::new(len).with_epoch(epoch.to_unix_seconds());
                if let Some(f) = fine_length {
                    decoder = decoder.with_fine_time(*f, fine_nanos.unwrap_or(1))
                }
                Box::new(decoder)
            })
        }
    }
}
