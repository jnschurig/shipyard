pub mod schema;

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use directories::ProjectDirs;
use thiserror::Error;

pub use schema::{CURRENT_SCHEMA_VERSION, Config};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Diagnostic {
    ConfigParseError {
        backup: PathBuf,
        message: String,
    },
    SchemaVersionMismatch {
        backup: PathBuf,
        found: u32,
    },
    /// A ROM file referenced by an older config version was missing on disk;
    /// migration dropped the slot assignment.
    RomMigrationSkipped {
        path: PathBuf,
    },
    /// A ROM file referenced by an older config version failed to import
    /// (e.g. permission denied); migration dropped the slot assignment.
    RomMigrationFailed {
        path: PathBuf,
        message: String,
    },
}

/// Source description for a slot assignment that needs to be materialized into
/// the ROM library after a migration. Produced by `Config::load_from`,
/// consumed by app startup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RomImportRequest {
    pub game_slug: String,
    pub slot_id: String,
    pub source_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingMigration {
    pub from_version: u32,
    pub rom_imports: Vec<RomImportRequest>,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("no home directory available")]
    NoHome,
    #[error("io error on {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("serialize error: {0}")]
    Serialize(#[from] serde_yaml::Error),
}

pub struct LoadedConfig {
    pub config: Config,
    pub path: PathBuf,
    pub diagnostic: Option<Diagnostic>,
    pub pending_migration: Option<PendingMigration>,
}

fn project_dirs() -> Result<ProjectDirs, ConfigError> {
    ProjectDirs::from("", "", "Shipyard").ok_or(ConfigError::NoHome)
}

pub fn config_path() -> Result<PathBuf, ConfigError> {
    Ok(project_dirs()?.config_dir().join("config.yaml"))
}

impl Config {
    pub fn load() -> Result<LoadedConfig, ConfigError> {
        let path = config_path()?;
        Self::load_from(&path)
    }

    pub fn load_from(path: &Path) -> Result<LoadedConfig, ConfigError> {
        let raw = match fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Ok(LoadedConfig {
                    config: Config::default(),
                    path: path.to_path_buf(),
                    diagnostic: None,
                    pending_migration: None,
                });
            }
            Err(e) => {
                return Err(ConfigError::Io {
                    path: path.to_path_buf(),
                    source: e,
                });
            }
        };

        let value: serde_yaml::Value = match serde_yaml::from_str(&raw) {
            Ok(v) => v,
            Err(e) => {
                let backup = backup_malformed(path, &raw)?;
                return Ok(LoadedConfig {
                    config: Config::default(),
                    path: path.to_path_buf(),
                    diagnostic: Some(Diagnostic::ConfigParseError {
                        backup,
                        message: e.to_string(),
                    }),
                    pending_migration: None,
                });
            }
        };

        let found_version = value
            .get("schema_version")
            .and_then(|v| v.as_u64())
            .unwrap_or(CURRENT_SCHEMA_VERSION as u64) as u32;

        // Forward-compat: a newer schema we don't understand → backup + default,
        // matching the prior behavior so we never overwrite an unknown future config.
        if found_version > CURRENT_SCHEMA_VERSION {
            let backup = backup_malformed(path, &raw)?;
            return Ok(LoadedConfig {
                config: Config::default(),
                path: path.to_path_buf(),
                diagnostic: Some(Diagnostic::SchemaVersionMismatch {
                    backup,
                    found: found_version,
                }),
                pending_migration: None,
            });
        }

        if found_version < CURRENT_SCHEMA_VERSION {
            let (config, pending) = migrate(found_version, &value);
            return Ok(LoadedConfig {
                config,
                path: path.to_path_buf(),
                diagnostic: None,
                pending_migration: Some(pending),
            });
        }

        match serde_yaml::from_value::<Config>(value) {
            Ok(config) => Ok(LoadedConfig {
                config,
                path: path.to_path_buf(),
                diagnostic: None,
                pending_migration: None,
            }),
            Err(e) => {
                let backup = backup_malformed(path, &raw)?;
                Ok(LoadedConfig {
                    config: Config::default(),
                    path: path.to_path_buf(),
                    diagnostic: Some(Diagnostic::ConfigParseError {
                        backup,
                        message: e.to_string(),
                    }),
                    pending_migration: None,
                })
            }
        }
    }

    pub fn save(&self) -> Result<(), ConfigError> {
        let path = config_path()?;
        self.save_to(&path)
    }

    pub fn save_to(&self, path: &Path) -> Result<(), ConfigError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| ConfigError::Io {
                path: parent.to_path_buf(),
                source: e,
            })?;
        }

        let yaml = serde_yaml::to_string(self)?;

        let tmp = path.with_extension("yaml.tmp");
        {
            let mut f = fs::File::create(&tmp).map_err(|e| ConfigError::Io {
                path: tmp.clone(),
                source: e,
            })?;
            f.write_all(yaml.as_bytes()).map_err(|e| ConfigError::Io {
                path: tmp.clone(),
                source: e,
            })?;
            f.sync_all().map_err(|e| ConfigError::Io {
                path: tmp.clone(),
                source: e,
            })?;
        }
        fs::rename(&tmp, path).map_err(|e| ConfigError::Io {
            path: path.to_path_buf(),
            source: e,
        })?;
        Ok(())
    }
}

/// Produce a current-schema `Config` and a `PendingMigration` describing
/// filesystem work (ROM imports) that must happen during startup before the
/// new config is considered fully migrated. Pure: never touches the filesystem.
fn migrate(from_version: u32, raw: &serde_yaml::Value) -> (Config, PendingMigration) {
    let mut config = Config::default();

    if let Some(root) = raw.get("library_root").and_then(|v| v.as_str()) {
        config.library_root = Some(PathBuf::from(root));
    }
    if let Some(overrides) = raw.get("install_overrides").and_then(|v| v.as_mapping()) {
        for (k, v) in overrides {
            if let (Some(k), Some(v)) = (k.as_str(), v.as_str()) {
                config
                    .install_overrides
                    .insert(k.to_string(), PathBuf::from(v));
            }
        }
    }

    let mut rom_imports = Vec::new();

    // Pre-v4 stored single-path-per-slot under `roms.{oot,oot_mq}`. Pull those
    // forward as ROM library imports + slot assignments under the SoH game.
    if let Some(roms) = raw.get("roms") {
        for (yaml_key, slot_id) in [("oot", "oot"), ("oot_mq", "oot-mq")] {
            if let Some(p) = roms.get(yaml_key).and_then(|v| v.as_str()) {
                rom_imports.push(RomImportRequest {
                    game_slug: "soh".to_string(),
                    slot_id: slot_id.to_string(),
                    source_path: PathBuf::from(p),
                });
            }
        }
    }

    // Forward any v4-shaped slot_assignments that may already be present
    // (defensive — a partially-migrated user state).
    if let Some(map) = raw.get("slot_assignments").and_then(|v| v.as_mapping()) {
        for (game_k, slot_map) in map {
            let Some(game_slug) = game_k.as_str() else {
                continue;
            };
            let Some(slot_map) = slot_map.as_mapping() else {
                continue;
            };
            for (slot_k, fname_v) in slot_map {
                if let (Some(slot_id), Some(fname)) = (slot_k.as_str(), fname_v.as_str()) {
                    config.set_assignment(game_slug, slot_id, Some(fname.to_string()));
                }
            }
        }
    }

    (
        config,
        PendingMigration {
            from_version,
            rom_imports,
        },
    )
}

fn backup_malformed(path: &Path, contents: &str) -> Result<PathBuf, ConfigError> {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let name = format!(
        "{}.bak.{}",
        path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("config"),
        ts
    );
    let backup = path.with_file_name(name);
    fs::write(&backup, contents).map_err(|e| ConfigError::Io {
        path: backup.clone(),
        source: e,
    })?;
    Ok(backup)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::tempdir;

    #[test]
    fn round_trip_default() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.yaml");

        let cfg = Config::default();
        cfg.save_to(&path).unwrap();

        let loaded = Config::load_from(&path).unwrap();
        assert_eq!(loaded.config, cfg);
        assert!(loaded.diagnostic.is_none());
        assert!(loaded.pending_migration.is_none());
    }

    #[test]
    fn round_trip_populated() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.yaml");

        let mut install_overrides = std::collections::HashMap::new();
        install_overrides.insert("9.2.3".to_string(), PathBuf::from("/custom/9.2.3"));
        let mut cfg = Config {
            library_root: Some(PathBuf::from("/some/library")),
            install_overrides,
            ..Config::default()
        };
        cfg.set_assignment("soh", "oot", Some("oot.z64".to_string()));

        cfg.save_to(&path).unwrap();
        let loaded = Config::load_from(&path).unwrap();
        assert_eq!(loaded.config, cfg);
        assert!(loaded.pending_migration.is_none());
    }

    #[test]
    fn missing_file_returns_default() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nope.yaml");

        let loaded = Config::load_from(&path).unwrap();
        assert_eq!(loaded.config, Config::default());
        assert!(loaded.diagnostic.is_none());
    }

    #[test]
    fn malformed_file_is_backed_up_and_defaulted() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.yaml");

        std::fs::write(&path, "::: not yaml ::: {[}").unwrap();
        let loaded = Config::load_from(&path).unwrap();

        assert_eq!(loaded.config, Config::default());
        assert!(matches!(
            loaded.diagnostic,
            Some(Diagnostic::ConfigParseError { .. })
        ));
    }

    #[test]
    fn newer_schema_version_is_backed_up_and_defaulted() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.yaml");

        std::fs::write(&path, "schema_version: 999\nlibrary_root: /foo\n").unwrap();
        let loaded = Config::load_from(&path).unwrap();

        assert_eq!(loaded.config, Config::default());
        match loaded.diagnostic.expect("expected diagnostic") {
            Diagnostic::SchemaVersionMismatch { found, backup } => {
                assert_eq!(found, 999);
                assert!(backup.exists());
            }
            other => panic!("unexpected diagnostic: {other:?}"),
        }
    }

    #[test]
    fn second_backup_does_not_clobber_first() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.yaml");

        std::fs::write(&path, "garbage 1").unwrap();
        let _ = Config::load_from(&path).unwrap();
        std::thread::sleep(std::time::Duration::from_secs(1));
        std::fs::write(&path, "garbage 2").unwrap();
        let _ = Config::load_from(&path).unwrap();

        let backups: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".bak."))
            .collect();
        assert_eq!(backups.len(), 2);
    }

    #[test]
    fn v3_with_rom_paths_produces_pending_migration_no_side_effects() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        std::fs::write(
            &path,
            "schema_version: 3\nlibrary_root: /lib\nroms:\n  oot: /tmp/oot.z64\n  oot_mq: /tmp/mq.z64\n",
        )
        .unwrap();
        let original = std::fs::read_to_string(&path).unwrap();

        let loaded = Config::load_from(&path).unwrap();
        // Pure: original file untouched.
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);

        assert_eq!(loaded.config.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(loaded.config.library_root, Some(PathBuf::from("/lib")));
        // Slot assignments aren't populated yet — that happens after the
        // imports succeed during startup orchestration.
        assert!(loaded.config.slot_assignments.is_empty());

        let pending = loaded.pending_migration.expect("pending migration");
        assert_eq!(pending.from_version, 3);
        assert_eq!(pending.rom_imports.len(), 2);
        assert!(
            pending
                .rom_imports
                .iter()
                .any(|r| r.slot_id == "oot" && r.source_path == PathBuf::from("/tmp/oot.z64"))
        );
        assert!(
            pending
                .rom_imports
                .iter()
                .any(|r| r.slot_id == "oot-mq" && r.source_path == PathBuf::from("/tmp/mq.z64"))
        );
    }

    #[test]
    fn v3_without_roms_field_migrates_with_empty_pending() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        std::fs::write(&path, "schema_version: 3\nlibrary_root: /lib\n").unwrap();

        let loaded = Config::load_from(&path).unwrap();
        assert_eq!(loaded.config.schema_version, CURRENT_SCHEMA_VERSION);
        let pending = loaded.pending_migration.expect("pending migration");
        assert!(pending.rom_imports.is_empty());
    }

    #[test]
    fn migrated_config_when_saved_loads_clean() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        std::fs::write(&path, "schema_version: 3\n").unwrap();

        let loaded = Config::load_from(&path).unwrap();
        assert!(loaded.pending_migration.is_some());
        loaded.config.save_to(&path).unwrap();

        let reloaded = Config::load_from(&path).unwrap();
        assert!(reloaded.pending_migration.is_none());
        assert_eq!(reloaded.config.schema_version, CURRENT_SCHEMA_VERSION);
    }

    #[test]
    fn v4_migrates_to_v5_with_defaults_for_new_fields() {
        use crate::config::schema::DEFAULT_VERSIONS_TO_SHOW;
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        std::fs::write(
            &path,
            "schema_version: 4\nlibrary_root: /lib\nslot_assignments:\n  soh:\n    oot: oot.z64\n",
        )
        .unwrap();

        let loaded = Config::load_from(&path).unwrap();
        assert_eq!(loaded.config.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(loaded.config.library_root, Some(PathBuf::from("/lib")));
        assert_eq!(loaded.config.assignment_for("soh", "oot"), Some("oot.z64"));
        assert_eq!(loaded.config.versions_to_show, DEFAULT_VERSIONS_TO_SHOW);
        assert!(loaded.config.last_launched.is_none());
        assert!(loaded.config.rate_limit_snapshot.is_none());
    }

    #[test]
    fn fresh_default_uses_documented_defaults() {
        use crate::config::schema::DEFAULT_VERSIONS_TO_SHOW;
        let cfg = Config::default();
        assert_eq!(cfg.versions_to_show, DEFAULT_VERSIONS_TO_SHOW);
        assert!(cfg.last_launched.is_none());
        assert!(cfg.rate_limit_snapshot.is_none());
    }

    #[test]
    fn clear_assignments_referencing_drops_every_match() {
        let mut cfg = Config::default();
        cfg.set_assignment("soh", "oot", Some("rom.z64".to_string()));
        cfg.set_assignment("soh", "oot-mq", Some("rom.z64".to_string()));
        cfg.set_assignment("soh", "other", Some("kept.z64".to_string()));
        let cleared = cfg.clear_assignments_referencing("rom.z64");
        assert_eq!(cleared, 2);
        assert_eq!(cfg.assignment_for("soh", "oot"), None);
        assert_eq!(cfg.assignment_for("soh", "oot-mq"), None);
        assert_eq!(cfg.assignment_for("soh", "other"), Some("kept.z64"));
    }

    #[test]
    fn assignment_helpers_round_trip() {
        let mut cfg = Config::default();
        cfg.set_assignment("soh", "oot", Some("oot.z64".to_string()));
        assert_eq!(cfg.assignment_for("soh", "oot"), Some("oot.z64"));
        assert_eq!(cfg.assignment_for("soh", "oot-mq"), None);
        cfg.set_assignment("soh", "oot", None);
        assert_eq!(cfg.assignment_for("soh", "oot"), None);
        assert!(cfg.slot_assignments.is_empty());
    }
}
