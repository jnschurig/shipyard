pub mod extract;
pub mod manifest;

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use chrono::Utc;
use tokio::sync::mpsc;
use tracing::{debug, warn};

use crate::config::Config;
use crate::games::Game;
use crate::github::{self, Release};
use crate::paths::expand_path;
use crate::platform::Platform;
use manifest::InstallManifest;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledVersion {
    pub tag: String,
    pub game_slug: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone)]
pub enum InstallProgress {
    Starting,
    Downloading { downloaded: u64, total: Option<u64> },
    Extracting,
    Finalizing,
    Done(InstalledVersion),
}

/// Walk `library_root` (one or two levels deep) plus every path in
/// `config.install_overrides`, read manifests, return every directory that has
/// a valid `.shipyard-install.json`. Supports both the legacy flat layout
/// (`versions/<tag>/`) and the partitioned layout (`versions/<game_slug>/<tag>/`).
pub fn scan(library_root: &Path, config: &Config) -> Vec<InstalledVersion> {
    let mut found = Vec::new();
    let library_root = expand_path(library_root);

    if library_root.is_dir()
        && let Ok(read) = fs::read_dir(&library_root)
    {
        for entry in read.flatten() {
            let p = entry.path();
            if !p.is_dir() {
                continue;
            }
            if let Some(v) = read_version(&p) {
                found.push(v);
                continue;
            }
            // No manifest at this level — try one level deeper for the
            // partitioned layout (`<library_root>/<game_slug>/<tag>/`).
            if let Ok(inner) = fs::read_dir(&p) {
                for sub in inner.flatten() {
                    let sp = sub.path();
                    if sp.is_dir()
                        && let Some(v) = read_version(&sp)
                    {
                        found.push(v);
                    }
                }
            }
        }
    }

    for (tag, override_path) in &config.install_overrides {
        let expanded = expand_path(override_path);
        if let Some(v) = read_version(&expanded) {
            if !found.iter().any(|x| x.path == v.path) {
                found.push(v);
            }
        } else {
            debug!(
                "override for tag {tag} at {} has no manifest",
                expanded.display()
            );
        }
    }

    found
}

fn read_version(dir: &Path) -> Option<InstalledVersion> {
    match InstallManifest::read(dir) {
        Ok(Some(m)) => Some(InstalledVersion {
            tag: m.tag,
            game_slug: m.game_slug,
            path: dir.to_path_buf(),
        }),
        Ok(None) => None,
        Err(e) => {
            warn!("failed to read manifest in {}: {e}", dir.display());
            None
        }
    }
}

pub struct InstallRequest<'a> {
    pub game: &'a dyn Game,
    pub release: &'a Release,
    pub platform: &'a dyn Platform,
    pub library_root: &'a Path,
    pub destination_override: Option<PathBuf>,
    pub download_dir: &'a Path,
}

/// Crash-safe install: download → extract to `<dest>.partial/` → write manifest → rename.
/// On any error inside the pipeline, the `.partial` dir is removed.
///
/// Returns `(InstalledVersion, Option<PathBuf> override_to_persist)`. When an override
/// was used, the caller is responsible for writing it into `Config::install_overrides`
/// and saving the config.
pub async fn install(
    client: &github::Client,
    req: InstallRequest<'_>,
    progress: Option<mpsc::Sender<InstallProgress>>,
) -> Result<(InstalledVersion, Option<PathBuf>)> {
    let InstallRequest {
        game,
        release,
        platform,
        library_root,
        destination_override,
        download_dir,
    } = req;

    let send = |p: InstallProgress| {
        let progress = progress.clone();
        async move {
            if let Some(tx) = progress {
                let _ = tx.send(p).await;
            }
        }
    };

    send(InstallProgress::Starting).await;

    let asset = game
        .pick_asset(&release.assets, platform)
        .ok_or_else(|| anyhow!("no asset for {} on this platform", release.tag_name))?;

    let dest_final = destination_override.clone().map(|p| expand_path(&p)).unwrap_or_else(|| {
        expand_path(library_root)
            .join(game.slug())
            .join(&release.tag_name)
    });
    let dest_partial = partial_path(&dest_final);

    if dest_final.exists() {
        return Err(anyhow!(
            "destination already exists: {}",
            dest_final.display()
        ));
    }
    if dest_partial.exists() {
        fs::remove_dir_all(&dest_partial).ok();
    }

    fs::create_dir_all(download_dir)
        .with_context(|| format!("create download dir {}", download_dir.display()))?;
    let archive_path = download_dir.join(&asset.name);

    // Download with progress forwarded into InstallProgress::Downloading.
    let (dl_tx, mut dl_rx) = mpsc::channel::<github::DownloadProgress>(32);
    let progress_clone = progress.clone();
    let forwarder = tokio::spawn(async move {
        while let Some(p) = dl_rx.recv().await {
            if let Some(tx) = &progress_clone {
                let _ = tx
                    .send(InstallProgress::Downloading {
                        downloaded: p.downloaded,
                        total: p.total,
                    })
                    .await;
            }
        }
    });

    let dl_result = client
        .download_asset(&asset.browser_download_url, &archive_path, Some(dl_tx))
        .await;
    forwarder.abort();
    dl_result.context("download asset")?;

    send(InstallProgress::Extracting).await;

    let extract_result = (|| -> Result<()> {
        fs::create_dir_all(&dest_partial)
            .with_context(|| format!("create partial {}", dest_partial.display()))?;
        game.extract(&archive_path, &dest_partial, platform)?;
        let manifest = InstallManifest {
            tag: release.tag_name.clone(),
            game_slug: game.slug().to_string(),
            installed_at: Utc::now(),
            archive_sha256: None,
        };
        manifest.write(&dest_partial)?;
        Ok(())
    })();

    if let Err(e) = extract_result {
        let _ = fs::remove_dir_all(&dest_partial);
        return Err(e);
    }

    send(InstallProgress::Finalizing).await;

    fs::rename(&dest_partial, &dest_final).with_context(|| {
        format!(
            "rename {} -> {}",
            dest_partial.display(),
            dest_final.display()
        )
    })?;

    // Archive is no longer needed; best-effort cleanup.
    let _ = fs::remove_file(&archive_path);

    let installed = InstalledVersion {
        tag: release.tag_name.clone(),
        game_slug: game.slug().to_string(),
        path: dest_final.clone(),
    };
    send(InstallProgress::Done(installed.clone())).await;

    Ok((installed, destination_override))
}

/// Remove an installed version from disk. Caller is responsible for clearing any
/// matching `Config::install_overrides` entry and saving config.
pub fn uninstall(installed: &InstalledVersion) -> Result<()> {
    if installed.path.exists() {
        fs::remove_dir_all(&installed.path)
            .with_context(|| format!("remove {}", installed.path.display()))?;
    }
    Ok(())
}

fn partial_path(final_path: &Path) -> PathBuf {
    let file_name = final_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("install");
    let parent = final_path.parent().unwrap_or_else(|| Path::new("."));
    parent.join(format!("{file_name}.partial"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use tempfile::tempdir;

    fn write_manifest(dir: &Path, tag: &str, slug: &str) {
        fs::create_dir_all(dir).unwrap();
        InstallManifest {
            tag: tag.into(),
            game_slug: slug.into(),
            installed_at: Utc::now(),
            archive_sha256: None,
        }
        .write(dir)
        .unwrap();
    }

    #[test]
    fn scan_finds_dirs_with_manifest() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        write_manifest(&root.join("9.2.3"), "9.2.3", "soh");
        write_manifest(&root.join("9.2.2"), "9.2.2", "soh");
        fs::create_dir_all(root.join("stray")).unwrap(); // no manifest

        let cfg = Config::default();
        let found = scan(root, &cfg);
        assert_eq!(found.len(), 2);
        let tags: std::collections::HashSet<_> = found.iter().map(|v| v.tag.as_str()).collect();
        assert!(tags.contains("9.2.3"));
        assert!(tags.contains("9.2.2"));
    }

    #[test]
    fn scan_includes_overrides_outside_library_root() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("lib");
        fs::create_dir_all(&root).unwrap();
        let over = dir.path().join("elsewhere/custom");
        write_manifest(&over, "custom-tag", "soh");

        let mut cfg = Config::default();
        cfg.install_overrides
            .insert("custom-tag".into(), over.clone());

        let found = scan(&root, &cfg);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].tag, "custom-tag");
        assert_eq!(found[0].path, over);
    }

    #[test]
    fn scan_deduplicates_overrides_inside_library_root() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let inside = root.join("9.2.3");
        write_manifest(&inside, "9.2.3", "soh");

        let mut cfg = Config::default();
        cfg.install_overrides.insert("9.2.3".into(), inside.clone());

        let found = scan(root, &cfg);
        assert_eq!(found.len(), 1);
    }

    use crate::games::Game;
    use crate::github::{Release, ReleaseAsset};
    use std::io::Write;
    use std::process::Command;
    use std::sync::atomic::{AtomicBool, Ordering};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    struct FakePlatform;
    impl crate::platform::Platform for FakePlatform {
        fn default_library_root(&self) -> PathBuf {
            PathBuf::from("/tmp/fake")
        }
        fn config_dir(&self) -> PathBuf {
            PathBuf::from("/tmp/fake/cfg")
        }
        fn cache_dir(&self) -> PathBuf {
            PathBuf::from("/tmp/fake/cache")
        }
        fn asset_keyword(&self) -> &'static str {
            "Mac"
        }
    }

    struct FakeGame {
        fail_extract: AtomicBool,
    }
    impl FakeGame {
        fn new() -> Self {
            Self {
                fail_extract: AtomicBool::new(false),
            }
        }
    }
    impl Game for FakeGame {
        fn slug(&self) -> &'static str {
            "soh"
        }
        fn repo_slug(&self) -> &'static str {
            "Fake/Repo"
        }
        fn display_name(&self) -> &'static str {
            "Fake"
        }
        fn data_dir(&self, install_dir: &Path, _: &dyn crate::platform::Platform) -> PathBuf {
            install_dir.to_path_buf()
        }
        fn slots(&self) -> &'static [crate::games::SlotSpec] {
            &[]
        }
        fn cached_assets(&self) -> &'static [crate::games::CachedAssetSpec] {
            &[]
        }
        fn pick_asset<'a>(
            &self,
            assets: &'a [ReleaseAsset],
            _: &dyn crate::platform::Platform,
        ) -> Option<&'a ReleaseAsset> {
            assets.first()
        }
        fn launch_command(&self, _: &Path, _: &dyn crate::platform::Platform) -> Command {
            Command::new("true")
        }
        fn extract(
            &self,
            _archive: &Path,
            dest: &Path,
            _: &dyn crate::platform::Platform,
        ) -> Result<()> {
            if self.fail_extract.load(Ordering::SeqCst) {
                return Err(anyhow!("synthetic extraction failure"));
            }
            fs::create_dir_all(dest)?;
            fs::write(dest.join("payload.txt"), b"ok")?;
            Ok(())
        }
    }

    async fn setup_server_with_asset(body: &[u8]) -> (MockServer, String) {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/dl/asset.zip"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body.to_vec()))
            .mount(&server)
            .await;
        let url = format!("{}/dl/asset.zip", server.uri());
        (server, url)
    }

    fn fixture_release(asset_url: &str) -> Release {
        Release {
            tag_name: "9.2.3".into(),
            name: Some("fake".into()),
            published_at: None,
            assets: vec![ReleaseAsset {
                name: "asset.zip".into(),
                browser_download_url: asset_url.into(),
                size: 0,
            }],
        }
    }

    #[tokio::test]
    async fn install_happy_path_writes_manifest_and_scan_finds_it() {
        let dir = tempdir().unwrap();
        let lib = dir.path().join("library");
        let dl = dir.path().join("downloads");
        let cache = dir.path().join("etags.json");

        let (_server, url) = setup_server_with_asset(b"archive-body").await;
        let client = github::Client::with_base(cache, _server.uri()).unwrap();
        let release = fixture_release(&url);
        let plat = FakePlatform;
        let game = FakeGame::new();

        let (installed, override_out) = install(
            &client,
            InstallRequest {
                game: &game,
                release: &release,
                platform: &plat,
                library_root: &lib,
                destination_override: None,
                download_dir: &dl,
            },
            None,
        )
        .await
        .unwrap();

        assert_eq!(installed.tag, "9.2.3");
        assert_eq!(installed.path, lib.join("soh").join("9.2.3"));
        assert!(installed.path.join("payload.txt").exists());
        assert!(installed.path.join(manifest::MANIFEST_FILE).exists());
        assert!(override_out.is_none());

        // partial directory should be gone
        assert!(!lib.join("soh").join("9.2.3.partial").exists());

        // scan finds it
        let cfg = Config::default();
        let found = scan(&lib, &cfg);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].tag, "9.2.3");
    }

    #[tokio::test]
    async fn install_extraction_failure_leaves_no_partial() {
        let dir = tempdir().unwrap();
        let lib = dir.path().join("library");
        let dl = dir.path().join("downloads");
        let cache = dir.path().join("etags.json");

        let (_server, url) = setup_server_with_asset(b"archive-body").await;
        let client = github::Client::with_base(cache, _server.uri()).unwrap();
        let release = fixture_release(&url);
        let plat = FakePlatform;
        let game = FakeGame::new();
        game.fail_extract.store(true, Ordering::SeqCst);

        let res = install(
            &client,
            InstallRequest {
                game: &game,
                release: &release,
                platform: &plat,
                library_root: &lib,
                destination_override: None,
                download_dir: &dl,
            },
            None,
        )
        .await;
        assert!(res.is_err());

        assert!(!lib.join("soh").join("9.2.3").exists());
        assert!(!lib.join("soh").join("9.2.3.partial").exists());
    }

    #[tokio::test]
    async fn install_respects_destination_override() {
        let dir = tempdir().unwrap();
        let lib = dir.path().join("library");
        let over = dir.path().join("custom/loc");
        let dl = dir.path().join("downloads");
        let cache = dir.path().join("etags.json");

        let (_server, url) = setup_server_with_asset(b"archive-body").await;
        let client = github::Client::with_base(cache, _server.uri()).unwrap();
        let release = fixture_release(&url);
        let plat = FakePlatform;
        let game = FakeGame::new();

        let (installed, override_out) = install(
            &client,
            InstallRequest {
                game: &game,
                release: &release,
                platform: &plat,
                library_root: &lib,
                destination_override: Some(over.clone()),
                download_dir: &dl,
            },
            None,
        )
        .await
        .unwrap();

        assert_eq!(installed.path, over);
        assert_eq!(override_out, Some(over));
        let _ = Write::flush(&mut std::io::stdout());
    }

    #[test]
    fn uninstall_removes_directory_and_is_idempotent() {
        let dir = tempdir().unwrap();
        let install_path = dir.path().join("9.2.3");
        write_manifest(&install_path, "9.2.3", "soh");

        let v = InstalledVersion {
            tag: "9.2.3".into(),
            game_slug: "soh".into(),
            path: install_path.clone(),
        };
        uninstall(&v).unwrap();
        assert!(!install_path.exists());
        // second call is a no-op
        uninstall(&v).unwrap();
    }
}
