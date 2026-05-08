use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, anyhow};

use super::{CachedAssetSpec, Game, SlotSpec};
use crate::github::ReleaseAsset;
use crate::library::extract::{find_first_with_ext_recursive, unzip};
use crate::platform::Platform;

pub const SLOT_SF64: &str = "sf64";

const SLOTS: &[SlotSpec] = &[SlotSpec {
    id: SLOT_SF64,
    display_name: "Star Fox 64",
    symlink_filename: "sf64.z64",
}];

const CACHED_ASSETS: &[CachedAssetSpec] = &[CachedAssetSpec {
    slot_id: SLOT_SF64,
    filenames: &["sf64.o2r"],
}];

pub struct Starship;

impl Game for Starship {
    fn slug(&self) -> &'static str {
        "starship"
    }

    fn repo_slug(&self) -> &'static str {
        "HarbourMasters/Starship"
    }

    fn display_name(&self) -> &'static str {
        "Starship"
    }

    fn rom_group_name(&self) -> &'static str {
        "Star Fox 64"
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
        if platform.asset_keyword() != "Linux" {
            return None;
        }
        assets
            .iter()
            .find(|a| a.name.to_ascii_lowercase().contains("linux"))
    }

    fn launch_command(&self, install_dir: &Path, platform: &dyn Platform) -> Command {
        let bin = match platform.asset_keyword() {
            "Linux" => install_dir.join("starship.appimage"),
            _ => install_dir.join("starship"),
        };
        let mut cmd = Command::new(bin);
        cmd.current_dir(install_dir);
        cmd
    }

    fn extract(&self, archive: &Path, dest: &Path, platform: &dyn Platform) -> Result<()> {
        match platform.asset_keyword() {
            "Linux" => extract_linux(archive, dest),
            other => Err(anyhow!(
                "Starship: unsupported platform keyword {other} (Linux-only in v1)"
            )),
        }
    }
}

fn extract_linux(archive: &Path, dest: &Path) -> Result<()> {
    let scratch = tempfile::tempdir().context("mktemp scratch dir")?;
    unzip(archive, scratch.path()).context("unzip starship release")?;

    let appimage = find_first_with_ext_recursive(scratch.path(), "appimage")?;
    fs::create_dir_all(dest).with_context(|| format!("create dest {}", dest.display()))?;
    let target = dest.join(appimage.file_name().unwrap());
    fs::copy(&appimage, &target).with_context(|| format!("copy to {}", target.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&target, fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::{linux::Linux, macos::MacOs};
    use std::collections::HashSet;
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
            asset("Starship-Barnard-Alfa-Linux.zip"),
            asset("Starship-Barnard-Alfa-Switch.zip"),
            asset("Starship-Barnard-Alfa-Windows.zip"),
        ]
    }

    #[test]
    fn picks_linux_asset_on_linux() {
        let assets = fixture_assets();
        let picked = Starship.pick_asset(&assets, &Linux).unwrap();
        assert_eq!(picked.name, "Starship-Barnard-Alfa-Linux.zip");
    }

    #[test]
    fn returns_none_when_no_matching_asset() {
        let assets = vec![asset("Starship-Barnard-Alfa-Windows.zip")];
        assert!(Starship.pick_asset(&assets, &Linux).is_none());
    }

    #[test]
    fn returns_none_on_macos() {
        // Linux-only in v1: refuse on macOS so install fails before download
        // even if upstream ever ships a Mac asset.
        let assets = fixture_assets();
        assert!(Starship.pick_asset(&assets, &MacOs).is_none());
    }

    #[test]
    fn slots_returns_single_sf64_slot() {
        let slots = Starship.slots();
        assert_eq!(slots.len(), 1);
        assert_eq!(slots[0].id, SLOT_SF64);
        assert_eq!(slots[0].symlink_filename, "sf64.z64");
        assert_eq!(slots[0].display_name, "Star Fox 64");
    }

    #[test]
    fn cached_asset_slot_ids_match_declared_slots() {
        let slot_ids: HashSet<&str> = Starship.slots().iter().map(|s| s.id).collect();
        for ca in Starship.cached_assets() {
            assert!(slot_ids.contains(ca.slot_id));
        }
    }

    #[test]
    fn data_dir_is_install_dir() {
        let install = Path::new("/some/install");
        assert_eq!(Starship.data_dir(install, &Linux), install);
    }

    #[test]
    fn extract_linux_handles_nested_appimage() {
        let dir = tempdir().unwrap();
        let archive = dir.path().join("release.zip");
        let f = fs::File::create(&archive).unwrap();
        let mut w = zip::ZipWriter::new(f);
        let opts: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        w.start_file("Starship-Barnard-Alfa-Linux/readme.txt", opts)
            .unwrap();
        w.write_all(b"readme").unwrap();
        w.start_file("Starship-Barnard-Alfa-Linux/starship.appimage", opts)
            .unwrap();
        w.write_all(b"appimage-body").unwrap();
        w.finish().unwrap();

        let dest = dir.path().join("install");
        Starship.extract(&archive, &dest, &Linux).unwrap();

        let target = dest.join("starship.appimage");
        assert!(target.exists(), "expected {} to exist", target.display());
        assert_eq!(fs::read(&target).unwrap(), b"appimage-body");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&target).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o755);
        }
    }
}
