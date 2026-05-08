use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Result;

use crate::github::ReleaseAsset;
use crate::platform::Platform;

pub mod ghostship;
pub mod soh;
pub mod spaghettikart;
pub mod starship;
pub mod twoship;

/// Game-declared ROM slot. Each game owns its own slot id namespace; ids are
/// only meaningful within a `(game_slug, slot_id)` pair.
pub struct SlotSpec {
    pub id: &'static str,
    pub display_name: &'static str,
    /// Filename Shipyard creates as a symlink in each install dir to point the
    /// game at the assigned ROM file in the ROM library.
    pub symlink_filename: &'static str,
}

/// Declares a cached-asset file associated with a slot. The `filenames` list is
/// ordered; scanning picks the first file that exists on disk. This lets games
/// transparently accept both the current format (e.g. `.o2r`) and a legacy one
/// (e.g. `.otr`) without version-sniffing the install.
pub struct CachedAssetSpec {
    pub slot_id: &'static str,
    pub filenames: &'static [&'static str],
}

pub trait Game: Send + Sync {
    fn slug(&self) -> &'static str;
    fn repo_slug(&self) -> &'static str;
    fn display_name(&self) -> &'static str;

    /// Sort key for the game picker. Defaults to `display_name`. Games whose
    /// display name doesn't sort sensibly (e.g. leading digit) override this.
    fn sort_name(&self) -> &'static str {
        self.display_name()
    }

    /// User-facing label for the underlying game whose ROMs the slots accept.
    /// Distinct from `display_name` (which names the launcher/port). For
    /// example, the SoH port's `display_name` is "Ship of Harkinian" while
    /// `rom_group_name` is "Ocarina of Time". Defaults to `display_name` for
    /// games where the two coincide.
    fn rom_group_name(&self) -> &'static str {
        self.display_name()
    }

    /// Where the game writes cached ROM archives (`.o2r` / `.otr`).
    fn data_dir(&self, install_dir: &Path, platform: &dyn Platform) -> PathBuf;

    /// ROM slots this game accepts. Each entry's `id` must be unique within the
    /// game and is the persistence key for slot assignments.
    fn slots(&self) -> &'static [SlotSpec];

    /// Slot ↔ cached-asset-filename mapping used by the cached-asset scanner
    /// for status display and clearing. `slot_id` values must match those in
    /// `slots()`.
    fn cached_assets(&self) -> &'static [CachedAssetSpec];

    /// Whether this game requires the ROM to be a real file copy in the install
    /// dir rather than a symlink. Default is symlink — every shipping game today
    /// (SoH, 2Ship, Ghostship) follows symlinks fine, since libultraship's
    /// resource resolver dereferences before reading.
    ///
    /// Override to `true` if a future port's loader rejects symlinks and demands
    /// a regular file. The copy path lives in `roms::wiring::reconcile`
    /// (atomic copy → rename, size-based skip-if-equal) and is exercised by
    /// tests in that module — so flipping this flag is a one-line change.
    fn requires_rom_copy(&self) -> bool {
        false
    }

    fn pick_asset<'a>(
        &self,
        assets: &'a [ReleaseAsset],
        platform: &dyn Platform,
    ) -> Option<&'a ReleaseAsset>;

    fn launch_command(&self, install_dir: &Path, platform: &dyn Platform) -> Command;

    /// Extract a downloaded release archive into `dest`. Each port packages
    /// releases differently (SoH wraps a DMG inside a zip; 2Ship ships a
    /// `.app` or nested AppImage directly), so this lives on `Game` rather
    /// than `Platform`.
    fn extract(&self, archive: &Path, dest: &Path, platform: &dyn Platform) -> Result<()>;
}

pub fn registry() -> &'static [&'static dyn Game] {
    &[
        &soh::Soh,
        &twoship::TwoShip,
        &ghostship::Ghostship,
        &starship::Starship,
        &spaghettikart::SpaghettiKart,
    ]
}
