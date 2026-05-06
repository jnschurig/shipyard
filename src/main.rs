use std::sync::Arc;

use shipyard::{app, config, games, github, platform, roms};

fn main() -> iced::Result {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tracing::info!("shipyard {} starting", env!("CARGO_PKG_VERSION"));

    iced::application("Shipyard", app::App::update, app::App::view).run_with(|| {
        let platform = platform::current();
        let game = games::registry()[0];

        let mut loaded = config::Config::load().expect("load config");
        let library_root = loaded
            .config
            .library_root
            .clone()
            .unwrap_or_else(|| platform.default_library_root());
        let download_dir = platform.cache_dir().join("downloads");
        let cache_path = platform.cache_dir().join("etags.json");

        let client = Arc::new(github::Client::new(cache_path).expect("init github client"));

        let mut diagnostics = Vec::new();
        if let Some(d) = loaded.diagnostic.take() {
            diagnostics.push(d);
        }

        if let Some(pending) = loaded.pending_migration.take() {
            let rom_lib_root = roms::library::library_root(platform);
            let mut migration_diags =
                roms::library::apply_pending_migration(&rom_lib_root, &mut loaded.config, pending);
            diagnostics.append(&mut migration_diags);
            if let Err(e) = loaded.config.save_to(&loaded.path) {
                tracing::warn!(error = %e, "failed to persist migrated config");
            }
        }

        let rom_library_root = roms::library::library_root(platform);

        app::App::new(app::AppDeps {
            config: loaded.config,
            config_path: loaded.path,
            library_root,
            rom_library_root,
            download_dir,
            game,
            platform,
            client,
            startup_diagnostics: diagnostics,
        })
    })
}
