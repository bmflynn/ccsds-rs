use anyhow::{bail, Context, Result};
use ccsds::SCID;
use handlebars::handlebars_helper;
use serde::Serialize;
use spacecrafts::{Spacecraft, DB};
use std::{
    io::{stdout, Write},
    vec::Vec,
};

pub fn spacecraft_info(scid: SCID, show_apids: bool, show_vcids: bool) -> Result<()> {
    let scdb = DB::new()
        .context("loading spacecraft db")
        .with_context(|| {
            "Spacecraft database not found!

Please download a spacecraft database from https://github.com/bmflynn/spacecraftsdb/releases"
        })?;
    let Some(sc) = scdb.find(scid) else {
        bail!("No spacraft found for scid={scid}");
    };

    let data = render(&RenderData {
        sc,
        show_framing: true,
        show_apids,
        show_vcids,
    })
    .context("rendering")?;
    stdout()
        .write_all(str::as_bytes(&data))
        .context("writing to stdout")
}

#[derive(Serialize)]
struct RenderData {
    sc: Spacecraft,
    show_framing: bool,
    show_vcids: bool,
    show_apids: bool,
}

fn render(data: &RenderData) -> Result<String> {
    let mut hb = handlebars::Handlebars::new();

    handlebars_helper!(join: |arr: array, sep: str| {
        let strings: Vec<String> = arr.iter().filter_map(|v| {
            match v {
                serde_json::Value::String(s) => Some(s.to_string()),
                _ => None
            }
        }).collect();

        strings.join(sep);
    });
    hb.register_helper("join", Box::new(join));

    handlebars_helper!(left_pad: |num: u64, v: Json| {
        let v = if let serde_json::Value::String(s) = v {
            s.to_owned()
        } else {
            v.to_string()
        };
        let mut num: usize = usize::try_from(num).unwrap();
        if num < v.len() {
            num = v.len();
        }
        let mut s = String::new();
        let padding = num - v.len();
        for _ in 0..padding {
            s.push(' ');
        }
        s.push_str(&v);
        s
    });
    hb.register_helper("lpad", Box::new(left_pad));

    handlebars_helper!(right_pad: |num: u64, v: Json| {
        let mut v = if let serde_json::Value::String(s) = v {
            s.to_owned()
        } else {
            v.to_string()
        };
        let mut num: usize = usize::try_from(num).unwrap();
        if num < v.len() {
            num = v.len();
        }
        let padding = num - v.len();
        for _ in 0..padding {
            v.push(' ');
        }
        v
    });
    hb.register_helper("rpad", Box::new(right_pad));

    hb.register_template_string("info", TEXT_TEMPLATE)
        .context("registering template")?;

    hb.render("info", &data).context("rendering text")
}

const TEXT_TEMPLATE: &str = r#"
- Spacecraft ------------------------------------------------------------------------------
  Name:        {{ sc.name }}
  SCID:        {{ sc.scid }}
  Aliases:     {{ join sc.aliases "," }}
  Catalog Num: {{ sc.catalogNumber }}
{{ #if show_framing and sc.framing ~}}
- Framing ---------------------------------------------------------------------------------
Length:           {{ sc.framingConfig.length }}
InsertZoneLength: {{ sc.framingConfig.insertZoneLength }}
TrailerLength:    {{ sc.framingConfig.trailerLength }}
PseudoNoise:      {{ sc.framingConfig.pseudoNoise }}
ReedSolomon:
  Interleave:     {{ sc.framingConfig.reedSolomon.interleave }}
  VirtualFill:    {{ sc.framingConfig.reedSolomon.virtualFill }}
  NumCorrectable: {{ sc.framingConfig.reedSolomon.numCorrectable }}
{{ /if ~}}
{{~ #if show_vcids ~}}
-------------------------------------------------------------------------------------------
VCID   Description 
-------------------------------------------------------------------------------------------
{{ #each sc.vcids }}{{ lpad 4 vcid }}  {{ description }}
{{ /each ~}}
{{ /if ~}}
{{ #if show_apids ~}}
-------------------------------------------------------------------------------------------
VCID  APID  Sensor           Description 
-------------------------------------------------------------------------------------------
{{ #each sc.vcids ~}}
{{ #each apids ~}}
{{ lpad 4 ../vcid }}  {{ lpad 4 apid }}  {{ rpad 16 sensor }}  {{ description }}
{{ /each ~}}
{{ /each }}
{{ /if ~}}
"#;
