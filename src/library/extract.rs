use std::fs;
use std::io;
use std::path::Path;
#[cfg(target_os = "macos")]
use std::path::PathBuf;

use anyhow::{Context, Result};
#[cfg(target_os = "macos")]
use anyhow::anyhow;

/// Internal helper shared by `platform::linux::install_appimage_release` and
/// `platform::macos::install_flat_binary_release`: unzip the archive directly
/// into `dest` and chmod `binary_name` (if present) to 0o755. Game extract
/// dispatchers should call the per-platform wrapper, not this directly.
pub(crate) fn install_flat_zip(archive: &Path, dest: &Path, binary_name: &str) -> Result<()> {
    fs::create_dir_all(dest).with_context(|| format!("create dest {}", dest.display()))?;
    unzip(archive, dest).with_context(|| format!("unzip {}", archive.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let bin = dest.join(binary_name);
        if bin.exists() {
            fs::set_permissions(&bin, fs::Permissions::from_mode(0o755))?;
        }
    }
    Ok(())
}

/// Unzip every entry into `dest`. Preserves relative paths, creates parents.
pub fn unzip(archive: &Path, dest: &Path) -> Result<()> {
    let file =
        fs::File::open(archive).with_context(|| format!("open archive {}", archive.display()))?;
    let mut zip =
        zip::ZipArchive::new(file).with_context(|| format!("read zip {}", archive.display()))?;
    fs::create_dir_all(dest).with_context(|| format!("create dest {}", dest.display()))?;

    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let Some(rel) = entry.enclosed_name() else {
            continue;
        };
        let out = dest.join(rel);
        if entry.is_dir() {
            fs::create_dir_all(&out)?;
            continue;
        }
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out_f =
            fs::File::create(&out).with_context(|| format!("create {}", out.display()))?;
        io::copy(&mut entry, &mut out_f)?;

        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&out, fs::Permissions::from_mode(mode))?;
        }
    }
    Ok(())
}

/// Find the first direct child of `dir` whose extension (case-insensitive) matches `ext`.
/// Used by macOS DMG/.app extraction; gated since Linux builds don't need it.
#[cfg(target_os = "macos")]
pub fn find_first_with_ext(dir: &Path, ext: &str) -> Result<PathBuf> {
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

/// Recursively copy `src` to `dst` using `cp -R`. Used for `.app` bundles on macOS.
#[cfg(unix)]
pub fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    use anyhow::bail;
    use std::process::Command;
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

/// macOS-only: mount a DMG read-only at a temporary mount point. The returned
/// guard detaches the DMG when dropped.
#[cfg(target_os = "macos")]
pub fn mount_dmg(dmg: &Path) -> Result<MountGuard> {
    use anyhow::bail;
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mount_dir = tempfile::tempdir().context("mktemp dmg mount dir")?.keep();
    let mut child = Command::new("hdiutil")
        .args([
            "attach",
            "-nobrowse",
            "-readonly",
            "-noverify",
            "-mountpoint",
        ])
        .arg(&mount_dir)
        .arg(dmg)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("spawn hdiutil")?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(b"Y\n");
    }
    let out = child.wait_with_output().context("wait hdiutil")?;
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

#[cfg(target_os = "macos")]
pub struct MountGuard {
    pub mount_point: PathBuf,
}

#[cfg(target_os = "macos")]
impl Drop for MountGuard {
    fn drop(&mut self) {
        use std::process::Command;
        let out = Command::new("hdiutil")
            .args(["detach", "-quiet"])
            .arg(&self.mount_point)
            .output();
        if let Err(e) = out {
            tracing::warn!(
                "hdiutil detach failed for {}: {e}",
                self.mount_point.display()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    fn make_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let f = fs::File::create(path).unwrap();
        let mut w = zip::ZipWriter::new(f);
        let opts: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (name, bytes) in entries {
            w.start_file(*name, opts).unwrap();
            w.write_all(bytes).unwrap();
        }
        w.finish().unwrap();
    }

    #[test]
    fn unzip_round_trip() {
        let dir = tempdir().unwrap();
        let archive = dir.path().join("a.zip");
        make_zip(
            &archive,
            &[
                ("readme.txt", b"hello"),
                ("inner/thing.bin", b"\x01\x02\x03"),
            ],
        );
        let dest = dir.path().join("out");
        unzip(&archive, &dest).unwrap();

        assert_eq!(fs::read(dest.join("readme.txt")).unwrap(), b"hello");
        assert_eq!(
            fs::read(dest.join("inner/thing.bin")).unwrap(),
            b"\x01\x02\x03"
        );
    }

}
