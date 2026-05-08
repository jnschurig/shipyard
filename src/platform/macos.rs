use std::path::{Path, PathBuf};

use anyhow::Result;

use super::{Platform, project_dirs};

pub struct MacOs;

impl Platform for MacOs {
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
        "Mac"
    }
}

/// Install a macOS release whose layout is `.app`-bundle-inside-DMG-inside-zip
/// (SoH / 2Ship / Ghostship pattern). Mounts the DMG and copies the `.app`
/// directory into `dest`. macOS-only; on other targets returns an error so
/// game extract dispatchers stay platform-agnostic at the call site.
#[cfg(target_os = "macos")]
pub fn install_app_in_dmg_release(archive: &Path, dest: &Path) -> Result<()> {
    use anyhow::Context;
    use std::fs;

    use crate::library::extract::{copy_dir_recursive, find_first_with_ext, mount_dmg, unzip};

    let scratch = tempfile::tempdir().context("mktemp scratch dir")?;
    unzip(archive, scratch.path()).context("unzip outer wrapper")?;

    let dmg = find_first_with_ext(scratch.path(), "dmg")?;
    let mount = mount_dmg(&dmg)?;

    let app = find_first_with_ext(&mount.mount_point, "app")?;
    fs::create_dir_all(dest).with_context(|| format!("create dest {}", dest.display()))?;
    let target = dest.join(app.file_name().unwrap());
    copy_dir_recursive(&app, &target)
        .with_context(|| format!("copy {} -> {}", app.display(), target.display()))?;
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn install_app_in_dmg_release(_archive: &Path, _dest: &Path) -> Result<()> {
    Err(anyhow::anyhow!(
        "DMG-based macOS installs are only supported when running on macOS"
    ))
}
