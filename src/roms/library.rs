//! Managed ROM library. Imported ROMs are stored as plain files under
//! `<config_dir>/roms/`. No format detection, hashing, or validation —
//! whatever the user picks is trusted.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::config::{Config, Diagnostic, PendingMigration};
use crate::platform::Platform;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RomEntry {
    pub filename: String,
    pub size: u64,
}

pub fn library_root(platform: &dyn Platform) -> PathBuf {
    platform.config_dir().join("roms")
}

/// List ROM files in `library_root`, sorted by filename. A missing directory
/// returns an empty list (the directory is created lazily on first import).
pub fn list(library_root: &Path) -> io::Result<Vec<RomEntry>> {
    let read = match fs::read_dir(library_root) {
        Ok(it) => it,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut out = Vec::new();
    for entry in read {
        let entry = entry?;
        let meta = entry.metadata()?;
        if !meta.is_file() {
            continue;
        }
        // Skip in-flight imports.
        let Some(name) = entry.file_name().to_str().map(|s| s.to_owned()) else {
            continue;
        };
        if name.ends_with(".partial") {
            continue;
        }
        out.push(RomEntry {
            filename: name,
            size: meta.len(),
        });
    }
    out.sort_by(|a, b| a.filename.cmp(&b.filename));
    Ok(out)
}

/// Copy `src` into `library_root`. If a file with the same filename already
/// exists, append `-1`, `-2`, … before the extension until a free name is
/// found. Returns the resulting `RomEntry`.
pub fn import(library_root: &Path, src: &Path) -> io::Result<RomEntry> {
    fs::create_dir_all(library_root)?;
    let original = src
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "rom source has no filename"))?
        .to_owned();
    let final_name = pick_unique_name(library_root, &original);
    let final_path = library_root.join(&final_name);
    let partial = library_root.join(format!("{final_name}.partial"));

    // Best-effort cleanup of any leftover .partial from a prior crash.
    let _ = fs::remove_file(&partial);

    fs::copy(src, &partial)?;
    if let Err(e) = fs::rename(&partial, &final_path) {
        let _ = fs::remove_file(&partial);
        return Err(e);
    }
    let size = fs::metadata(&final_path)?.len();
    Ok(RomEntry {
        filename: final_name,
        size,
    })
}

pub fn delete(library_root: &Path, filename: &str) -> io::Result<()> {
    fs::remove_file(library_root.join(filename))
}

/// Materialize a `PendingMigration` from `Config::load_from`: import each
/// referenced ROM into the library and populate `config.slot_assignments`.
/// Each failed import becomes a `Diagnostic` and is otherwise skipped.
pub fn apply_pending_migration(
    library_root: &Path,
    config: &mut Config,
    pending: PendingMigration,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for req in pending.rom_imports {
        match import(library_root, &req.source_path) {
            Ok(entry) => {
                config.set_assignment(&req.game_slug, &req.slot_id, Some(entry.filename));
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                diagnostics.push(Diagnostic::RomMigrationSkipped {
                    path: req.source_path,
                });
            }
            Err(e) => {
                diagnostics.push(Diagnostic::RomMigrationFailed {
                    path: req.source_path,
                    message: e.to_string(),
                });
            }
        }
    }
    diagnostics
}

fn pick_unique_name(library_root: &Path, original: &str) -> String {
    if !library_root.join(original).exists() {
        return original.to_owned();
    }
    let (stem, ext) = split_stem_ext(original);
    let mut n: u32 = 1;
    loop {
        let candidate = match ext {
            Some(ext) => format!("{stem}-{n}.{ext}"),
            None => format!("{stem}-{n}"),
        };
        if !library_root.join(&candidate).exists() {
            return candidate;
        }
        n += 1;
    }
}

fn split_stem_ext(name: &str) -> (&str, Option<&str>) {
    match name.rfind('.') {
        Some(0) | None => (name, None),
        Some(i) => (&name[..i], Some(&name[i + 1..])),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write(p: &Path, body: &[u8]) {
        fs::write(p, body).unwrap();
    }

    #[test]
    fn import_creates_library_root_and_copies_file() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("oot.z64");
        write(&src, b"hello rom");
        let lib = dir.path().join("library");
        let entry = import(&lib, &src).unwrap();
        assert_eq!(entry.filename, "oot.z64");
        assert_eq!(entry.size, b"hello rom".len() as u64);
        assert_eq!(fs::read(lib.join("oot.z64")).unwrap(), b"hello rom");
    }

    #[test]
    fn import_collision_appends_numeric_suffix() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("oot.z64");
        write(&src, b"a");
        let lib = dir.path().join("lib");
        let e1 = import(&lib, &src).unwrap();
        let e2 = import(&lib, &src).unwrap();
        let e3 = import(&lib, &src).unwrap();
        assert_eq!(e1.filename, "oot.z64");
        assert_eq!(e2.filename, "oot-1.z64");
        assert_eq!(e3.filename, "oot-2.z64");
    }

    #[test]
    fn import_collision_with_no_extension() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("noext");
        write(&src, b"x");
        let lib = dir.path().join("lib");
        let _ = import(&lib, &src).unwrap();
        let e2 = import(&lib, &src).unwrap();
        assert_eq!(e2.filename, "noext-1");
    }

    #[test]
    fn list_is_sorted_and_skips_partial() {
        let dir = tempdir().unwrap();
        let lib = dir.path().join("lib");
        fs::create_dir_all(&lib).unwrap();
        write(&lib.join("b.z64"), b"bb");
        write(&lib.join("a.z64"), b"a");
        write(&lib.join("ignored.z64.partial"), b"in flight");
        let entries = list(&lib).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].filename, "a.z64");
        assert_eq!(entries[1].filename, "b.z64");
    }

    #[test]
    fn list_missing_dir_is_empty() {
        let dir = tempdir().unwrap();
        let entries = list(&dir.path().join("nope")).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn apply_pending_migration_imports_and_assigns() {
        use crate::config::{Config, PendingMigration, RomImportRequest};

        let dir = tempdir().unwrap();
        let src = dir.path().join("oot.z64");
        write(&src, b"rom-bytes");
        let lib = dir.path().join("lib");

        let mut config = Config::default();
        let pending = PendingMigration {
            from_version: 3,
            rom_imports: vec![RomImportRequest {
                game_slug: "soh".to_string(),
                slot_id: "oot".to_string(),
                source_path: src.clone(),
            }],
        };
        let diags = apply_pending_migration(&lib, &mut config, pending);
        assert!(diags.is_empty());
        assert_eq!(config.assignment_for("soh", "oot"), Some("oot.z64"));
        assert!(lib.join("oot.z64").exists());
    }

    #[test]
    fn apply_pending_migration_records_missing_source_diagnostic() {
        use crate::config::{Config, Diagnostic, PendingMigration, RomImportRequest};

        let dir = tempdir().unwrap();
        let lib = dir.path().join("lib");

        let mut config = Config::default();
        let pending = PendingMigration {
            from_version: 3,
            rom_imports: vec![RomImportRequest {
                game_slug: "soh".to_string(),
                slot_id: "oot".to_string(),
                source_path: PathBuf::from("/this/path/does/not/exist.z64"),
            }],
        };
        let diags = apply_pending_migration(&lib, &mut config, pending);
        assert_eq!(diags.len(), 1);
        assert!(matches!(diags[0], Diagnostic::RomMigrationSkipped { .. }));
        assert!(config.slot_assignments.is_empty());
    }

    #[test]
    fn delete_removes_file() {
        let dir = tempdir().unwrap();
        let lib = dir.path().join("lib");
        fs::create_dir_all(&lib).unwrap();
        write(&lib.join("oot.z64"), b"x");
        delete(&lib, "oot.z64").unwrap();
        assert!(list(&lib).unwrap().is_empty());
    }
}
