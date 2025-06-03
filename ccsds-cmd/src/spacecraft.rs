use super::spacecrafts::Spacecrafts;
use anyhow::{bail, Context, Result};
use ccsds::framing::Scid;
use handlebars::handlebars_helper;
use serde::Serialize;
use std::{
    io::{stdout, Write},
    path::Path,
    vec::Vec,
};

pub fn spacecraft_info<P: AsRef<Path>>(
    path: Option<P>,
    scid: Option<Scid>,
    show_apids: bool,
    show_vcids: bool,
) -> Result<()> {
    let spacecrafts = match path {
        Some(path) => Spacecrafts::with_file(&path, false).with_context(|| {
            format!("loading spacecrafts from {:?}", path.as_ref().to_path_buf())
        })?,
        None => Spacecrafts::default(),
    };

    let output = match scid {
        Some(scid) => {
            let Some(sc) = spacecrafts.lookup(scid) else {
                bail!("No spacecraft found for scid={scid}");
            };
            let data = SpacecraftRenderData {
                sc,
                show_framing: true,
                show_apids,
                show_vcids,
            };
            render_spacecraft(data)
        }
        None => render_spacecrafts(spacecrafts.all()),
    }
    .context("rendering")?;
    stdout()
        .write_all(str::as_bytes(&output))
        .context("writing to stdout")
}

#[derive(Serialize)]
struct SpacecraftsRenderData {
    spacecrafts: Vec<spacecrafts::Spacecraft>,
}

#[derive(Serialize)]
struct SpacecraftRenderData {
    sc: spacecrafts::Spacecraft,
    show_framing: bool,
    show_vcids: bool,
    show_apids: bool,
}

fn setup_handlebars() -> handlebars::Handlebars<'static> {
    let mut hb = handlebars::Handlebars::new();

    handlebars_helper!(join: |arr: array, sep: str| {
        let strings: Vec<String> = arr.iter().filter_map(|v| {
            match v {
                serde_json::Value::String(s) => Some(s.to_string()),
                _ => None
            }
        }).collect();

        strings.join(sep)
    });
    hb.register_helper("join", Box::new(join));

    handlebars_helper!(left_pad: |num: u64, v: Json| {
        let v = match v {
            serde_json::Value::String(s) => s.to_owned(),
            serde_json::Value::Null => String::new(),
            _ => v.to_string()
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

    hb
}

fn render_spacecraft(data: SpacecraftRenderData) -> Result<String> {
    let mut hb = setup_handlebars();

    hb.register_template_string("template", SPACECRAFT_TEMPLATE)
        .context("registering template")?;

    hb.render("template", &data).context("rendering text")
}

fn render_spacecrafts(spacecrafts: Vec<spacecrafts::Spacecraft>) -> Result<String> {
    let mut hb = setup_handlebars();

    hb.register_template_string("template", SPACECRAFTS_TEMPLATE)
        .context("registering template")?;

    hb.render("template", &SpacecraftsRenderData { spacecrafts })
        .context("rendering text")
}

const SPACECRAFTS_TEMPLATE: &str = r#"
-------------------------------------------------------------------------------------------
SCID    Name       CatNum    Aliases
-------------------------------------------------------------------------------------------
{{ #each spacecrafts }}
{{ lpad 4 scid }}    {{ rpad 10 name }} {{ rpad 9 catalogNumber }} {{ join aliases ", " }}
{{ /each ~}}
"#;

const SPACECRAFT_TEMPLATE: &str = r#"
===========================================================================================
Spacecraft SCID {{ sc.scid }}
===========================================================================================
  Name:        {{ sc.name }}
  SCID:        {{ sc.scid }}
  Aliases:     {{ join sc.aliases "," }}
  Catalog Num: {{ sc.catalogNumber }}
{{ #if show_framing and sc.framing ~}}
===========================================================================================
Framing 
-------------------------------------------------------------------------------------------
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
{{ #each sc.vcids }}{{ lpad 4 vcid }}   {{ description }}
{{ /each ~}}
{{ /if ~}}
{{ #if show_apids ~}}
-------------------------------------------------------------------------------------------
VCID    APID    Sensor              Size (min/max)   Description 
-------------------------------------------------------------------------------------------
{{ #each sc.vcids ~}}
{{ #each apids }}
{{ lpad 4 ../vcid }}    {{ lpad 4 apid }}    {{ rpad 18 sensor }}  {{lpad 5 minSize}}/{{lpad 5 maxSize}}  {{ description }}
{{ /each ~}}
{{ /each }}
{{ /if ~}}
"#;
