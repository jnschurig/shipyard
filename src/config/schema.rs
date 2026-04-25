use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const CURRENT_SCHEMA_VERSION: u32 = 4;

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
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            library_root: None,
            install_overrides: HashMap::new(),
            slot_assignments: HashMap::new(),
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
}
