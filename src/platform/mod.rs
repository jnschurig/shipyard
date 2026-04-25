use std::path::{Path, PathBuf};

use anyhow::Result;

pub mod linux;
pub mod macos;

pub trait Platform: Send + Sync {
    fn default_library_root(&self) -> PathBuf;
    fn config_dir(&self) -> PathBuf;
    fn cache_dir(&self) -> PathBuf;
    fn asset_keyword(&self) -> &'static str;
    fn extract(&self, archive: &Path, dest: &Path) -> Result<()>;
}

#[cfg(target_os = "macos")]
pub fn current() -> &'static dyn Platform {
    &macos::MacOs
}

#[cfg(target_os = "linux")]
pub fn current() -> &'static dyn Platform {
    &linux::Linux
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn current() -> &'static dyn Platform {
    compile_error!("unsupported target OS");
}

fn project_dirs() -> directories::ProjectDirs {
    directories::ProjectDirs::from("", "", "Shipyard")
        .expect("no home directory available for Shipyard")
}
