use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const CURRENT_SCHEMA_VERSION: u32 = 5;

pub const DEFAULT_VERSIONS_TO_SHOW: u32 = 10;
pub const MIN_VERSIONS_TO_SHOW: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LastLaunched {
    pub game_slug: String,
    pub tag: String,
}

/// Serializable snapshot of the most recently observed GitHub rate-limit
/// state. Persisted across restarts so the UI can render quota status before
/// any new request is made.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RateLimitSnapshot {
    pub remaining: Option<u32>,
    pub limit: Option<u32>,
    /// Unix timestamp (seconds) when the quota resets.
    pub reset_at_unix: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Config {
    pub schema_version: u32,

    #[serde(default)]
    pub library_root: Option<PathBuf>,

    #[serde(default)]
    pub install_overrides: HashMap<String, PathBuf>,

    /// Slot assignments per game: `slot_assignments[game_slug][slot_id] = rom filename`.
    /// The filename is relative to the ROM library root.
    #[serde(default)]
    pub slot_assignments: HashMap<String, HashMap<String, String>>,

    /// Maximum number of recent versions to show in the Library Version
    /// dropdown. Installed versions older than this window are still shown.
    #[serde(default = "default_versions_to_show")]
    pub versions_to_show: u32,

    /// (game_slug, tag) of the last successful launch. Used to pre-select the
    /// Library tab dropdowns on cold start.
    #[serde(default)]
    pub last_launched: Option<LastLaunched>,

    /// Most recently observed GitHub rate-limit headers. Persisted so the UI
    /// can render quota status before any new request runs.
    #[serde(default)]
    pub rate_limit_snapshot: Option<RateLimitSnapshot>,
}

fn default_versions_to_show() -> u32 {
    DEFAULT_VERSIONS_TO_SHOW
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            library_root: None,
            install_overrides: HashMap::new(),
            slot_assignments: HashMap::new(),
            versions_to_show: DEFAULT_VERSIONS_TO_SHOW,
            last_launched: None,
            rate_limit_snapshot: None,
        }
    }
}

impl Config {
    pub fn assignment_for(&self, game_slug: &str, slot_id: &str) -> Option<&str> {
        self.slot_assignments
            .get(game_slug)
            .and_then(|m| m.get(slot_id))
            .map(|s| s.as_str())
    }

    pub fn set_assignment(&mut self, game_slug: &str, slot_id: &str, filename: Option<String>) {
        match filename {
            Some(name) => {
                self.slot_assignments
                    .entry(game_slug.to_string())
                    .or_default()
                    .insert(slot_id.to_string(), name);
            }
            None => {
                if let Some(inner) = self.slot_assignments.get_mut(game_slug) {
                    inner.remove(slot_id);
                    if inner.is_empty() {
                        self.slot_assignments.remove(game_slug);
                    }
                }
            }
        }
    }

    /// Remove every assignment referencing `filename` across all games. Returns
    /// the number of assignments cleared.
    pub fn clear_assignments_referencing(&mut self, filename: &str) -> usize {
        let mut cleared = 0;
        let mut empty_games: Vec<String> = Vec::new();
        for (game, slots) in self.slot_assignments.iter_mut() {
            let before = slots.len();
            slots.retain(|_, fname| fname != filename);
            cleared += before - slots.len();
            if slots.is_empty() {
                empty_games.push(game.clone());
            }
        }
        for g in empty_games {
            self.slot_assignments.remove(&g);
        }
        cleared
    }
}
