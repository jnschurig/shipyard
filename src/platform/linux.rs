use std::path::{Path, PathBuf};

use anyhow::Result;

use super::{Platform, project_dirs};

pub struct Linux;

impl Platform for Linux {
    fn default_library_root(&self) -> PathBuf {
        project_dirs().data_dir().join("versions")
    }

    fn config_dir(&self) -> PathBuf {
        project_dirs().config_dir().to_path_buf()
    }

    fn cache_dir(&self) -> PathBuf {
        project_dirs().cache_dir().to_path_buf()
    }

    fn asset_keyword(&self) -> &'static str {
        "Linux"
    }
}

/// Install a Linux release whose layout is a flat zip with the appimage at
/// the root alongside companion data files (`gamecontrollerdb.txt`,
/// `assets/`, `config.yml`, etc.). Unzips into `dest` and chmods the named
/// appimage. The data files matter — extracting only the appimage strips
/// runtime resources and causes crashes (e.g. Starship's `AudioLoad_Init`).
pub fn install_appimage_release(archive: &Path, dest: &Path, appimage_name: &str) -> Result<()> {
    crate::library::extract::install_flat_zip(archive, dest, appimage_name)
}
