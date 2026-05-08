use std::sync::Arc;

use shipyard::{app, config, games, github, platform, roms};

const ICON_SVG: &[u8] = include_bytes!("../assets/icon.svg");
const ICON_RENDER_SIZE: u32 = 256;

fn load_window_icon() -> Option<iced::window::Icon> {
    let tree = usvg::Tree::from_data(ICON_SVG, &usvg::Options::default()).ok()?;
    let svg_size = tree.size();
    let scale = ICON_RENDER_SIZE as f32 / svg_size.width().max(svg_size.height());
    let mut pixmap = tiny_skia::Pixmap::new(ICON_RENDER_SIZE, ICON_RENDER_SIZE)?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    iced::window::icon::from_rgba(pixmap.take(), ICON_RENDER_SIZE, ICON_RENDER_SIZE).ok()
}

#[cfg(target_os = "linux")]
fn log_linux_display_backend() {
    let session = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
    let wayland_display = std::env::var("WAYLAND_DISPLAY").ok();
    let x_display = std::env::var("DISPLAY").ok();
    // winit 0.30 picks Wayland when WAYLAND_DISPLAY is set, else falls back to X11.
    let expected = if wayland_display.is_some() {
        "wayland"
    } else if x_display.is_some() {
        "x11"
    } else {
        "none"
    };
    tracing::info!(
        xdg_session_type = %session,
        wayland_display = ?wayland_display,
        display = ?x_display,
        expected_backend = expected,
        "linux display backend",
    );
    if session == "wayland" && wayland_display.is_none() {
        tracing::warn!(
            "XDG_SESSION_TYPE=wayland but WAYLAND_DISPLAY is unset; winit will use X11 (XWayland)"
        );
    }
}

fn main() -> iced::Result {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tracing::info!("shipyard {} starting", env!("CARGO_PKG_VERSION"));

    #[cfg(target_os = "linux")]
    log_linux_display_backend();

    let window_settings = iced::window::Settings {
        icon: load_window_icon(),
        ..Default::default()
    };

    iced::application("Shipyard", app::App::update, app::App::view)
        .window(window_settings)
        .theme(app::App::theme)
        .font(iced_fonts::BOOTSTRAP_FONT_BYTES)
        .run_with(|| {
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
                let mut migration_diags = roms::library::apply_pending_migration(
                    &rom_lib_root,
                    &mut loaded.config,
                    pending,
                );
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
