use std::{fs::File, path::Path};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::frame::FrameType;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RS {
    pub interleave: usize,
    pub virtualfill: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub asm: Option<Vec<u8>>,
    pub scid: u16,
    #[serde(rename = "type")]
    pub frame_type: FrameType,
    pub length: usize,
    pub pn: bool,
    pub rs: Option<RS>,
}

impl Config {
    pub fn read<P: AsRef<Path>>(path: P) -> Result<Config> {
        let reader = File::open(&path)?;
        Ok(serde_json::from_reader(reader)
            .context(format!("reading config from {:?}", path.as_ref()))?)
    }
}
