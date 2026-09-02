use crate::{
    error::{Error, Result},
    timecode::{cds, cuc, TimecodeDecoder},
};
use hifitime::Epoch;
use spacecrafts::TimecodeConfig;

// Create a boxed [TimecodeDecoder] from this config.
pub fn new_timecode_decoder(tc: &TimecodeConfig) -> Result<Box<dyn TimecodeDecoder>> {
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
