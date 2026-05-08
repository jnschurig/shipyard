use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Result, anyhow};

use super::{CachedAssetSpec, Game, SlotSpec};
use crate::github::ReleaseAsset;
use crate::platform::{Platform, linux, macos};

pub const SLOT_MK64: &str = "mk64";

const SLOTS: &[SlotSpec] = &[SlotSpec {
    id: SLOT_MK64,
    display_name: "Mario Kart 64",
    symlink_filename: "mk64.z64",
}];

const CACHED_ASSETS: &[CachedAssetSpec] = &[CachedAssetSpec {
    slot_id: SLOT_MK64,
    filenames: &["mk64.o2r"],
}];

pub struct SpaghettiKart;

impl Game for SpaghettiKart {
    fn slug(&self) -> &'static str {
        "spaghettikart"
    }

    fn repo_slug(&self) -> &'static str {
        "HarbourMasters/SpaghettiKart"
    }

    fn display_name(&self) -> &'static str {
        "SpaghettiKart"
    }

    fn rom_group_name(&self) -> &'static str {
        "Mario Kart 64"
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
        let needle = match platform.asset_keyword() {
            "Linux" => "linux",
            "Mac" => match std::env::consts::ARCH {
                "aarch64" => "mac-arm64",
                "x86_64" => "mac-intel",
                _ => return None,
            },
            _ => return None,
        };
        assets
            .iter()
            .find(|a| a.name.to_ascii_lowercase().contains(needle))
    }

    fn launch_command(&self, install_dir: &Path, platform: &dyn Platform) -> Command {
        let bin = match platform.asset_keyword() {
            "Linux" => install_dir.join("spaghetti.appimage"),
            "Mac" => install_dir.join("Spaghettify"),
            _ => install_dir.join("spaghetti"),
        };
        let mut cmd = Command::new(bin);
        cmd.current_dir(install_dir);
        cmd
    }

    fn extract(&self, archive: &Path, dest: &Path, platform: &dyn Platform) -> Result<()> {
        match platform.asset_keyword() {
            "Linux" => linux::install_appimage_release(archive, dest, "spaghetti.appimage"),
            // Mac release is a flat zip (binary + assets at root, no .app, no DMG).
            "Mac" => macos::install_flat_binary_release(archive, dest, "Spaghettify"),
            other => Err(anyhow!(
                "SpaghettiKart: unsupported platform keyword {other}"
            )),
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
            asset("spaghetti-linux.zip"),
            asset("spaghetti-mac-arm64.zip"),
            asset("spaghetti-mac-intel-x64.zip"),
            asset("spaghetti-windows.zip"),
        ]
    }

    #[test]
    fn picks_linux_asset_on_linux() {
        let assets = fixture_assets();
        let picked = SpaghettiKart.pick_asset(&assets, &Linux).unwrap();
        assert_eq!(picked.name, "spaghetti-linux.zip");
    }

    #[test]
    fn returns_none_when_no_matching_asset() {
        let assets = vec![asset("spaghetti-windows.zip")];
        assert!(SpaghettiKart.pick_asset(&assets, &Linux).is_none());
    }

    #[test]
    #[cfg(target_arch = "aarch64")]
    fn picks_mac_arm64_asset_on_apple_silicon() {
        let assets = fixture_assets();
        let picked = SpaghettiKart.pick_asset(&assets, &MacOs).unwrap();
        assert_eq!(picked.name, "spaghetti-mac-arm64.zip");
    }

    #[test]
    #[cfg(target_arch = "x86_64")]
    fn picks_mac_intel_asset_on_intel() {
        let assets = fixture_assets();
        let picked = SpaghettiKart.pick_asset(&assets, &MacOs).unwrap();
        assert_eq!(picked.name, "spaghetti-mac-intel-x64.zip");
    }

    #[test]
    fn extract_mac_unzips_flat_layout_and_chmods_binary() {
        let dir = tempdir().unwrap();
        let archive = dir.path().join("release.zip");
        let f = fs::File::create(&archive).unwrap();
        let mut w = zip::ZipWriter::new(f);
        let opts: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        w.start_file("Spaghettify", opts).unwrap();
        w.write_all(b"binary-body").unwrap();
        w.start_file("config.yml", opts).unwrap();
        w.write_all(b"k: v").unwrap();
        w.finish().unwrap();

        let dest = dir.path().join("install");
        SpaghettiKart.extract(&archive, &dest, &MacOs).unwrap();

        let bin = dest.join("Spaghettify");
        assert!(bin.exists());
        assert_eq!(fs::read(&bin).unwrap(), b"binary-body");
        assert!(dest.join("config.yml").exists());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&bin).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o755);
        }
    }

    #[test]
    fn slots_returns_single_mk64_slot() {
        let slots = SpaghettiKart.slots();
        assert_eq!(slots.len(), 1);
        assert_eq!(slots[0].id, SLOT_MK64);
        assert_eq!(slots[0].symlink_filename, "mk64.z64");
        assert_eq!(slots[0].display_name, "Mario Kart 64");
    }

    #[test]
    fn cached_asset_slot_ids_match_declared_slots() {
        let slot_ids: HashSet<&str> = SpaghettiKart.slots().iter().map(|s| s.id).collect();
        for ca in SpaghettiKart.cached_assets() {
            assert!(slot_ids.contains(ca.slot_id));
        }
    }

    #[test]
    fn data_dir_is_install_dir() {
        let install = Path::new("/some/install");
        assert_eq!(SpaghettiKart.data_dir(install, &Linux), install);
    }

    #[test]
    fn extract_linux_unzips_full_release_tree() {
        let dir = tempdir().unwrap();
        let archive = dir.path().join("release.zip");
        let f = fs::File::create(&archive).unwrap();
        let mut w = zip::ZipWriter::new(f);
        let opts: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        w.start_file("spaghetti.appimage", opts).unwrap();
        w.write_all(b"appimage-body").unwrap();
        w.start_file("gamecontrollerdb.txt", opts).unwrap();
        w.write_all(b"db").unwrap();
        w.finish().unwrap();

        let dest = dir.path().join("install");
        SpaghettiKart.extract(&archive, &dest, &Linux).unwrap();

        let bin = dest.join("spaghetti.appimage");
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
