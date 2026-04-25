use std::path::{Path, PathBuf};
use std::process::Command;

use super::{CachedAssetSpec, Game, SlotSpec};
use crate::github::ReleaseAsset;
use crate::platform::Platform;

pub const SLOT_OOT: &str = "oot";
pub const SLOT_OOT_MQ: &str = "oot-mq";

const SLOTS: &[SlotSpec] = &[
    SlotSpec {
        id: SLOT_OOT,
        display_name: "Ocarina of Time",
        symlink_filename: "oot.z64",
    },
    SlotSpec {
        id: SLOT_OOT_MQ,
        display_name: "Ocarina of Time - Master Quest",
        symlink_filename: "oot-mq.z64",
    },
];

const CACHED_ASSETS: &[CachedAssetSpec] = &[
    CachedAssetSpec {
        slot_id: SLOT_OOT,
        filenames: &["oot.o2r", "oot.otr"],
    },
    CachedAssetSpec {
        slot_id: SLOT_OOT_MQ,
        filenames: &["oot-mq.o2r", "oot-mq.otr"],
    },
];

pub struct Soh;

impl Game for Soh {
    fn slug(&self) -> &'static str {
        "soh"
    }

    fn repo_slug(&self) -> &'static str {
        "HarbourMasters/Shipwright"
    }

    fn display_name(&self) -> &'static str {
        "Ship of Harkinian"
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
            "Mac" => install_dir.join("soh.app/Contents/MacOS/soh"),
            "Linux" => install_dir.join("soh.appimage"),
            _ => install_dir.join("soh"),
        };
        let mut cmd = Command::new(bin);
        cmd.current_dir(install_dir);
        cmd
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::ReleaseAsset;
    use crate::platform::{linux::Linux, macos::MacOs};
    use std::collections::HashSet;

    fn asset(name: &str) -> ReleaseAsset {
        ReleaseAsset {
            name: name.into(),
            browser_download_url: String::new(),
            size: 0,
        }
    }

    fn fixture_assets() -> Vec<ReleaseAsset> {
        vec![
            asset("SoH-Ackbar-Delta-Linux.zip"),
            asset("SoH-Ackbar-Delta-Mac.zip"),
            asset("SoH-Ackbar-Delta-Win64.zip"),
        ]
    }

    #[test]
    fn picks_mac_asset_on_macos() {
        let assets = fixture_assets();
        let picked = Soh.pick_asset(&assets, &MacOs).unwrap();
        assert_eq!(picked.name, "SoH-Ackbar-Delta-Mac.zip");
    }

    #[test]
    fn picks_linux_asset_on_linux() {
        let assets = fixture_assets();
        let picked = Soh.pick_asset(&assets, &Linux).unwrap();
        assert_eq!(picked.name, "SoH-Ackbar-Delta-Linux.zip");
    }

    #[test]
    fn returns_none_when_no_matching_asset() {
        let assets = vec![asset("SoH-Ackbar-Delta-Win64.zip")];
        assert!(Soh.pick_asset(&assets, &MacOs).is_none());
        assert!(Soh.pick_asset(&assets, &Linux).is_none());
    }

    #[test]
    fn slots_returns_oot_and_oot_mq() {
        let slots = Soh.slots();
        assert_eq!(slots.len(), 2);
        assert_eq!(slots[0].id, SLOT_OOT);
        assert_eq!(slots[0].symlink_filename, "oot.z64");
        assert_eq!(slots[0].display_name, "Ocarina of Time");
        assert_eq!(slots[1].id, SLOT_OOT_MQ);
        assert_eq!(slots[1].symlink_filename, "oot-mq.z64");
        assert_eq!(slots[1].display_name, "Ocarina of Time - Master Quest");
    }

    #[test]
    fn cached_asset_slot_ids_match_declared_slots() {
        let slot_ids: HashSet<&str> = Soh.slots().iter().map(|s| s.id).collect();
        for ca in Soh.cached_assets() {
            assert!(
                slot_ids.contains(ca.slot_id),
                "cached asset slot_id {:?} is not declared in slots()",
                ca.slot_id
            );
        }
    }
}
