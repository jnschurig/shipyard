use std::io;
use std::path::Path;
use std::process::Child;

use tracing::debug;

use crate::config::Config;
use crate::games::Game;
use crate::library::InstalledVersion;
use crate::platform::Platform;
use crate::roms::wiring;

/// Handle to a spawned game process. Kept in app state keyed by tag so the
/// install state machine can block uninstall/relaunch while a version is
/// running. Polls child status lazily — a crashed child clears `is_running`
/// the next time it's queried.
#[derive(Debug)]
pub struct LaunchHandle {
    tag: String,
    pid: u32,
    child: Option<Child>,
}

impl LaunchHandle {
    pub fn tag(&self) -> &str {
        &self.tag
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Non-blocking status check. Returns `true` if the child is still alive.
    /// Reaps the child on first observation of exit so we don't leave a zombie.
    pub fn is_running(&mut self) -> bool {
        let Some(child) = self.child.as_mut() else {
            return false;
        };
        match child.try_wait() {
            Ok(Some(status)) => {
                debug!(tag = %self.tag, pid = self.pid, ?status, "launched process exited");
                self.child = None;
                false
            }
            Ok(None) => true,
            Err(e) => {
                debug!(tag = %self.tag, pid = self.pid, error = %e, "try_wait failed; assuming dead");
                self.child = None;
                false
            }
        }
    }
}

/// Spawn the game binary with no extra arguments. Earlier iterations of
/// Shipyard injected a ROM path so SoH could generate `oot.o2r` headlessly,
/// but argv handling differs between SoH versions (9.2.0 segfaults on a
/// positional ROM arg) so the launcher stays out of it: SoH brings up its
/// own ROM picker on first run, and skips it on subsequent launches once
/// `oot.o2r` exists in the install dir.
pub fn launch(
    installed: &InstalledVersion,
    game: &dyn Game,
    platform: &dyn Platform,
    config: &Config,
    rom_library_root: &Path,
) -> io::Result<LaunchHandle> {
    wiring::reconcile(&installed.path, game, platform, config, rom_library_root)?;

    let mut cmd = game.launch_command(&installed.path, platform);
    cmd.current_dir(&installed.path);

    #[cfg(unix)]
    unsafe {
        use std::os::unix::process::CommandExt;
        cmd.pre_exec(|| {
            // Detach into a new session so the child isn't killed when Shipyard exits.
            if libc::setsid() == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }

    let child = cmd.spawn()?;
    let pid = child.id();
    debug!(tag = %installed.tag, pid, "spawned game process");

    Ok(LaunchHandle {
        tag: installed.tag.clone(),
        pid,
        child: Some(child),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::ReleaseAsset;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    struct FakePlatform;
    impl Platform for FakePlatform {
        fn default_library_root(&self) -> PathBuf {
            PathBuf::from("/tmp")
        }
        fn config_dir(&self) -> PathBuf {
            PathBuf::from("/tmp")
        }
        fn cache_dir(&self) -> PathBuf {
            PathBuf::from("/tmp")
        }
        fn asset_keyword(&self) -> &'static str {
            "Mac"
        }
    }

    /// Game whose launch command runs a caller-supplied binary, so tests can
    /// drive a real spawn → exit sequence.
    struct ScriptGame {
        bin: PathBuf,
    }
    impl Game for ScriptGame {
        fn slug(&self) -> &'static str {
            "fake"
        }
        fn repo_slug(&self) -> &'static str {
            "x/y"
        }
        fn display_name(&self) -> &'static str {
            "Fake"
        }
        fn data_dir(&self, install_dir: &Path, _: &dyn Platform) -> PathBuf {
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
            a: &'a [ReleaseAsset],
            _: &dyn Platform,
        ) -> Option<&'a ReleaseAsset> {
            a.first()
        }
        fn launch_command(&self, _: &Path, _: &dyn Platform) -> Command {
            Command::new(&self.bin)
        }
        fn extract(&self, _: &Path, _: &Path, _: &dyn Platform) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn launch_spawns_and_is_running_clears_after_exit() {
        let dir = tempfile::tempdir().unwrap();
        let installed = InstalledVersion {
            tag: "t1".into(),
            game_slug: "fake".into(),
            path: dir.path().to_path_buf(),
        };
        let game = ScriptGame {
            bin: PathBuf::from("/usr/bin/true"),
        };

        let config = Config::default();
        let lib_root = dir.path().join("rom-library");
        let mut handle = launch(&installed, &game, &FakePlatform, &config, &lib_root).unwrap();
        assert!(handle.pid() > 0);

        for _ in 0..50 {
            if !handle.is_running() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        panic!("child never exited");
    }
}
