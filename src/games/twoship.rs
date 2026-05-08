use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Result, anyhow};

use super::{CachedAssetSpec, Game, SlotSpec};
use crate::github::ReleaseAsset;
use crate::library::extract::install_flat_release;
use crate::platform::{Platform, macos};

pub const SLOT_MM: &str = "mm";

const SLOTS: &[SlotSpec] = &[SlotSpec {
    id: SLOT_MM,
    display_name: "Majora's Mask",
    symlink_filename: "majoras_mask.z64",
}];

const CACHED_ASSETS: &[CachedAssetSpec] = &[CachedAssetSpec {
    slot_id: SLOT_MM,
    filenames: &["mm.o2r"],
}];

pub struct TwoShip;

impl Game for TwoShip {
    fn slug(&self) -> &'static str {
        "2ship"
    }

    fn repo_slug(&self) -> &'static str {
        "HarbourMasters/2ship2harkinian"
    }

    fn display_name(&self) -> &'static str {
        "2Ship2Harkinian"
    }

    /// Sorts after "Ship of Harkinian" in the picker; never user-visible.
    fn sort_name(&self) -> &'static str {
        "Ship of Harkinian 2"
    }

    fn rom_group_name(&self) -> &'static str {
        "Majora's Mask"
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
            "Mac" => install_dir.join("2s2h.app/Contents/MacOS/2s2h"),
            "Linux" => install_dir.join("2ship.appimage"),
            _ => install_dir.join("2s2h"),
        };
        let mut cmd = Command::new(bin);
        cmd.current_dir(install_dir);
        cmd
    }

    fn extract(&self, archive: &Path, dest: &Path, platform: &dyn Platform) -> Result<()> {
        match platform.asset_keyword() {
            "Mac" => macos::install_app_in_dmg_release(archive, dest),
            "Linux" => install_flat_release(archive, dest, "2ship.appimage"),
            other => Err(anyhow!("2Ship: unsupported platform keyword {other}")),
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
            asset("2Ship-Keiichi-Charlie-Linux.zip"),
            asset("2Ship-Keiichi-Charlie-Mac.zip"),
            asset("2Ship-Keiichi-Charlie-Win64.zip"),
        ]
    }

    #[test]
    fn picks_mac_asset_on_macos() {
        let assets = fixture_assets();
        let picked = TwoShip.pick_asset(&assets, &MacOs).unwrap();
        assert_eq!(picked.name, "2Ship-Keiichi-Charlie-Mac.zip");
    }

    #[test]
    fn picks_linux_asset_on_linux() {
        let assets = fixture_assets();
        let picked = TwoShip.pick_asset(&assets, &Linux).unwrap();
        assert_eq!(picked.name, "2Ship-Keiichi-Charlie-Linux.zip");
    }

    #[test]
    fn returns_none_when_no_matching_asset() {
        let assets = vec![asset("2Ship-Keiichi-Charlie-Win64.zip")];
        assert!(TwoShip.pick_asset(&assets, &MacOs).is_none());
        assert!(TwoShip.pick_asset(&assets, &Linux).is_none());
    }

    #[test]
    fn slots_returns_single_mm_slot() {
        let slots = TwoShip.slots();
        assert_eq!(slots.len(), 1);
        assert_eq!(slots[0].id, SLOT_MM);
        assert_eq!(slots[0].symlink_filename, "majoras_mask.z64");
        assert_eq!(slots[0].display_name, "Majora's Mask");
    }

    #[test]
    fn cached_asset_slot_ids_match_declared_slots() {
        let slot_ids: HashSet<&str> = TwoShip.slots().iter().map(|s| s.id).collect();
        for ca in TwoShip.cached_assets() {
            assert!(slot_ids.contains(ca.slot_id));
        }
    }

    #[test]
    fn data_dir_is_install_dir() {
        let install = Path::new("/some/install");
        assert_eq!(TwoShip.data_dir(install, &MacOs), install);
    }

    #[test]
    fn extract_linux_unzips_full_release_tree() {
        let dir = tempdir().unwrap();
        let archive = dir.path().join("release.zip");
        let f = fs::File::create(&archive).unwrap();
        let mut w = zip::ZipWriter::new(f);
        let opts: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        w.start_file("2ship.appimage", opts).unwrap();
        w.write_all(b"appimage-body").unwrap();
        w.start_file("readme.txt", opts).unwrap();
        w.write_all(b"readme").unwrap();
        w.finish().unwrap();

        let dest = dir.path().join("install");
        TwoShip.extract(&archive, &dest, &Linux).unwrap();

        let bin = dest.join("2ship.appimage");
        assert!(bin.exists());
        assert_eq!(fs::read(&bin).unwrap(), b"appimage-body");
        assert!(dest.join("readme.txt").exists());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&bin).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o755);
        }
    }

    // No unit test for macOS extract: 2Ship Mac zips contain a real DMG and
    // mounting one in tests would require fabricating a valid HFS image.
    // Covered by manual end-to-end run in Step 6.
}
