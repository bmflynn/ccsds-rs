use std::{collections::HashSet, fs::File, path::Path};

use crate::framing::Scid;

const SPACECRAFTSDB: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/spacecraftsdb.json"
));

/// Spacecraft metadata database.
///
/// This is a wrapper around the [spacecrafts](https://crates.io/crates/spacecrafts) crate.
///
/// The default implementation uses an embeded spacecrafts [database](https://github.com/bmflynn/spacecraftsdb)
/// that was included at compile-time. To use a specific/custom database use [with_file](Spacecrafts).
///
/// # Example
/// ```
/// use ccsds::spacecrafts::Spacecrafts;
///
/// let spacecrafts = Spacecrafts::default();
/// let snpp = spacecrafts.lookup(157).unwrap();
/// assert_eq!(snpp.scid, 157);
/// ```
#[derive(Debug)]
pub struct Spacecrafts {
    db: spacecrafts::DB,
}

impl Default for Spacecrafts {
    fn default() -> Self {
        let db: spacecrafts::DB =
            serde_json::from_str(SPACECRAFTSDB).expect("built-in spacecraft db is not valid");
        Self { db }
    }
}

impl Spacecrafts {
    pub fn with_file<P: AsRef<Path>>(
        path: P,
        built_in: bool,
    ) -> Result<Spacecrafts, std::io::Error> {
        let mut db: spacecrafts::DB = serde_json::from_reader(File::open(path)?)?;
        let file_ids: HashSet<Scid> = db.spacecrafts.iter().map(|sc| sc.scid).collect();

        if built_in {
            let builtin: spacecrafts::DB =
                serde_json::from_str(SPACECRAFTSDB).expect("built-in spacecraft db is not valid");

            for sc in &builtin.spacecrafts {
                // skip any that already exist from the file
                if file_ids.contains(&sc.scid) {
                    continue;
                }
                db.spacecrafts.push(sc.clone());
            }
        }

        Ok(Self { db })
    }

    pub fn all(&self) -> Vec<spacecrafts::Spacecraft> {
        self.db.spacecrafts.clone()
    }

    pub fn lookup(&self, scid: Scid) -> Option<spacecrafts::Spacecraft> {
        self.db.find(scid)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn default() {
        let spacecrafts = Spacecrafts::default();
        let snpp = spacecrafts.lookup(157).unwrap();
        assert_eq!(snpp.scid, 157);
    }

    #[test]
    fn all() {
        let spacecrafts = Spacecrafts::default();
        spacecrafts.all();
    }

    #[test]
    fn with_file() {
        let tmpdir = tempfile::tempdir().unwrap();
        let out_path = tmpdir.path().join("spacecrafts.json");
        fs::write(
            &out_path,
            r#"{
  "spacecrafts": [
    {
      "scid": 157,
      "name": "snpp",
      "aliases": [
        "npp"
      ],
      "catalogNumber": 37849,
      "framingConfig": {
        "length": 892,
        "insertZoneLength": 0,
        "trailerLength": 0,
        "pseudoNoise": {},
        "reedSolomon": {
          "interleave": 4,
          "virtualFillLength": 0,
          "numCorrectable": 16
        }
      },
      "vcids": []
    }
  ],
  "version": "xxx",
  "gitSha": "xxx",
  "generated": "now"
}"#,
        )
        .unwrap();

        let spacecrafts = Spacecrafts::with_file(&out_path, false).unwrap();
        assert_eq!(spacecrafts.all().len(), 1, "Should only be 1 spacecraft");

        let spacecrafts = Spacecrafts::with_file(&out_path, true).unwrap();
        assert!(
            spacecrafts.all().len() > 1,
            "Should be more than 1 spacecraft when including built-ins"
        );
    }
}
