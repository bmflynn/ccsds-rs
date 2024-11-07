use std::{collections::HashSet, fs::File, path::Path};

use crate::SCID;

const SPACECRAFTSDB: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/spacecraftsdb.json"
));

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
    pub fn with_file<P: AsRef<Path>>(path: P) -> Result<Spacecrafts, std::io::Error> {
        let mut db: spacecrafts::DB = serde_json::from_reader(File::open(path)?)?;
        let file_ids: HashSet<SCID> = db.spacecrafts.iter().map(|sc| sc.scid).collect();

        let builtin: spacecrafts::DB =
            serde_json::from_str(SPACECRAFTSDB).expect("built-in spacecraft db is not valid");

        for sc in &builtin.spacecrafts {
            // skip any that already exist from the file
            if file_ids.contains(&sc.scid) {
                continue;
            }
            db.spacecrafts.push(sc.clone());
        }

        Ok(Self { db })
    }

    pub fn all(&self) -> Vec<spacecrafts::Spacecraft> {
        self.db.spacecrafts.clone()
    }

    pub fn lookup(&self, scid: SCID) -> Option<spacecrafts::Spacecraft> {
        self.db.find(scid)
    }
}
