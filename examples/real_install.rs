//! Runtime validation: install + optional launch against the real
//! HarbourMasters/Shipwright release on the current platform, exercising the
//! managed ROM-library flow (copy ROM into library → assign to slot → launch
//! with launch-time symlink reconciliation).
//!
//! Usage:
//!   cargo run --example real_install -- [--launch] [--mq] <oot_rom_path>

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use shipyard::config::Config;
use shipyard::games::soh::{SLOT_OOT, SLOT_OOT_MQ};
use shipyard::roms::library;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let do_launch = args.iter().any(|a| a == "--launch");
    let is_mq = args.iter().any(|a| a == "--mq");
    args.retain(|a| a != "--launch" && a != "--mq");
    let rom: PathBuf = args
        .pop()
        .map(PathBuf::from)
        .expect("pass path to an OoT .z64/.n64/.v64 ROM as the last arg");

    let workdir = tempfile::tempdir()?;
    let library_root = workdir.path().join("library");
    let rom_library_root = workdir.path().join("rom_library");
    let download_dir = workdir.path().join("downloads");
    let cache_dir = workdir.path().join("cache");
    let cache_path = cache_dir.join("etags.json");
    std::fs::create_dir_all(&library_root)?;
    std::fs::create_dir_all(&rom_library_root)?;
    std::fs::create_dir_all(&download_dir)?;
    std::fs::create_dir_all(&cache_dir)?;

    let platform = shipyard::platform::current();
    let game = shipyard::games::registry()[0];

    tracing::info!("using rom_library_root={}", rom_library_root.display());

    // --- ROM library import -------------------------------------------------
    let entry = library::import(&rom_library_root, &rom)
        .map_err(|e| anyhow::anyhow!("import {}: {e}", rom.display()))?;
    tracing::info!("ROM imported into library: {}", entry.filename);

    // --- Slot assignment ----------------------------------------------------
    let mut config = Config::default();
    let slot_id = if is_mq { SLOT_OOT_MQ } else { SLOT_OOT };
    config.set_assignment(game.slug(), slot_id, Some(entry.filename.clone()));

    // --- GitHub release install ---------------------------------------------
    tracing::info!("fetching {} releases", game.repo_slug());
    let client = Arc::new(shipyard::github::Client::new(cache_path)?);
    let (releases, rl) = client.list_releases(game.repo_slug()).await?;
    tracing::info!(
        "{} releases; rate limit remaining={:?} limit={:?}",
        releases.len(),
        rl.remaining,
        rl.limit
    );
    let release = releases
        .iter()
        .find(|r| !r.tag_name.to_ascii_lowercase().contains("pre"))
        .or_else(|| releases.first())
        .expect("no releases");
    tracing::info!("picking release tag={}", release.tag_name);

    let asset = game
        .pick_asset(&release.assets, platform)
        .expect("no asset matched this platform");
    tracing::info!(
        "asset: {} ({} bytes) — asset_keyword={}",
        asset.name,
        asset.size,
        platform.asset_keyword()
    );

    tracing::info!("installing (this downloads + extracts)…");
    let (installed, _) = shipyard::library::install(
        &client,
        shipyard::library::InstallRequest {
            game,
            release,
            platform,
            library_root: &library_root,
            destination_override: None,
            download_dir: &download_dir,
        },
        None,
    )
    .await?;
    tracing::info!("installed at {}", installed.path.display());

    let cmd = game.launch_command(&installed.path, platform);
    let program = cmd.get_program();
    let bin = PathBuf::from(program);
    anyhow::ensure!(
        bin.exists(),
        "expected launch binary missing: {}",
        bin.display()
    );

    // --- Launch (with symlink reconciliation) ------------------------------
    if do_launch {
        tracing::info!("launching {} via launcher (slot={slot_id})", installed.tag);
        let mut handle =
            shipyard::launcher::launch(&installed, game, platform, &config, &rom_library_root)?;
        tracing::info!("spawned pid={}", handle.pid());
        tracing::info!(
            "waiting 8s to confirm process stays alive, then exiting (GUI stays running)…"
        );
        for _ in 0..40 {
            if !handle.is_running() {
                anyhow::bail!("child exited prematurely");
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        tracing::info!("process alive at exit — close the SoH window manually when done.");

        let cached =
            shipyard::roms::cached_assets::scan_cached_assets(game, &installed.path, platform);
        for c in &cached {
            tracing::info!("cached asset for {}: {:?}", c.slot_id, c.status);
        }

        let _ = workdir.keep();
    } else {
        tracing::info!("skipping launch (pass --launch to actually spawn SoH).");
    }

    Ok(())
}
