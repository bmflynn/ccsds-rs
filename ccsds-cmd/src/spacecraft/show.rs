use std::io::{stdout, Write};

use anyhow::{Context, Result};
use serde::Serialize;
use spacecrafts::{FrameType, Spacecraft, TimecodeConfig};

/// Show the configuration of a single spacecraft as text tables.
pub fn show(sc: &Spacecraft) -> Result<()> {
    let content = render_text(sc)?;
    stdout()
        .write_all(content.as_bytes())
        .context("writing to stdout")
}

#[derive(Debug, Serialize)]
struct FramingView {
    frame_type: String,
    length: usize,
    cadu_length: usize,
    asm: String,
    pseudo_noise: String,
    reed_solomon: String,
    izone_length: usize,
    ocf: String,
    fec: String,
    fhec: String,
}

#[derive(Debug, Serialize)]
struct VcidView {
    vcid: u16,
    description: String,
}

#[derive(Debug, Serialize)]
struct ApidView {
    apid: u16,
    vcid: u16,
    sensor: String,
    timecode: String,
    description: String,
}

#[derive(Debug, Serialize)]
struct TimecodeView {
    name: String,
    format: String,
    epoch: String,
    params: String,
}

#[derive(Debug, Serialize)]
struct View {
    name: String,
    aliases: String,
    scid: u16,
    norad_id: u32,
    framing: FramingView,
    vcid_count: usize,
    vcids: Vec<VcidView>,
    apid_count: usize,
    apid_sensor_width: usize,
    apid_timecode_width: usize,
    apids: Vec<ApidView>,
    timecode_count: usize,
    timecodes: Vec<TimecodeView>,
}

fn yes_no(v: bool) -> String {
    if v { "yes" } else { "no" }.to_string()
}

fn or_dash(s: &str) -> String {
    if s.is_empty() { "-" } else { s }.to_string()
}

fn build_view(sc: &Spacecraft) -> View {
    let f = &sc.framing;
    let frame_type = match f.frame_type {
        FrameType::CcsdsAos => "ccsds_aos",
        FrameType::CcsdsTm => "ccsds_tm",
        FrameType::None => "none",
    };
    let asm = f.asm.iter().map(|b| format!("{b:02X}")).collect::<String>();
    let reed_solomon = match &f.reed_solomon {
        Some(rs) => format!(
            "interleave={} correctable={} virtual_fill={}",
            rs.interleave, rs.num_correctable, rs.virtual_fill_length
        ),
        None => "no".to_string(),
    };

    let mut vcids: Vec<VcidView> = sc
        .vcids
        .iter()
        .map(|c| VcidView {
            vcid: c.vcid,
            description: c.description.clone(),
        })
        .collect();
    vcids.sort_unstable_by_key(|v| v.vcid);

    let mut apids: Vec<ApidView> = sc
        .apids
        .iter()
        .map(|a| ApidView {
            apid: a.apid,
            vcid: a.vcid,
            sensor: or_dash(&a.sensor),
            timecode: a.timecode.clone().unwrap_or_else(|| "-".to_string()),
            description: a.description.clone(),
        })
        .collect();
    apids.sort_unstable_by_key(|a| (a.apid, a.vcid));
    let apid_sensor_width = apids
        .iter()
        .map(|a| a.sensor.len())
        .max()
        .unwrap_or(0)
        .max("Sensor".len());
    let apid_timecode_width = apids
        .iter()
        .map(|a| a.timecode.len())
        .max()
        .unwrap_or(0)
        .max("Timecode".len());

    let mut timecodes: Vec<TimecodeView> = sc
        .timecodes
        .iter()
        .map(|(name, tc)| match tc {
            TimecodeConfig::CDS {
                epoch,
                day_length,
                submillis_length,
                self_identifying,
            } => TimecodeView {
                name: name.clone(),
                format: "cds".to_string(),
                epoch: epoch.clone(),
                params: format!(
                    "day_length={day_length} submillis_length={submillis_length} self_identifying={}",
                    yes_no(*self_identifying)
                ),
            },
            TimecodeConfig::CUC {
                epoch,
                basic_length,
                fine_length,
                fine_nanos,
                self_identifying,
            } => TimecodeView {
                name: name.clone(),
                format: "cuc".to_string(),
                epoch: epoch.clone(),
                params: format!(
                    "basic_length={basic_length} fine_length={fine_length} fine_nanos={fine_nanos} self_identifying={}",
                    yes_no(*self_identifying)
                ),
            },
        })
        .collect();
    timecodes.sort_unstable_by(|a, b| a.name.cmp(&b.name));

    View {
        name: sc.name.clone(),
        aliases: if sc.aliases.is_empty() {
            "-".to_string()
        } else {
            sc.aliases.join(", ")
        },
        scid: sc.scid,
        norad_id: sc.norad_catalog_id,
        framing: FramingView {
            frame_type: frame_type.to_string(),
            length: f.length,
            cadu_length: sc.cadu_length(),
            asm,
            pseudo_noise: yes_no(f.pseudo_noise.is_some()),
            reed_solomon,
            izone_length: f.izone_length,
            ocf: yes_no(f.ocf_present),
            fec: yes_no(f.fec_present),
            fhec: yes_no(f.fhec_present),
        },
        vcid_count: vcids.len(),
        vcids,
        apid_count: apids.len(),
        apid_sensor_width,
        apid_timecode_width,
        apids,
        timecode_count: timecodes.len(),
        timecodes,
    }
}

const TEXT_TEMPLATE: &str = r#"======================================================================================================
Name:     {{ name }}
Aliases:  {{ aliases }}
SCID:     {{ scid }}
NORAD ID: {{ norad_id }}
------------------------------------------------------------------------------------------------------
Framing
------------------------------------------------------------------------------------------------------
Type:          {{ framing.frame_type }}
Frame length:  {{ framing.length }}
CADU length:   {{ framing.cadu_length }}
ASM:           {{ framing.asm }}
Pseudo-noise:  {{ framing.pseudo_noise }}
Reed-Solomon:  {{ framing.reed_solomon }}
Insert zone:   {{ framing.izone_length }}
OCF:           {{ framing.ocf }}
FEC:           {{ framing.fec }}
FHEC:          {{ framing.fhec }}
------------------------------------------------------------------------------------------------------
VCIDs ({{ vcid_count }})
------------------------------------------------------------------------------------------------------
VCID  Description
------------------------------------------------------------------------------------------------------
{{ #each vcids }}{{ lpad 4 this.vcid }}  {{ this.description }}
{{/each }}
------------------------------------------------------------------------------------------------------
APIDs ({{ apid_count }})
------------------------------------------------------------------------------------------------------
APID    VCID  {{ rpad apid_sensor_width "Sensor" }}  {{ rpad apid_timecode_width "Timecode" }}  Description
------------------------------------------------------------------------------------------------------
{{ #each apids }}{{ lpad 5 this.apid }}   {{ lpad 4 this.vcid }}  {{ rpad ../apid_sensor_width this.sensor }}  {{ rpad ../apid_timecode_width this.timecode }}  {{ this.description }}
{{/each }}
------------------------------------------------------------------------------------------------------
Timecodes ({{ timecode_count }})
------------------------------------------------------------------------------------------------------
Name                  Format  Epoch                 Parameters
------------------------------------------------------------------------------------------------------
{{ #each timecodes }}{{ rpad 20 this.name }}  {{ rpad 6 this.format }}  {{ rpad 20 this.epoch }}  {{ this.params }}
{{/each }}
"#;

fn render_text(sc: &Spacecraft) -> Result<String> {
    let mut hb = crate::render::engine();
    hb.register_template_string("show", TEXT_TEMPLATE)
        .context("registering template")?;
    hb.render("show", &build_view(sc)).context("rendering text")
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn test_spacecraft() -> Spacecraft {
        serde_json::from_str(
            r#"{
              "scid": 178,
              "name": "test",
              "aliases": ["a", "b"],
              "noradCatalogID": 12345,
              "framing": {
                "length": 1115,
                "insertZoneLength": 9,
                "ocfPresent": true,
                "pseudoNoise": {},
                "reedSolomon": {"interleave": 5, "numCorrectable": 16},
                "type": "ccsds_aos"
              },
              "vcids": [{"vcid": 16, "description": "VIIRS sci"}, {"vcid": 1}],
              "apids": [
                {"apid": 826, "vcid": 16, "timecode": "cds", "sensor": "viirs", "description": "VIIRS science"},
                {"apid": 11, "vcid": 1}
              ],
              "timecodes": {
                "cds": {"format": "cds", "epoch": "1958-01-01T00:00:00Z", "dayLength": 2, "submillisLength": 2}
              }
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn view_is_sorted_and_formatted() {
        let view = build_view(&test_spacecraft());
        let vcids: Vec<u16> = view.vcids.iter().map(|v| v.vcid).collect();
        assert_eq!(vcids, vec![1, 16]);
        assert_eq!(view.vcids[1].description, "VIIRS sci");
        assert_eq!(view.apids[0].apid, 11);
        assert_eq!(view.apids[0].timecode, "-");
        assert_eq!(view.aliases, "a, b");
        assert_eq!(view.framing.cadu_length, 4 + 1115 + 5 * 32);
        assert_eq!(view.framing.asm, "1ACFFC1D");
        assert_eq!(view.apids[0].sensor, "-");
        assert_eq!(view.apids[1].sensor, "viirs");
        assert_eq!(view.apid_sensor_width, 6);
        assert_eq!(view.framing.ocf, "yes");
        assert_eq!(view.framing.fec, "no");
    }

    #[test]
    fn renders_text() {
        let out = render_text(&test_spacecraft()).unwrap();
        assert!(out.contains("Name:     test\n"), "{out}");
        assert!(out.contains("SCID:     178\n"), "{out}");
        assert!(out.contains("VCIDs (2)\n"), "{out}");
        assert!(out.contains("VCID  Description\n"), "{out}");
        assert!(out.contains("\n   1  \n  16  VIIRS sci\n"), "{out}");
        assert!(out.contains("ASM:           1ACFFC1D\n"), "{out}");
        assert!(
            out.contains("APID    VCID  Sensor  Timecode  Description\n"),
            "{out}"
        );
        assert!(
            out.contains("\n   11      1  -       -         \n"),
            "{out}"
        );
        assert!(
            out.contains("\n  826     16  viirs   cds       VIIRS science\n"),
            "{out}"
        );
        assert!(
            out.contains("cds                   cds     1958-01-01T00:00:00Z  day_length=2 submillis_length=2 self_identifying=no\n"),
            "{out}"
        );
    }
}
