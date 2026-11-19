use std::{
    fs,
    io::{stdout, Write},
    path::Path,
};

use anyhow::{Context, Result};
use serde::Serialize;
use spacecrafts::Spacecraft;

/// List the spacecraft configurations found in `dir`, one row per `*.json` file.
pub fn list(dir: &Path) -> Result<()> {
    let content = render_text(&build_rows(dir)?)?;
    stdout()
        .write_all(content.as_bytes())
        .context("writing to stdout")
}

#[derive(Debug, Serialize)]
struct Row {
    name: String,
    scid: String,
    norad_id: String,
    vcids: String,
    apids: String,
    aliases: String,
}

fn build_rows(dir: &Path) -> Result<Vec<Row>> {
    let mut paths: Vec<_> = fs::read_dir(dir)
        .with_context(|| format!("reading directory {dir:?}"))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|ext| ext == "json"))
        .collect();
    paths.sort();

    let rows = paths
        .iter()
        .map(|path| {
            let file_name = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            match Spacecraft::with_file(path) {
                Ok(sc) => Row {
                    name: file_name,
                    scid: sc.scid.to_string(),
                    norad_id: sc.norad_catalog_id.to_string(),
                    vcids: sc.vcids.len().to_string(),
                    apids: sc.apids.len().to_string(),
                    aliases: sc.aliases.join(", "),
                },
                Err(err) => Row {
                    name: file_name,
                    scid: "-".to_string(),
                    norad_id: "-".to_string(),
                    vcids: "-".to_string(),
                    apids: "-".to_string(),
                    aliases: format!("error: {err}"),
                },
            }
        })
        .collect();
    Ok(rows)
}

const TEXT_TEMPLATE: &str = r#"======================================================================================================
Name                  SCID   NORAD  VCIDs  APIDs  Aliases
------------------------------------------------------------------------------------------------------
{{ #each rows }}{{ rpad 20 this.name }}  {{ lpad 5 this.scid }}  {{ lpad 5 this.norad_id }}  {{ lpad 5 this.vcids }}  {{ lpad 5 this.apids }}  {{ this.aliases }}
{{/each }}
"#;

fn render_text(rows: &[Row]) -> Result<String> {
    let mut hb = crate::render::engine();
    hb.register_template_string("list", TEXT_TEMPLATE)
        .context("registering template")?;
    hb.render("list", &serde_json::json!({ "rows": rows }))
        .context("rendering text")
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"{"scid": 1, "name": "x", "aliases": ["y"], "noradCatalogID": 7,
        "framing": {"length": 100}, "vcids": [{"vcid": 0}], "apids": []}"#;

    #[test]
    fn lists_good_and_broken_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("b.json"), GOOD).unwrap();
        fs::write(dir.path().join("a.json"), GOOD).unwrap();
        fs::write(dir.path().join("c.json"), "not json").unwrap();
        fs::write(dir.path().join("ignored.txt"), "x").unwrap();

        let rows = build_rows(dir.path()).unwrap();
        let names: Vec<_> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["a", "b", "c"]);
        assert_eq!(rows[0].scid, "1");
        assert_eq!(rows[0].vcids, "1");
        assert!(rows[2].aliases.starts_with("error:"));

        let out = render_text(&rows).unwrap();
        assert!(
            out.contains("a                         1      7      1      0  y\n"),
            "{out}"
        );
    }

    #[test]
    fn missing_dir_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(build_rows(&dir.path().join("nope")).is_err());
    }
}
