use std::{
    fs::File,
    io::{stdout, BufReader, Write},
    path::Path,
};

use anyhow::{Context, Result};
use serde::Serialize;
use spacecrafts::Spacecraft;
use tracing::debug;

use ccsds::framing::Frame;

use crate::{
    frame::{Info, Summary},
    packet::IterChunks,
};

pub fn info<I>(input: I, sc: &Spacecraft) -> Result<()>
where
    I: AsRef<Path>,
{
    let reader = File::open(input)?;
    let chunks = IterChunks {
        file: BufReader::new(reader),
        size: sc.framing.length,
    };

    let frames = chunks
        .into_iter()
        .map_while(Result::ok)
        .map(|b| Frame::decode(&b));

    let mut summary = Summary::default();
    for frame in frames {
        debug!(header=?frame.header, "frame");
        summary.collect_frame(&frame);
    }

    write_text_summary(stdout(), &summary)
}

const TEXT_TEMPLATE: &str = r#"======================================================================================================
Frames:        {{ summary.total_frames }}
Bytes:         {{ summary.total_bytes }} 
Missing:       {{ summary.missing_frames}}
------------------------------------------------------------------------------------------------------
VCID  Frames      Bytes       Missing     Corrected   Uncorr.     Ok          Error       NotPerf.
------------------------------------------------------------------------------------------------------
{{ #each vcids }}
{{ lpad 4 this.vcid }}
{{~ lpad 12 this.total_frames }}
{{~ lpad 12 this.total_bytes }}
{{~ lpad 12 this.missing_frames }}
{{~ lpad 12 this.corrected }}
{{~ lpad 12 this.uncorrectable }}
{{~ lpad 12 this.ok }}
{{~ lpad 12 this.error }}
{{~ lpad 12 this.not_performed }}
{{/each}}
"#;

fn write_text_summary<W: Write>(mut w: W, summary: &Summary) -> Result<()> {
    let mut hb = crate::render::engine();
    assert!(hb.register_template_string("main", TEXT_TEMPLATE).is_ok());

    let mut ordered_vcids: Vec<Info> = summary.vcids.values().cloned().collect();
    ordered_vcids.sort_unstable_by_key(|a| a.vcid);

    #[derive(Debug, Serialize)]
    struct Data {
        vcids: Vec<Info>,
        summary: Summary,
    }

    let content = hb
        .render(
            "main",
            &Data {
                vcids: ordered_vcids,
                summary: summary.clone(),
            },
        )
        .context("rendering text")?;
    w.write_all(content.as_bytes()).context("writing tempalte")
}
