use std::{collections::HashMap, fs::File, path::Path};

use crate::{
    error::{Error, Result},
    framing::Vcid,
    spacepacket::Apid,
    timecode::{cds, cuc, TimecodeDecoder},
};
use hifitime::Epoch;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RS {
    pub interleave: usize,
    pub virtual_fill_length: Option<usize>,
    pub num_correctable: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PNConfig {}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FramingConfig {
    pub length: usize,
    pub fhec_present: Option<bool>,
    pub ocf_present: Option<bool>,
    pub fec_present: Option<bool>,
    pub izone_length: Option<usize>,
    pub pseudo_noise: Option<PNConfig>,
    #[serde(rename = "type")]
    pub frame_type: String,
    pub reed_solomon: Option<RS>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelFramingConfig {
    pub fhec_present: Option<bool>,
    pub ocf_present: Option<bool>,
    pub fec_present: Option<bool>,
    pub izone_length: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameChannel {
    pub vcid: Vcid,
    pub framing: Option<ChannelFramingConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PacketChannel {
    pub apid: Apid,
    pub vcid: Vcid,
    pub timecode: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", tag = "format")]
pub enum TimecodeConfig {
    CDS {
        epoch: String,
        #[serde(rename = "dayLength")]
        day_length: Option<usize>,
        #[serde(rename = "submillisLength")]
        submillis_length: Option<usize>,
        #[serde(rename = "selfIdentifying")]
        self_identifying: Option<bool>,
    },
    CUC {
        epoch: String,
        #[serde(rename = "basicLength")]
        basic_length: Option<usize>,
        #[serde(rename = "fineLength")]
        fine_length: Option<usize>,
        #[serde(rename = "fineNanos")]
        fine_nanos: Option<u32>,
        #[serde(rename = "selfIdentifying")]
        self_identifying: Option<bool>,
    },
}

impl TimecodeConfig {
    // Create a boxed [TimecodeDecoder] from this config.
    pub fn decoder(self) -> Result<Box<dyn TimecodeDecoder>> {
        match self {
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
                        decoder = decoder.with_fine_time(f, fine_nanos.unwrap_or(1))
                    }
                    Box::new(decoder)
                })
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub scid: u16,
    pub name: String,
    pub aliaes: Option<Vec<String>>,
    pub framing: FramingConfig,
    pub vcids: Vec<FrameChannel>,
    pub apids: Vec<PacketChannel>,
    pub timecodes: HashMap<String, TimecodeConfig>,
}

impl Config {
    pub fn read<P: AsRef<Path>>(path: P) -> Result<Config> {
        let reader = File::open(&path)?;
        let cfg: Config = match serde_json::from_reader(reader) {
            Ok(c) => c,
            Err(e) => {
                return Err(Error::Config(format!("failed to decode config: {e}",)));
            }
        };
        for (key, tc) in cfg.timecodes.iter() {
            match tc {
                TimecodeConfig::CDS {
                    day_length,
                    self_identifying,
                    ..
                } => {
                    if !self_identifying.unwrap_or(false) {
                        if day_length.is_none() {
                            return Err(Error::Config(format!(
                                "timecode config {key} requires at least day length and epoch"
                            )));
                        }
                    }
                }
                TimecodeConfig::CUC {
                    basic_length,
                    self_identifying,
                    ..
                } => {
                    if !self_identifying.unwrap_or(false) {
                        if basic_length.is_none() {
                            return Err(Error::Config(format!(
                                "timecode config {key} requires at least basic length and epoch"
                            )));
                        }
                    }
                }
            }
        }
        for (i, apid) in cfg.apids.iter().enumerate() {
            let Some(tcname) = &apid.timecode else {
                continue;
            };
            if cfg.timecodes.get(tcname).is_none() {
                return Err(Error::Config(format!(
                    "apid index {i} references unconfigured timecode {tcname}"
                )));
            };
        }
        for ch in cfg.vcids.iter() {
            if ch.framing.is_some() {
                return Err(Error::Config(format!(
                    "per-channel framing config (vcid={}) not currenty supported",
                    ch.vcid
                )));
            };
        }
        if cfg.framing.frame_type != "ccsds_aos".to_string() {}
        Ok(cfg)
    }
}
