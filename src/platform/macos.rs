use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, anyhow, bail};
use tracing::warn;

use super::{Platform, project_dirs};
use crate::library::extract::unzip;

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

    fn extract(&self, archive: &Path, dest: &Path) -> Result<()> {
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
}

fn find_first_with_ext(dir: &Path, ext: &str) -> Result<PathBuf> {
    for entry in fs::read_dir(dir)? {
        let e = entry?;
        let p = e.path();
        if p.extension()
            .and_then(|s| s.to_str())
            .map(str::to_ascii_lowercase)
            == Some(ext.to_ascii_lowercase())
        {
            return Ok(p);
        }
    }
    Err(anyhow!("no .{ext} file found in {}", dir.display()))
}

struct MountGuard {
    mount_point: PathBuf,
}

impl Drop for MountGuard {
    fn drop(&mut self) {
        let out = Command::new("hdiutil")
            .args(["detach", "-quiet"])
            .arg(&self.mount_point)
            .output();
        if let Err(e) = out {
            warn!(
                "hdiutil detach failed for {}: {e}",
                self.mount_point.display()
            );
        }
    }
}

fn mount_dmg(dmg: &Path) -> Result<MountGuard> {
    let mount_dir = tempfile::tempdir().context("mktemp dmg mount dir")?.keep();
    let out = Command::new("hdiutil")
        .args([
            "attach",
            "-nobrowse",
            "-readonly",
            "-noverify",
            "-mountpoint",
        ])
        .arg(&mount_dir)
        .arg(dmg)
        .output()
        .context("spawn hdiutil")?;
    if !out.status.success() {
        bail!(
            "hdiutil attach failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(MountGuard {
        mount_point: mount_dir,
    })
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    let out = Command::new("cp")
        .arg("-R")
        .arg(src)
        .arg(dst)
        .output()
        .context("spawn cp")?;
    if !out.status.success() {
        bail!("cp failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    Ok(())
}
