use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const MANIFEST_FILE: &str = ".shipyard-install.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstallManifest {
    pub tag: String,
    pub game_slug: String,
    pub installed_at: DateTime<Utc>,
    pub archive_sha256: Option<String>,
}

impl InstallManifest {
    pub fn path_in(dir: &Path) -> PathBuf {
        dir.join(MANIFEST_FILE)
    }

    pub fn read(dir: &Path) -> Result<Option<Self>> {
        let p = Self::path_in(dir);
        match fs::read_to_string(&p) {
            Ok(s) => {
                let m: InstallManifest = serde_json::from_str(&s)
                    .with_context(|| format!("parse manifest {}", p.display()))?;
                Ok(Some(m))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e).with_context(|| format!("read {}", p.display())),
        }
    }

    pub fn write(&self, dir: &Path) -> Result<()> {
        let p = Self::path_in(dir);
        let json = serde_json::to_string_pretty(self)?;
        fs::write(&p, json).with_context(|| format!("write {}", p.display()))?;
        Ok(())
    }
}
