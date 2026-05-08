use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Result, anyhow};

use super::{CachedAssetSpec, Game, SlotSpec};
use crate::github::ReleaseAsset;
use crate::platform::{Platform, linux, macos};

pub const SLOT_SM64: &str = "sm64";

const SLOTS: &[SlotSpec] = &[SlotSpec {
    id: SLOT_SM64,
    display_name: "Super Mario 64",
    symlink_filename: "sm64.z64",
}];

const CACHED_ASSETS: &[CachedAssetSpec] = &[CachedAssetSpec {
    slot_id: SLOT_SM64,
    filenames: &["sm64.o2r"],
}];

pub struct Ghostship;

impl Game for Ghostship {
    fn slug(&self) -> &'static str {
        "ghostship"
    }

    fn repo_slug(&self) -> &'static str {
        "HarbourMasters/Ghostship"
    }

    fn display_name(&self) -> &'static str {
        "Ghostship"
    }

    fn rom_group_name(&self) -> &'static str {
        "Super Mario 64"
    }

    fn data_dir(&self, install_dir: &Path, _platform: &dyn Platform) -> PathBuf {
        install_dir.to_path_buf()
    }

    fn slots(&self) -> &'static [SlotSpec] {
        SLOTS
    }

    fn cached_assets(&self) -> &'static [CachedAssetSpec] {
        CACHED_ASSETS
    }

    fn requires_rom_copy(&self) -> bool {
        false
    }

    fn pick_asset<'a>(
        &self,
        assets: &'a [ReleaseAsset],
        platform: &dyn Platform,
    ) -> Option<&'a ReleaseAsset> {
        let keyword = platform.asset_keyword().to_ascii_lowercase();
        assets
            .iter()
            .find(|a| a.name.to_ascii_lowercase().contains(&keyword))
    }

    fn launch_command(&self, install_dir: &Path, platform: &dyn Platform) -> Command {
        let bin = match platform.asset_keyword() {
            "Mac" => install_dir.join("Ghostship.app/Contents/MacOS/Ghostship"),
            "Linux" => install_dir.join("ghostship.appimage"),
            _ => install_dir.join("ghostship"),
        };
        let mut cmd = Command::new(bin);
        cmd.current_dir(install_dir);
        cmd
    }

    fn extract(&self, archive: &Path, dest: &Path, platform: &dyn Platform) -> Result<()> {
        match platform.asset_keyword() {
            "Mac" => macos::install_app_in_dmg_release(archive, dest),
            "Linux" => linux::install_appimage_release(archive, dest, "ghostship.appimage"),
            other => Err(anyhow!("Ghostship: unsupported platform keyword {other}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::{linux::Linux, macos::MacOs};
    use std::collections::HashSet;
    use std::fs;
    use std::io::Write;
    use tempfile::tempdir;

    fn asset(name: &str) -> ReleaseAsset {
        ReleaseAsset {
            name: name.into(),
            browser_download_url: String::new(),
            size: 0,
        }
    }

    fn fixture_assets() -> Vec<ReleaseAsset> {
        vec![
            asset("Ghostship-Dutchman-Charlie-Linux.zip"),
            asset("Ghostship-Dutchman-Charlie-Mac.zip"),
            asset("Ghostship-Dutchman-Charlie-Win64.zip"),
        ]
    }

    #[test]
    fn picks_mac_asset_on_macos() {
        let assets = fixture_assets();
        let picked = Ghostship.pick_asset(&assets, &MacOs).unwrap();
        assert_eq!(picked.name, "Ghostship-Dutchman-Charlie-Mac.zip");
    }

    #[test]
    fn picks_linux_asset_on_linux() {
        let assets = fixture_assets();
        let picked = Ghostship.pick_asset(&assets, &Linux).unwrap();
        assert_eq!(picked.name, "Ghostship-Dutchman-Charlie-Linux.zip");
    }

    #[test]
    fn returns_none_when_no_matching_asset() {
        let assets = vec![asset("Ghostship-Dutchman-Charlie-Win64.zip")];
        assert!(Ghostship.pick_asset(&assets, &MacOs).is_none());
        assert!(Ghostship.pick_asset(&assets, &Linux).is_none());
    }

    #[test]
    fn slots_returns_single_sm64_slot() {
        let slots = Ghostship.slots();
        assert_eq!(slots.len(), 1);
        assert_eq!(slots[0].id, SLOT_SM64);
        assert_eq!(slots[0].symlink_filename, "sm64.z64");
        assert_eq!(slots[0].display_name, "Super Mario 64");
    }

    #[test]
    fn cached_asset_slot_ids_match_declared_slots() {
        let slot_ids: HashSet<&str> = Ghostship.slots().iter().map(|s| s.id).collect();
        for ca in Ghostship.cached_assets() {
            assert!(slot_ids.contains(ca.slot_id));
        }
    }

    #[test]
    fn data_dir_is_install_dir() {
        let install = Path::new("/some/install");
        assert_eq!(Ghostship.data_dir(install, &MacOs), install);
    }

    #[test]
    fn extract_linux_unzips_full_release_tree() {
        let dir = tempdir().unwrap();
        let archive = dir.path().join("release.zip");
        let f = fs::File::create(&archive).unwrap();
        let mut w = zip::ZipWriter::new(f);
        let opts: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        w.start_file("ghostship.appimage", opts).unwrap();
        w.write_all(b"appimage-body").unwrap();
        w.start_file("gamecontrollerdb.txt", opts).unwrap();
        w.write_all(b"db").unwrap();
        w.finish().unwrap();

        let dest = dir.path().join("install");
        Ghostship.extract(&archive, &dest, &Linux).unwrap();

        let bin = dest.join("ghostship.appimage");
        assert!(bin.exists());
        assert_eq!(fs::read(&bin).unwrap(), b"appimage-body");
        assert!(dest.join("gamecontrollerdb.txt").exists());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&bin).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o755);
        }
    }
}
