use std::path::PathBuf;

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
