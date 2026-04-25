use std::fs;
use std::io;
use std::path::Path;

use anyhow::{Context, Result};

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
