use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};

use super::{Platform, project_dirs};
use crate::library::extract::unzip;

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

    fn extract(&self, archive: &Path, dest: &Path) -> Result<()> {
        let scratch = tempfile::tempdir().context("mktemp scratch dir")?;
        unzip(archive, scratch.path()).context("unzip outer wrapper")?;

        let appimage = find_appimage(scratch.path())?;
        fs::create_dir_all(dest).with_context(|| format!("create dest {}", dest.display()))?;
        let target = dest.join(appimage.file_name().unwrap());
        fs::copy(&appimage, &target).with_context(|| format!("copy to {}", target.display()))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&target, fs::Permissions::from_mode(0o755))?;
        }
        Ok(())
    }
}

fn find_appimage(dir: &Path) -> Result<PathBuf> {
    for entry in fs::read_dir(dir)? {
        let e = entry?;
        let p = e.path();
        let name = p
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if name.ends_with(".appimage") {
            return Ok(p);
        }
    }
    Err(anyhow!("no .appimage file found in {}", dir.display()))
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
    fn extract_appimage_zip_on_unix() {
        let dir = tempdir().unwrap();
        let archive = dir.path().join("release.zip");
        make_zip(
            &archive,
            &[
                ("readme.txt", b"hi"),
                ("soh.appimage", b"\x7fELF-fake-appimage-body"),
            ],
        );
        let dest = dir.path().join("install");
        Linux.extract(&archive, &dest).unwrap();

        let target = dest.join("soh.appimage");
        assert!(target.exists());
        assert_eq!(fs::read(&target).unwrap(), b"\x7fELF-fake-appimage-body");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&target).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o755);
        }
    }
}
