//! Launch-time symlink reconciliation. Right before spawning a game, we
//! ensure each declared slot's `<install_dir>/<symlink_filename>` is a
//! symlink pointing at the assigned ROM in the library, or, if the slot is
//! unassigned, that no such file exists. This runs unconditionally on every
//! launch — see `reconcile` for why we don't try to skip it when a cached
//! archive is already present.

use std::io;
use std::path::Path;

use crate::config::Config;
use crate::games::Game;
use crate::platform::Platform;

/// Reconcile slot ROM placement for a single install. Each assigned slot's
/// `<install_dir>/<symlink_filename>` is symlinked to the ROM in the library
/// — this is where each supported game's extractor scans for ROMs (CWD- or
/// bundle-relative path that resolves to the install dir at runtime).
///
/// Earlier iterations skipped this when a cached `.o2r` was already present,
/// but that was unsafe for games like 2Ship whose cached archive lives in a
/// user-global path: a stale archive from a previous install can trigger a
/// "regenerate ROM" prompt at launch, and the ROM needs to be findable in
/// the install dir at that moment. Symlinking is cheap (a few syscalls)
/// compared to that failure mode.
pub fn reconcile(
    install_dir: &Path,
    game: &dyn Game,
    _platform: &dyn Platform,
    config: &Config,
    library_root: &Path,
) -> io::Result<()> {
    for slot in game.slots() {
        let symlink_path = install_dir.join(slot.symlink_filename);
        match config.assignment_for(game.slug(), slot.id) {
            Some(filename) => {
                let target = library_root.join(filename);
                place_symlink(&symlink_path, &target)?;
            }
            None => match std::fs::remove_file(&symlink_path) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            },
        }
    }
    Ok(())
}

/// Place a symlink at `dest` pointing at `target`, atomically (symlink → rename).
/// No-ops when an existing symlink at `dest` already points at `target`.
#[cfg(unix)]
fn place_symlink(dest: &Path, target: &Path) -> io::Result<()> {
    use std::os::unix::fs::symlink;
    if let Ok(existing) = std::fs::read_link(dest)
        && existing == target
    {
        return Ok(());
    }

    // Sibling temp path with `.tmp` appended to the existing filename, so we
    // don't assume any particular extension on `dest`.
    let tmp = match dest.file_name() {
        Some(name) => {
            let mut t = name.to_os_string();
            t.push(".tmp");
            dest.with_file_name(t)
        }
        None => return Err(io::Error::other("symlink dest has no filename")),
    };
    let _ = std::fs::remove_file(&tmp);
    symlink(target, &tmp)?;
    if let Err(e) = std::fs::rename(&tmp, dest) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

#[cfg(not(unix))]
fn place_symlink(_dest: &Path, _target: &Path) -> io::Result<()> {
    tracing::warn!("symlink reconciliation is not implemented on this platform");
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::games::soh::{SLOT_OOT, SLOT_OOT_MQ, Soh};
    use crate::games::{CachedAssetSpec, Game, SlotSpec};
    use crate::github::ReleaseAsset;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use tempfile::tempdir;

    struct FakePlatform;
    impl Platform for FakePlatform {
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
    }

    fn write(p: &Path, body: &[u8]) {
        fs::write(p, body).unwrap();
    }

    fn make_rom(library_root: &Path, name: &str) -> PathBuf {
        fs::create_dir_all(library_root).unwrap();
        let p = library_root.join(name);
        write(&p, b"rom-bytes");
        p
    }

    #[test]
    fn missing_cached_and_assigned_creates_symlink() {
        let dir = tempdir().unwrap();
        let install = dir.path().join("install");
        fs::create_dir_all(&install).unwrap();
        let lib = dir.path().join("lib");
        let target = make_rom(&lib, "oot.z64");

        let mut config = Config::default();
        config.set_assignment("soh", SLOT_OOT, Some("oot.z64".into()));

        reconcile(&install, &Soh, &FakePlatform, &config, &lib).unwrap();

        let link = install.join("oot.z64");
        assert!(link.is_symlink());
        assert_eq!(fs::read_link(&link).unwrap(), target);
        // oot-mq is unassigned → no file created
        assert!(!install.join("oot-mq.z64").exists());
    }

    #[test]
    fn copies_rom_even_when_cached_archive_present() {
        // Reconcile no longer skips when the cached archive exists — see the
        // doc comment on `reconcile` for why.
        let dir = tempdir().unwrap();
        let install = dir.path().join("install");
        fs::create_dir_all(&install).unwrap();
        write(&install.join("oot.o2r"), b"baked");
        let lib = dir.path().join("lib");
        make_rom(&lib, "oot.z64");

        let mut config = Config::default();
        config.set_assignment("soh", SLOT_OOT, Some("oot.z64".into()));

        reconcile(&install, &Soh, &FakePlatform, &config, &lib).unwrap();

        assert!(install.join("oot.z64").is_file());
    }

    #[test]
    fn unassigned_removes_stale_copy() {
        let dir = tempdir().unwrap();
        let install = dir.path().join("install");
        fs::create_dir_all(&install).unwrap();
        let lib = dir.path().join("lib");
        make_rom(&lib, "old.z64");
        // Stale ROM file (could be an old copy or symlink from a previous run).
        write(&install.join("oot.z64"), b"stale");

        let config = Config::default();
        reconcile(&install, &Soh, &FakePlatform, &config, &lib).unwrap();

        assert!(!install.join("oot.z64").exists());
    }

    #[test]
    fn reassignment_updates_target_atomically() {
        let dir = tempdir().unwrap();
        let install = dir.path().join("install");
        fs::create_dir_all(&install).unwrap();
        let lib = dir.path().join("lib");
        let a = make_rom(&lib, "a.z64");
        let b = make_rom(&lib, "b.z64");

        let mut config = Config::default();
        config.set_assignment("soh", SLOT_OOT, Some("a.z64".into()));
        reconcile(&install, &Soh, &FakePlatform, &config, &lib).unwrap();
        assert_eq!(
            fs::read(install.join("oot.z64")).unwrap(),
            fs::read(&a).unwrap()
        );

        config.set_assignment("soh", SLOT_OOT, Some("b.z64".into()));
        reconcile(&install, &Soh, &FakePlatform, &config, &lib).unwrap();
        assert_eq!(
            fs::read(install.join("oot.z64")).unwrap(),
            fs::read(&b).unwrap()
        );
    }

    #[test]
    fn reconcile_is_idempotent() {
        let dir = tempdir().unwrap();
        let install = dir.path().join("install");
        fs::create_dir_all(&install).unwrap();
        let lib = dir.path().join("lib");
        make_rom(&lib, "oot.z64");

        let mut config = Config::default();
        config.set_assignment("soh", SLOT_OOT, Some("oot.z64".into()));
        reconcile(&install, &Soh, &FakePlatform, &config, &lib).unwrap();
        reconcile(&install, &Soh, &FakePlatform, &config, &lib).unwrap();
        reconcile(&install, &Soh, &FakePlatform, &config, &lib).unwrap();
        assert!(install.join("oot.z64").is_file());
    }

    #[test]
    fn reconcile_leaves_unrelated_files_untouched() {
        let dir = tempdir().unwrap();
        let install = dir.path().join("install");
        fs::create_dir_all(&install).unwrap();
        write(&install.join("readme.txt"), b"hi");
        write(&install.join("game.exe"), b"bin");
        let lib = dir.path().join("lib");
        make_rom(&lib, "oot.z64");

        let mut config = Config::default();
        config.set_assignment("soh", SLOT_OOT_MQ, Some("oot.z64".into()));
        reconcile(&install, &Soh, &FakePlatform, &config, &lib).unwrap();

        assert!(install.join("readme.txt").exists());
        assert!(install.join("game.exe").exists());
    }

    /// A game whose cached_assets have a slot_id NOT in slots() — proves we
    /// only iterate slots() and never inspect arbitrary install-dir contents.
    struct GameWithSlotButNoCached;
    impl Game for GameWithSlotButNoCached {
        fn slug(&self) -> &'static str {
            "fakegame"
        }
        fn repo_slug(&self) -> &'static str {
            "x/y"
        }
        fn display_name(&self) -> &'static str {
            "Fake"
        }
        fn data_dir(&self, install_dir: &Path, _: &dyn Platform) -> PathBuf {
            install_dir.to_path_buf()
        }
        fn slots(&self) -> &'static [SlotSpec] {
            const S: &[SlotSpec] = &[SlotSpec {
                id: "primary",
                display_name: "Primary",
                symlink_filename: "primary.rom",
            }];
            S
        }
        fn cached_assets(&self) -> &'static [CachedAssetSpec] {
            &[]
        }
        fn pick_asset<'a>(
            &self,
            a: &'a [ReleaseAsset],
            _: &dyn Platform,
        ) -> Option<&'a ReleaseAsset> {
            a.first()
        }
        fn launch_command(&self, _: &Path, _: &dyn Platform) -> Command {
            Command::new("/bin/true")
        }
        fn extract(&self, _: &Path, _: &Path, _: &dyn Platform) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn reconcile_uses_slot_declared_filename() {
        let dir = tempdir().unwrap();
        let install = dir.path().join("install");
        fs::create_dir_all(&install).unwrap();
        let lib = dir.path().join("lib");
        make_rom(&lib, "any.bin");

        let mut config = Config::default();
        config.set_assignment("fakegame", "primary", Some("any.bin".into()));
        reconcile(
            &install,
            &GameWithSlotButNoCached,
            &FakePlatform,
            &config,
            &lib,
        )
        .unwrap();

        assert!(install.join("primary.rom").is_file());
    }
}
