//! Detect whether a game's cached ROM archives (`.o2r` / `.otr`) already
//! exist on disk. Used to label per-version "Clear cache" buttons and to
//! show what each install has generated.
//!
//! Always reads the filesystem fresh — never cached. A few `stat` calls in
//! one directory cost far less than the risk of stale cached state.

use std::fs;
use std::path::PathBuf;

use crate::games::{CachedAssetSpec, Game};
use crate::platform::Platform;

#[derive(Debug, Clone)]
pub struct CachedAssetPresence {
    pub slot_id: &'static str,
    pub status: CachedAssetStatus,
}

#[derive(Debug, Clone)]
pub enum CachedAssetStatus {
    Present {
        /// The filename that was found (first match from the spec's ordered list).
        filename: &'static str,
        path: PathBuf,
        size: u64,
    },
    Missing,
}

impl CachedAssetStatus {
    pub fn is_present(&self) -> bool {
        matches!(self, CachedAssetStatus::Present { .. })
    }
}

/// For every `CachedAssetSpec` declared by the game, stat candidate filenames
/// in the declared order. Return the first match (`Present`) or `Missing`.
pub fn scan_cached_assets(
    game: &dyn Game,
    install_dir: &std::path::Path,
    platform: &dyn Platform,
) -> Vec<CachedAssetPresence> {
    let data_dir = game.data_dir(install_dir, platform);
    game.cached_assets()
        .iter()
        .map(|spec| CachedAssetPresence {
            slot_id: spec.slot_id,
            status: first_present(&data_dir, spec),
        })
        .collect()
}

fn first_present(data_dir: &std::path::Path, spec: &CachedAssetSpec) -> CachedAssetStatus {
    for filename in spec.filenames {
        let path = data_dir.join(filename);
        if let Ok(meta) = fs::metadata(&path)
            && meta.is_file()
        {
            return CachedAssetStatus::Present {
                filename,
                path,
                size: meta.len(),
            };
        }
    }
    CachedAssetStatus::Missing
}

/// Paths + sizes a `clear` would delete, for the confirmation dialog. Pure —
/// no side effects. Return value matches what `clear_cached_assets` would
/// attempt to remove given identical inputs.
#[derive(Debug, Clone)]
pub struct PlannedClear {
    pub slot_id: &'static str,
    pub filename: &'static str,
    pub path: PathBuf,
    pub size: u64,
}

pub fn plan_clear(
    game: &dyn Game,
    install_dir: &std::path::Path,
    platform: &dyn Platform,
) -> Vec<PlannedClear> {
    scan_cached_assets(game, install_dir, platform)
        .into_iter()
        .filter_map(|presence| match presence.status {
            CachedAssetStatus::Present {
                filename,
                path,
                size,
            } => Some(PlannedClear {
                slot_id: presence.slot_id,
                filename,
                path,
                size,
            }),
            CachedAssetStatus::Missing => None,
        })
        .collect()
}

/// Best-effort delete. Returns the set of files actually removed (with their
/// byte size at time of read) plus a list of per-file failures. A partial
/// clear is valid and reportable; callers surface failures individually.
#[derive(Debug)]
pub struct ClearResult {
    pub deleted: Vec<(PathBuf, u64)>,
    pub failures: Vec<(PathBuf, std::io::Error)>,
}

pub fn clear_cached_assets(
    game: &dyn Game,
    install_dir: &std::path::Path,
    platform: &dyn Platform,
) -> ClearResult {
    let plan = plan_clear(game, install_dir, platform);
    let mut deleted = Vec::with_capacity(plan.len());
    let mut failures = Vec::new();
    for p in plan {
        match fs::remove_file(&p.path) {
            Ok(()) => deleted.push((p.path, p.size)),
            Err(e) => failures.push((p.path, e)),
        }
    }
    ClearResult { deleted, failures }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::games::soh::{SLOT_OOT, SLOT_OOT_MQ, Soh};
    use crate::platform::Platform;
    use std::path::Path;
    use tempfile::tempdir;

    // Linux-like fake platform: Soh::data_dir uses install_dir on non-Mac,
    // which lets us stage cached-asset files inside a tempdir.
    struct LinuxFake;
    impl Platform for LinuxFake {
        fn default_library_root(&self) -> PathBuf {
            PathBuf::from("/tmp")
        }
        fn config_dir(&self) -> PathBuf {
            PathBuf::from("/tmp")
        }
        fn cache_dir(&self) -> PathBuf {
            PathBuf::from("/tmp")
        }
        fn asset_keyword(&self) -> &'static str {
            "Linux"
        }
        fn extract(&self, _: &Path, _: &Path) -> anyhow::Result<()> {
            Ok(())
        }
    }

    fn write(dir: &Path, name: &str, body: &[u8]) {
        fs::write(dir.join(name), body).unwrap();
    }

    #[test]
    fn scan_detects_primary_filename() {
        let dir = tempdir().unwrap();
        write(dir.path(), "oot.o2r", b"rom-archive");
        let out = scan_cached_assets(&Soh, dir.path(), &LinuxFake);
        let oot = out.iter().find(|p| p.slot_id == SLOT_OOT).unwrap();
        let mq = out.iter().find(|p| p.slot_id == SLOT_OOT_MQ).unwrap();
        assert!(oot.status.is_present());
        assert!(!mq.status.is_present());
        if let CachedAssetStatus::Present { filename, size, .. } = &oot.status {
            assert_eq!(*filename, "oot.o2r");
            assert_eq!(*size, b"rom-archive".len() as u64);
        }
    }

    #[test]
    fn scan_falls_back_to_legacy_otr_when_o2r_absent() {
        let dir = tempdir().unwrap();
        write(dir.path(), "oot.otr", b"legacy");
        let out = scan_cached_assets(&Soh, dir.path(), &LinuxFake);
        let oot = out.iter().find(|p| p.slot_id == SLOT_OOT).unwrap();
        if let CachedAssetStatus::Present { filename, .. } = &oot.status {
            assert_eq!(*filename, "oot.otr");
        } else {
            panic!("expected Present");
        }
    }

    #[test]
    fn scan_returns_all_missing_when_dir_does_not_exist() {
        let dir = tempdir().unwrap();
        let non_existent = dir.path().join("never-created");
        let out = scan_cached_assets(&Soh, &non_existent, &LinuxFake);
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|c| !c.status.is_present()));
    }

    #[test]
    fn plan_clear_lists_present_files_only() {
        let dir = tempdir().unwrap();
        write(dir.path(), "oot.o2r", b"aa");
        write(dir.path(), "oot-mq.otr", b"bbb");

        let plan = plan_clear(&Soh, dir.path(), &LinuxFake);
        assert_eq!(plan.len(), 2);
        let sizes: std::collections::BTreeSet<u64> = plan.iter().map(|p| p.size).collect();
        assert_eq!(sizes, [2, 3].into_iter().collect());
    }

    #[test]
    fn clear_removes_all_present_files_and_scan_returns_missing() {
        let dir = tempdir().unwrap();
        write(dir.path(), "oot.o2r", b"aa");
        write(dir.path(), "oot-mq.otr", b"bbb");

        let result = clear_cached_assets(&Soh, dir.path(), &LinuxFake);
        assert_eq!(result.deleted.len(), 2);
        assert!(result.failures.is_empty());

        let after = scan_cached_assets(&Soh, dir.path(), &LinuxFake);
        assert!(after.iter().all(|p| !p.status.is_present()));
    }

    #[test]
    fn clear_on_empty_dir_is_a_noop() {
        let dir = tempdir().unwrap();
        let result = clear_cached_assets(&Soh, dir.path(), &LinuxFake);
        assert!(result.deleted.is_empty());
        assert!(result.failures.is_empty());
    }
}
