use std::path::PathBuf;

pub mod linux;
pub mod macos;

pub trait Platform: Send + Sync {
    fn default_library_root(&self) -> PathBuf;
    fn config_dir(&self) -> PathBuf;
    fn cache_dir(&self) -> PathBuf;
    fn asset_keyword(&self) -> &'static str;
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

pub(crate) fn project_dirs() -> directories::ProjectDirs {
    directories::ProjectDirs::from("", "", "Shipyard")
        .expect("no home directory available for Shipyard")
}
