use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use iced::widget::{column, container, opaque, row, stack, text};
use iced::{Element, Length, Task};

use chrono::TimeZone;

use crate::config::schema::{MIN_VERSIONS_TO_SHOW, RateLimitSnapshot};
use crate::config::{Config, Diagnostic};
use crate::games::{self as games_mod, Game};
use crate::github::{self, RateLimitStatus, Release};
use crate::launcher::{self, LaunchHandle};
use crate::library::{self, InstallRequest, InstalledVersion};
use crate::platform::Platform;
use crate::roms::cached_assets;
use crate::roms::library::{self as rom_library, RomEntry};
use crate::ui;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallState {
    Idle,
    Installing,
    Installed,
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Library,
    Roms,
    Mods,
    Settings,
}

#[derive(Debug, Clone)]
pub enum Banner {
    RateLimited(String),
    Info(String),
    Error(String),
}

#[derive(Debug, Clone)]
pub enum Message {
    TabSelected(Tab),
    ReleasesLoaded {
        game_slug: String,
        result: Result<(Vec<Release>, RateLimitStatus), String>,
    },
    InstallClicked {
        game_slug: String,
        tag: String,
    },
    InstallProgress(String, Option<u8>),
    InstallFinished(String, Result<InstalledVersion, String>),
    UninstallClicked(String),
    LaunchClicked(String),
    LibraryRootInputChanged(String),
    SaveSettings,

    ClearCachedAssetsClicked(String /* tag */),
    ClearCachedAssetsConfirm(String /* tag */),
    ClearCachedAssetsCancel,

    ImportRomClicked,
    RomImported(Result<RomEntry, String>),
    DeleteRomClicked(String),
    DeleteRomConfirm(String),
    DeleteRomCancel,
    AssignSlotChanged {
        game_slug: String,
        slot_id: String,
        filename: Option<String>,
    },

    VersionSelected {
        game_slug: String,
        tag: String,
    },
    PrimaryActionClicked(String /* game_slug */),
    ToggleGearMenu(String /* game_slug */),
    DismissPopovers,
    ManualRefreshClicked(String /* game_slug */),
    UninstallSelectedClicked(String /* game_slug */),
    ClearCacheSelectedClicked(String /* game_slug */),
    ToggleImportedRomsExpander,
    VersionsToShowInputChanged(String),
    VersionsToShowSubmit,
}

#[derive(Debug, Clone)]
pub enum Modal {
    Closed,
    ClearCachedConfirm {
        tag: String,
        game_slug: String,
        planned: Vec<crate::roms::cached_assets::PlannedClear>,
    },
    DeleteRomConfirm {
        filename: String,
    },
    UninstallConfirm {
        tag: String,
    },
}

impl Modal {
    pub fn is_closed(&self) -> bool {
        matches!(self, Modal::Closed)
    }
}

pub struct AppDeps {
    pub config: Config,
    pub config_path: PathBuf,
    pub library_root: PathBuf,
    pub rom_library_root: PathBuf,
    pub download_dir: PathBuf,
    pub game: &'static dyn Game,
    pub platform: &'static dyn Platform,
    pub client: Arc<github::Client>,
    pub startup_diagnostics: Vec<Diagnostic>,
}

pub struct App {
    pub(crate) config: Config,
    config_path: PathBuf,
    library_root: PathBuf,
    download_dir: PathBuf,
    game: &'static dyn Game,
    platform: &'static dyn Platform,
    client: Arc<github::Client>,

    pub(crate) installed: Vec<InstalledVersion>,
    pub(crate) releases_by_game: HashMap<String, Vec<Release>>,
    pub(crate) install_states: HashMap<String, InstallState>,
    pub(crate) install_progress: HashMap<String, Option<u8>>,
    running: HashMap<String, LaunchHandle>,
    pub(crate) rate_limit: RateLimitStatus,
    pub(crate) banners: Vec<Banner>,
    pub(crate) tab: Tab,
    pub(crate) modal: Modal,

    pub(crate) library_root_input: String,
    pub(crate) versions_to_show_input: String,

    rom_library_root: PathBuf,
    pub(crate) roms: Vec<RomEntry>,

    pub(crate) selected_game_slug: String,
    pub(crate) selected_tags: HashMap<String, String>,
    pub(crate) gear_menu_open_for_game: Option<String>,
    pub(crate) imported_roms_expanded: bool,
}

impl App {
    pub fn new(deps: AppDeps) -> (Self, Task<Message>) {
        let AppDeps {
            config,
            config_path,
            library_root,
            rom_library_root,
            download_dir,
            game,
            platform,
            client,
            startup_diagnostics,
        } = deps;

        let installed = library::scan(&library_root, &config);
        let library_root_input = library_root.display().to_string();

        let roms = rom_library::list(&rom_library_root).unwrap_or_default();

        let banners: Vec<Banner> = startup_diagnostics
            .into_iter()
            .map(|d| match d {
                Diagnostic::ConfigParseError { backup, message } => Banner::Error(format!(
                    "config parse error ({message}); backed up to {}",
                    backup.display()
                )),
                Diagnostic::SchemaVersionMismatch { backup, found } => Banner::Info(format!(
                    "unknown config schema version {found}; backed up to {}",
                    backup.display()
                )),
                Diagnostic::RomMigrationSkipped { path } => Banner::Info(format!(
                    "rom migration: source not found, skipped {}",
                    path.display()
                )),
                Diagnostic::RomMigrationFailed { path, message } => Banner::Error(format!(
                    "rom migration failed for {}: {message}",
                    path.display()
                )),
            })
            .collect();

        let mut install_states = HashMap::new();
        for v in &installed {
            install_states.insert(v.tag.clone(), InstallState::Installed);
        }

        let initial_rate_limit = config
            .rate_limit_snapshot
            .as_ref()
            .map(|s| RateLimitStatus {
                remaining: s.remaining,
                limit: s.limit,
                reset_at: s
                    .reset_at_unix
                    .and_then(|t| chrono::Utc.timestamp_opt(t, 0).single()),
            })
            .unwrap_or_default();

        let selected_game_slug = config
            .last_launched
            .as_ref()
            .map(|l| l.game_slug.clone())
            .unwrap_or_else(|| {
                games_mod::registry()
                    .first()
                    .map(|g| g.slug().to_string())
                    .unwrap_or_default()
            });
        let mut selected_tags: HashMap<String, String> = HashMap::new();
        if let Some(last) = config.last_launched.as_ref() {
            selected_tags.insert(last.game_slug.clone(), last.tag.clone());
        }
        let versions_to_show_input = config.versions_to_show.to_string();

        let app = Self {
            config,
            config_path,
            library_root,
            download_dir,
            game,
            platform,
            client: client.clone(),
            installed,
            releases_by_game: HashMap::new(),
            install_states,
            install_progress: HashMap::new(),
            running: HashMap::new(),
            rate_limit: initial_rate_limit,
            banners,
            tab: Tab::Library,
            modal: Modal::Closed,
            library_root_input,
            versions_to_show_input,
            rom_library_root,
            roms,
            selected_game_slug,
            selected_tags,
            gear_menu_open_for_game: None,
            imported_roms_expanded: false,
        };

        let release_tasks: Vec<Task<Message>> = games_mod::registry()
            .iter()
            .map(|g| {
                let client = client.clone();
                let repo = g.repo_slug().to_string();
                let slug = g.slug().to_string();
                Task::perform(
                    async move { client.list_releases(&repo).await.map_err(|e| e.to_string()) },
                    move |result| Message::ReleasesLoaded {
                        game_slug: slug.clone(),
                        result,
                    },
                )
            })
            .collect();

        (app, Task::batch(release_tasks))
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::TabSelected(t) => {
                self.tab = t;
                self.modal = Modal::Closed;
                self.gear_menu_open_for_game = None;
                Task::none()
            }
            Message::DismissPopovers => {
                self.gear_menu_open_for_game = None;
                Task::none()
            }
            Message::ToggleGearMenu(slug) => {
                if self.gear_menu_open_for_game.as_deref() == Some(slug.as_str()) {
                    self.gear_menu_open_for_game = None;
                } else {
                    self.gear_menu_open_for_game = Some(slug);
                }
                Task::none()
            }
            Message::ToggleImportedRomsExpander => {
                self.imported_roms_expanded = !self.imported_roms_expanded;
                Task::none()
            }
            Message::VersionSelected { game_slug, tag } => {
                self.selected_tags.insert(game_slug.clone(), tag);
                self.selected_game_slug = game_slug;
                Task::none()
            }
            Message::PrimaryActionClicked(slug) => {
                self.gear_menu_open_for_game = None;
                let tag = match self.selected_tags.get(&slug).cloned() {
                    Some(t) => t,
                    None => {
                        let Some(first) = self.versions_for_game(&slug).into_iter().next() else {
                            return Task::none();
                        };
                        first.tag
                    }
                };
                self.selected_game_slug = slug.clone();
                self.selected_tags.insert(slug.clone(), tag.clone());
                let installed = self
                    .installed
                    .iter()
                    .any(|v| v.tag == tag && v.game_slug == slug);
                if installed {
                    self.update(Message::LaunchClicked(tag))
                } else {
                    self.update(Message::InstallClicked {
                        game_slug: slug,
                        tag,
                    })
                }
            }
            Message::ManualRefreshClicked(slug) => {
                self.gear_menu_open_for_game = None;
                if let (Some(remaining), Some(reset_unix)) =
                    (self.rate_limit.remaining, self.rate_limit.reset_at)
                    && remaining == 0
                    && reset_unix > chrono::Utc::now()
                {
                    self.banners.push(Banner::RateLimited(format!(
                        "GitHub rate limit reached. Try again at {}.",
                        reset_unix.with_timezone(&chrono::Local).format("%H:%M:%S")
                    )));
                    return Task::none();
                }
                let Some(game) = game_for_slug(&slug) else {
                    return Task::none();
                };
                let client = self.client.clone();
                let repo = game.repo_slug().to_string();
                let slug_for_msg = slug.clone();
                Task::perform(
                    async move { client.list_releases(&repo).await.map_err(|e| e.to_string()) },
                    move |result| Message::ReleasesLoaded {
                        game_slug: slug_for_msg.clone(),
                        result,
                    },
                )
            }
            Message::UninstallSelectedClicked(slug) => {
                self.gear_menu_open_for_game = None;
                let Some(tag) = self.effective_tag_for(&slug) else {
                    return Task::none();
                };
                if !self
                    .installed
                    .iter()
                    .any(|v| v.tag == tag && v.game_slug == slug)
                {
                    return Task::none();
                }
                self.modal = Modal::UninstallConfirm { tag };
                Task::none()
            }
            Message::ClearCacheSelectedClicked(slug) => {
                self.gear_menu_open_for_game = None;
                let Some(tag) = self.effective_tag_for(&slug) else {
                    return Task::none();
                };
                self.update(Message::ClearCachedAssetsClicked(tag))
            }
            Message::VersionsToShowInputChanged(s) => {
                self.versions_to_show_input = s;
                Task::none()
            }
            Message::VersionsToShowSubmit => {
                match self.versions_to_show_input.trim().parse::<u32>() {
                    Ok(n) if n >= MIN_VERSIONS_TO_SHOW => {
                        if self.config.versions_to_show != n {
                            self.config.versions_to_show = n;
                            if self.save_config() {
                                self.banners
                                    .push(Banner::Info(format!("versions to show set to {n}")));
                            }
                        }
                    }
                    _ => {
                        self.versions_to_show_input = self.config.versions_to_show.to_string();
                        self.banners.push(Banner::Error(format!(
                            "versions to show must be an integer >= {MIN_VERSIONS_TO_SHOW}"
                        )));
                    }
                }
                Task::none()
            }
            Message::DeleteRomConfirm(filename) => {
                let cleared = self.config.clear_assignments_referencing(&filename);
                if cleared > 0 {
                    self.save_config();
                }
                if let Err(e) = rom_library::delete(&self.rom_library_root, &filename) {
                    self.banners
                        .push(Banner::Error(format!("delete {filename}: {e}")));
                } else {
                    self.banners
                        .push(Banner::Info(format!("deleted {filename}")));
                    self.refresh_rom_list();
                }
                self.modal = Modal::Closed;
                Task::none()
            }
            Message::DeleteRomCancel => {
                self.modal = Modal::Closed;
                Task::none()
            }
            Message::ReleasesLoaded {
                game_slug,
                result: Ok((releases, rl)),
            } => {
                for r in &releases {
                    self.install_states
                        .entry(r.tag_name.clone())
                        .or_insert(InstallState::Idle);
                }
                self.releases_by_game.insert(game_slug, releases);
                self.rate_limit = rl;
                self.persist_rate_limit_snapshot(rl);
                Task::none()
            }
            Message::ReleasesLoaded {
                game_slug: _,
                result: Err(e),
            } => {
                if e.contains("rate limited") {
                    self.banners.push(Banner::RateLimited(e));
                } else {
                    self.banners.push(Banner::Error(format!("releases: {e}")));
                }
                Task::none()
            }
            Message::InstallClicked { game_slug, tag } => {
                if matches!(
                    self.install_states.get(&tag),
                    Some(InstallState::Installing) | Some(InstallState::Installed)
                ) {
                    return Task::none();
                }
                let Some(release) = self
                    .releases_for(&game_slug)
                    .iter()
                    .find(|r| r.tag_name == tag)
                    .cloned()
                else {
                    return Task::none();
                };
                let game = game_for_slug(&game_slug).or_else(|| {
                    if self.game.slug() == game_slug {
                        Some(self.game)
                    } else {
                        None
                    }
                });
                let Some(game) = game else {
                    return Task::none();
                };
                self.install_states
                    .insert(tag.clone(), InstallState::Installing);
                self.install_progress.insert(tag.clone(), None);

                let client = self.client.clone();
                let platform = self.platform;
                let library_root = self.library_root.clone();
                let download_dir = self.download_dir.clone();
                let tag_out = tag.clone();

                Task::stream(iced::stream::channel(16, move |mut output| async move {
                    use futures_util::SinkExt;
                    let (tx, mut rx) = tokio::sync::mpsc::channel::<library::InstallProgress>(32);

                    let mut output_fwd = output.clone();
                    let tag_fwd = tag_out.clone();
                    let forwarder = tokio::spawn(async move {
                        let mut last_pct: Option<u8> = None;
                        let mut last_was_indeterminate = false;
                        while let Some(p) = rx.recv().await {
                            let msg = match p {
                                library::InstallProgress::Downloading {
                                    downloaded,
                                    total: Some(total),
                                } if total > 0 => {
                                    let pct =
                                        ((downloaded.saturating_mul(100)) / total).min(100) as u8;
                                    if last_pct == Some(pct) {
                                        continue;
                                    }
                                    last_pct = Some(pct);
                                    last_was_indeterminate = false;
                                    Some(Message::InstallProgress(tag_fwd.clone(), Some(pct)))
                                }
                                library::InstallProgress::Downloading { .. } => {
                                    if last_was_indeterminate {
                                        continue;
                                    }
                                    last_was_indeterminate = true;
                                    last_pct = None;
                                    Some(Message::InstallProgress(tag_fwd.clone(), None))
                                }
                                library::InstallProgress::Starting
                                | library::InstallProgress::Extracting
                                | library::InstallProgress::Finalizing => {
                                    last_pct = None;
                                    last_was_indeterminate = false;
                                    Some(Message::InstallProgress(tag_fwd.clone(), None))
                                }
                                library::InstallProgress::Done(_) => None,
                            };
                            if let Some(m) = msg {
                                let _ = output_fwd.send(m).await;
                            }
                        }
                    });

                    let result = library::install(
                        &client,
                        InstallRequest {
                            game,
                            release: &release,
                            platform,
                            library_root: &library_root,
                            destination_override: None,
                            download_dir: &download_dir,
                        },
                        Some(tx),
                    )
                    .await
                    .map(|(v, _)| v)
                    .map_err(|e| e.to_string());

                    let _ = forwarder.await;
                    let _ = output.send(Message::InstallFinished(tag_out, result)).await;
                }))
            }
            Message::InstallProgress(tag, pct) => {
                if matches!(
                    self.install_states.get(&tag),
                    Some(InstallState::Installing)
                ) {
                    self.install_progress.insert(tag, pct);
                }
                Task::none()
            }
            Message::InstallFinished(tag, Ok(version)) => {
                self.install_states
                    .insert(tag.clone(), InstallState::Installed);
                self.install_progress.remove(&tag);
                if !self.installed.iter().any(|v| v.tag == version.tag) {
                    self.installed.push(version);
                }
                Task::none()
            }
            Message::InstallFinished(tag, Err(e)) => {
                self.install_states
                    .insert(tag.clone(), InstallState::Failed(e.clone()));
                self.install_progress.remove(&tag);
                self.banners
                    .push(Banner::Error(format!("install {tag} failed: {e}")));
                Task::none()
            }
            Message::UninstallClicked(tag) => {
                self.modal = Modal::Closed;
                if self.running.get_mut(&tag).is_some_and(|h| h.is_running()) {
                    self.banners.push(Banner::Info(format!(
                        "cannot uninstall {tag}: still running"
                    )));
                    return Task::none();
                }
                if let Some(idx) = self.installed.iter().position(|v| v.tag == tag) {
                    let v = self.installed.remove(idx);
                    if let Err(e) = library::uninstall(&v) {
                        self.banners
                            .push(Banner::Error(format!("uninstall {tag}: {e}")));
                    }
                }
                self.install_states.insert(tag, InstallState::Idle);
                Task::none()
            }
            Message::LaunchClicked(tag) => {
                if self.running.get_mut(&tag).is_some_and(|h| h.is_running()) {
                    return Task::none();
                }
                let Some(installed) = self.installed.iter().find(|v| v.tag == tag).cloned() else {
                    return Task::none();
                };
                let game = game_for_slug(&installed.game_slug).unwrap_or(self.game);
                match launcher::launch(
                    &installed,
                    game,
                    self.platform,
                    &self.config,
                    &self.rom_library_root,
                ) {
                    Ok(handle) => {
                        self.running.insert(tag.clone(), handle);
                        let last = crate::config::schema::LastLaunched {
                            game_slug: installed.game_slug.clone(),
                            tag: tag.clone(),
                        };
                        if self.config.last_launched.as_ref() != Some(&last) {
                            self.config.last_launched = Some(last);
                            self.save_config();
                        }
                    }
                    Err(e) => {
                        self.banners
                            .push(Banner::Error(format!("launch {tag}: {e}")));
                    }
                }
                Task::none()
            }
            Message::LibraryRootInputChanged(s) => {
                self.library_root_input = s;
                Task::none()
            }
            Message::SaveSettings => {
                self.config.library_root = Some(PathBuf::from(&self.library_root_input));
                if self.save_config() {
                    self.banners
                        .push(Banner::Info("settings saved".to_string()));
                }
                Task::none()
            }
            Message::ClearCachedAssetsClicked(tag) => {
                let Some(installed) = self.installed.iter().find(|v| v.tag == tag).cloned() else {
                    return Task::none();
                };
                let Some(game) = game_for_slug(&installed.game_slug) else {
                    return Task::none();
                };
                let planned = cached_assets::plan_clear(game, &installed.path, self.platform);
                if planned.is_empty() {
                    self.banners
                        .push(Banner::Info(format!("{tag}: no cached assets to clear")));
                    return Task::none();
                }
                self.modal = Modal::ClearCachedConfirm {
                    tag,
                    game_slug: installed.game_slug,
                    planned,
                };
                Task::none()
            }
            Message::ClearCachedAssetsConfirm(tag) => {
                let Some(installed) = self.installed.iter().find(|v| v.tag == tag).cloned() else {
                    self.modal = Modal::Closed;
                    return Task::none();
                };
                let Some(game) = game_for_slug(&installed.game_slug) else {
                    self.modal = Modal::Closed;
                    return Task::none();
                };
                let result =
                    cached_assets::clear_cached_assets(game, &installed.path, self.platform);
                self.banners.push(Banner::Info(format!(
                    "{tag}: cleared {} cached file(s)",
                    result.deleted.len()
                )));
                for (path, e) in result.failures {
                    self.banners
                        .push(Banner::Error(format!("clear {}: {e}", path.display())));
                }
                self.modal = Modal::Closed;
                Task::none()
            }
            Message::ClearCachedAssetsCancel => {
                self.modal = Modal::Closed;
                Task::none()
            }

            Message::ImportRomClicked => {
                let lib_root = self.rom_library_root.clone();
                Task::perform(
                    async move {
                        let Some(handle) = rfd::AsyncFileDialog::new()
                            .set_title("Import ROM")
                            .pick_file()
                            .await
                        else {
                            return Err("import cancelled".to_string());
                        };
                        let src = handle.path().to_path_buf();
                        // Big files: copy off the UI thread.
                        tokio::task::spawn_blocking(move || rom_library::import(&lib_root, &src))
                            .await
                            .map_err(|e| e.to_string())?
                            .map_err(|e| e.to_string())
                    },
                    Message::RomImported,
                )
            }
            Message::RomImported(Ok(entry)) => {
                self.banners
                    .push(Banner::Info(format!("imported {}", entry.filename)));
                self.refresh_rom_list();
                Task::none()
            }
            Message::RomImported(Err(e)) => {
                if e != "import cancelled" {
                    self.banners.push(Banner::Error(format!("import rom: {e}")));
                }
                Task::none()
            }
            Message::DeleteRomClicked(filename) => {
                self.modal = Modal::DeleteRomConfirm { filename };
                Task::none()
            }
            Message::AssignSlotChanged {
                game_slug,
                slot_id,
                filename,
            } => {
                self.config
                    .set_assignment(&game_slug, &slot_id, filename.clone());
                self.save_config();
                Task::none()
            }
        }
    }

    pub(crate) fn releases_for(&self, slug: &str) -> &[Release] {
        self.releases_by_game
            .get(slug)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    pub(crate) fn selected_tag_for(&self, slug: &str) -> Option<&str> {
        self.selected_tags.get(slug).map(|s| s.as_str())
    }

    /// Tag the row currently acts on: explicit user selection if any,
    /// otherwise the first entry in the displayed version list.
    pub(crate) fn effective_tag_for(&self, slug: &str) -> Option<String> {
        if let Some(t) = self.selected_tags.get(slug) {
            return Some(t.clone());
        }
        self.versions_for_game(slug).into_iter().next().map(|v| v.tag)
    }

    pub(crate) fn install_state_for_game(&self, slug: &str) -> Option<InstallState> {
        let tag = self.effective_tag_for(slug)?;
        self.install_states.get(&tag).cloned()
    }

    fn refresh_rom_list(&mut self) {
        self.roms = rom_library::list(&self.rom_library_root).unwrap_or_default();
    }

    /// Persist `self.config` to disk; on failure push an error banner. Returns
    /// `true` when the save succeeded so callers can chain an info banner.
    fn save_config(&mut self) -> bool {
        match self.config.save_to(&self.config_path) {
            Ok(()) => true,
            Err(e) => {
                self.banners
                    .push(Banner::Error(format!("save config: {e}")));
                false
            }
        }
    }

    /// Update `Config.rate_limit_snapshot` from a freshly-observed
    /// `RateLimitStatus`. Persists only when `remaining` or `reset_at` actually
    /// changed vs. the prior snapshot, to avoid one disk write per refresh
    /// tick.
    fn persist_rate_limit_snapshot(&mut self, rl: RateLimitStatus) {
        let new_reset_unix = rl.reset_at.map(|d| d.timestamp());
        let changed = match &self.config.rate_limit_snapshot {
            Some(prev) => prev.remaining != rl.remaining || prev.reset_at_unix != new_reset_unix,
            None => rl.remaining.is_some() || rl.limit.is_some() || new_reset_unix.is_some(),
        };
        if !changed {
            return;
        }
        self.config.rate_limit_snapshot = Some(RateLimitSnapshot {
            remaining: rl.remaining,
            limit: rl.limit,
            reset_at_unix: new_reset_unix,
        });
        if let Err(e) = self.config.save_to(&self.config_path) {
            self.banners
                .push(Banner::Error(format!("save rate-limit snapshot: {e}")));
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let tabs = row![
            ui::tab_button("Library", self.tab == Tab::Library, Tab::Library),
            ui::tab_button("Roms", self.tab == Tab::Roms, Tab::Roms),
            ui::tab_button("Mods", self.tab == Tab::Mods, Tab::Mods),
            ui::tab_button("Settings", self.tab == Tab::Settings, Tab::Settings),
        ]
        .spacing(8);

        let banners = column(self.banners.iter().rev().take(5).map(|b| {
            let s = match b {
                Banner::Error(s) | Banner::RateLimited(s) => format!("! {s}"),
                Banner::Info(s) => format!("i {s}"),
            };
            text(s).size(12).into()
        }))
        .spacing(2);

        let body: Element<_> = match self.tab {
            Tab::Library => self.library_view(),
            Tab::Roms => self.roms_view(),
            Tab::Mods => self.mods_view(),
            Tab::Settings => self.settings_view(),
        };

        let page: Element<_> = container(column![tabs, banners, body].spacing(12))
            .padding(12)
            .width(Length::Fill)
            .height(Length::Fill)
            .into();

        if self.modal.is_closed() {
            page
        } else {
            let backdrop = container(text(""))
                .width(Length::Fill)
                .height(Length::Fill)
                .style(|_theme: &iced::Theme| container::Style {
                    background: Some(iced::Background::Color(iced::Color {
                        a: 0.4,
                        ..iced::Color::BLACK
                    })),
                    ..container::Style::default()
                });
            let card = container(self.modal_view())
                .padding(16)
                .max_width(520)
                .style(|theme: &iced::Theme| {
                    let palette = theme.extended_palette();
                    container::Style {
                        background: Some(iced::Background::Color(palette.background.base.color)),
                        border: iced::Border {
                            color: palette.background.strong.color,
                            width: 1.0,
                            radius: 8.0.into(),
                        },
                        ..container::Style::default()
                    }
                });
            let centered = container(card)
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill);
            stack![page, opaque(backdrop), opaque(centered)].into()
        }
    }

    #[cfg(test)]
    pub fn install_state(&self, tag: &str) -> Option<&InstallState> {
        self.install_states.get(tag)
    }
    #[cfg(test)]
    pub fn installed(&self) -> &[InstalledVersion] {
        &self.installed
    }
    #[cfg(test)]
    pub fn running_contains(&mut self, tag: &str) -> bool {
        self.running.get_mut(tag).is_some_and(|h| h.is_running())
    }
    #[cfg(test)]
    pub fn banners(&self) -> &[Banner] {
        &self.banners
    }
}

pub(crate) fn game_for_slug(slug: &str) -> Option<&'static dyn Game> {
    games_mod::registry()
        .iter()
        .copied()
        .find(|g| g.slug() == slug)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::games::{CachedAssetSpec, Game};
    use crate::github::ReleaseAsset;
    use std::path::Path;
    use std::process::Command;
    use std::sync::OnceLock;
    use tempfile::tempdir;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

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

    struct FakeGame;
    impl Game for FakeGame {
        fn slug(&self) -> &'static str {
            "fake"
        }
        fn repo_slug(&self) -> &'static str {
            "fake/repo"
        }
        fn display_name(&self) -> &'static str {
            "Fake"
        }
        fn data_dir(&self, install_dir: &Path, _: &dyn Platform) -> PathBuf {
            install_dir.to_path_buf()
        }
        fn slots(&self) -> &'static [crate::games::SlotSpec] {
            const S: &[crate::games::SlotSpec] = &[crate::games::SlotSpec {
                id: "oot",
                display_name: "OoT",
                symlink_filename: "oot.z64",
            }];
            S
        }
        fn cached_assets(&self) -> &'static [CachedAssetSpec] {
            &[]
        }
        fn pick_asset<'a>(
            &self,
            a: &'a [ReleaseAsset],
            _: &dyn Platform,
        ) -> Option<&'a ReleaseAsset> {
            a.first()
        }
        fn launch_command(&self, install_dir: &Path, _: &dyn Platform) -> Command {
            Command::new(install_dir.join("mockgame.sh"))
        }
        fn extract(&self, _archive: &Path, dest: &Path, _: &dyn Platform) -> anyhow::Result<()> {
            std::fs::create_dir_all(dest)?;
            let script = dest.join("mockgame.sh");
            std::fs::write(
                &script,
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$(dirname \"$0\")/args.txt\"\n",
            )?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
            }
            Ok(())
        }
    }

    fn static_platform() -> &'static dyn Platform {
        static P: OnceLock<FakePlatform> = OnceLock::new();
        P.get_or_init(|| FakePlatform)
    }
    fn static_game() -> &'static dyn Game {
        static G: OnceLock<FakeGame> = OnceLock::new();
        G.get_or_init(|| FakeGame)
    }

    async fn serve_asset(body: &[u8]) -> (MockServer, String, String) {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/fake/repo/releases"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("etag", "\"r1\"")
                    .insert_header("x-ratelimit-remaining", "59")
                    .insert_header("x-ratelimit-limit", "60")
                    .set_body_json(serde_json::json!([{
                        "tag_name": "1.0.0",
                        "name": "v1",
                        "published_at": null,
                        "assets": [{
                            "name": "fake-Mac.zip",
                            "browser_download_url": format!("{}/dl/fake.zip", server.uri()),
                            "size": body.len() as u64,
                        }]
                    }])),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/dl/fake.zip"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body.to_vec()))
            .mount(&server)
            .await;
        let releases_url = format!("{}/repos/fake/repo/releases", server.uri());
        let dl_url = format!("{}/dl/fake.zip", server.uri());
        (server, releases_url, dl_url)
    }

    #[tokio::test]
    async fn full_install_then_launch_flow() {
        let dir = tempdir().unwrap();
        let library_root = dir.path().join("library");
        let download_dir = dir.path().join("downloads");
        let config_path = dir.path().join("config.yaml");
        let cache_path = dir.path().join("etags.json");

        let (_server, _releases_url, _dl_url) = serve_asset(b"ignored-archive-bytes").await;
        let client = Arc::new(github::Client::with_base(cache_path, _server.uri()).unwrap());

        let config = Config {
            library_root: Some(library_root.clone()),
            ..Config::default()
        };

        let (mut app, _startup) = App::new(AppDeps {
            config,
            config_path,
            library_root: library_root.clone(),
            rom_library_root: dir.path().join("roms"),
            download_dir: download_dir.clone(),
            game: static_game(),
            platform: static_platform(),
            client: client.clone(),
            startup_diagnostics: vec![],
        });

        let fetched = client
            .list_releases("fake/repo")
            .await
            .map_err(|e| e.to_string());
        let _ = app.update(Message::ReleasesLoaded {
            game_slug: "fake".into(),
            result: fetched,
        });
        assert_eq!(app.releases_for("fake").len(), 1);
        assert!(matches!(
            app.install_state("1.0.0"),
            Some(InstallState::Idle)
        ));

        let _task = app.update(Message::InstallClicked {
            game_slug: "fake".into(),
            tag: "1.0.0".into(),
        });
        assert!(matches!(
            app.install_state("1.0.0"),
            Some(InstallState::Installing)
        ));
        let release = app.releases_for("fake")[0].clone();
        let installed = library::install(
            &client,
            InstallRequest {
                game: static_game(),
                release: &release,
                platform: static_platform(),
                library_root: &library_root,
                destination_override: None,
                download_dir: &download_dir,
            },
            None,
        )
        .await
        .map(|(v, _)| v)
        .map_err(|e| e.to_string());
        let _ = app.update(Message::InstallFinished("1.0.0".into(), installed));

        assert!(
            matches!(app.install_state("1.0.0"), Some(InstallState::Installed)),
            "expected Installed, got {:?}",
            app.install_state("1.0.0")
        );
        assert_eq!(app.installed().len(), 1);

        let _ = app.update(Message::LaunchClicked("1.0.0".into()));
        for _ in 0..100 {
            if !app.running_contains("1.0.0") {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let args_file = library_root.join("fake").join("1.0.0").join("args.txt");
        let args = std::fs::read_to_string(&args_file).expect("mock game should have run");
        // Launcher passes no extra args beyond what the binary itself sees.
        assert!(
            !args.contains("--baserompath"),
            "no rom flag should be passed"
        );
    }

    #[tokio::test]
    async fn settings_save_persists_config() {
        let dir = tempdir().unwrap();
        let library_root = dir.path().join("library");
        let config_path = dir.path().join("config.yaml");
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/fake/repo/releases"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .mount(&server)
            .await;
        let client = Arc::new(
            github::Client::with_base(dir.path().join("etags.json"), server.uri()).unwrap(),
        );

        let (mut app, _startup) = App::new(AppDeps {
            config: Config::default(),
            config_path: config_path.clone(),
            library_root: library_root.clone(),
            rom_library_root: dir.path().join("roms"),
            download_dir: dir.path().join("dl"),
            game: static_game(),
            platform: static_platform(),
            client,
            startup_diagnostics: vec![],
        });

        let _ = app.update(Message::LibraryRootInputChanged(
            library_root.display().to_string(),
        ));
        let _ = app.update(Message::SaveSettings);

        let loaded = Config::load_from(&config_path).unwrap();
        assert_eq!(loaded.config.library_root, Some(library_root));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn slot_assignment_drives_symlink_at_launch() {
        let dir = tempdir().unwrap();
        let library_root = dir.path().join("library");
        let rom_library_root = dir.path().join("roms");
        let config_path = dir.path().join("config.yaml");

        let (_server, _releases_url, _dl_url) = serve_asset(b"x").await;
        let client = Arc::new(
            github::Client::with_base(dir.path().join("etags.json"), _server.uri()).unwrap(),
        );

        let config = Config {
            library_root: Some(library_root.clone()),
            ..Config::default()
        };

        let (mut app, _startup) = App::new(AppDeps {
            config,
            config_path: config_path.clone(),
            library_root: library_root.clone(),
            rom_library_root: rom_library_root.clone(),
            download_dir: dir.path().join("dl"),
            game: static_game(),
            platform: static_platform(),
            client: client.clone(),
            startup_diagnostics: vec![],
        });

        // Stage a ROM in the library and feed it through RomImported.
        std::fs::create_dir_all(&rom_library_root).unwrap();
        std::fs::write(rom_library_root.join("oot.z64"), b"rom").unwrap();
        let _ = app.update(Message::RomImported(Ok(RomEntry {
            filename: "oot.z64".into(),
            size: 3,
        })));

        // Assign the ROM to the FakeGame's "oot" slot.
        let _ = app.update(Message::AssignSlotChanged {
            game_slug: "fake".into(),
            slot_id: "oot".into(),
            filename: Some("oot.z64".into()),
        });

        // Install a fake version (writes mockgame.sh into the install dir).
        let fetched = client
            .list_releases("fake/repo")
            .await
            .map_err(|e| e.to_string());
        let _ = app.update(Message::ReleasesLoaded {
            game_slug: "fake".into(),
            result: fetched,
        });
        let release = app.releases_for("fake")[0].clone();
        let installed = library::install(
            &client,
            InstallRequest {
                game: static_game(),
                release: &release,
                platform: static_platform(),
                library_root: &library_root,
                destination_override: None,
                download_dir: &dir.path().join("dl"),
            },
            None,
        )
        .await
        .map(|(v, _)| v)
        .map_err(|e| e.to_string());
        let _ = app.update(Message::InstallFinished("1.0.0".into(), installed));

        let install_dir = library_root.join("fake").join("1.0.0");
        let _ = app.update(Message::LaunchClicked("1.0.0".into()));
        for _ in 0..100 {
            if !app.running_contains("1.0.0") {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        let link = install_dir.join("oot.z64");
        assert!(link.is_symlink(), "expected oot.z64 symlink in install dir");
        assert_eq!(
            std::fs::read_link(&link).unwrap(),
            rom_library_root.join("oot.z64")
        );

        // Clear the assignment, relaunch, symlink should be removed.
        let _ = app.update(Message::AssignSlotChanged {
            game_slug: "fake".into(),
            slot_id: "oot".into(),
            filename: None,
        });
        let _ = app.update(Message::LaunchClicked("1.0.0".into()));
        for _ in 0..100 {
            if !app.running_contains("1.0.0") {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(
            !install_dir.join("oot.z64").exists(),
            "symlink should be gone"
        );
    }

    fn make_app(dir: &tempfile::TempDir, config: Config) -> App {
        let client = Arc::new(
            github::Client::with_base(
                dir.path().join("etags.json"),
                "http://127.0.0.1:1".to_string(),
            )
            .unwrap(),
        );
        let (app, _startup) = App::new(AppDeps {
            config,
            config_path: dir.path().join("config.yaml"),
            library_root: dir.path().join("library"),
            rom_library_root: dir.path().join("roms"),
            download_dir: dir.path().join("dl"),
            game: static_game(),
            platform: static_platform(),
            client,
            startup_diagnostics: vec![],
        });
        app
    }

    #[tokio::test]
    async fn tab_switch_dismisses_modal_and_gear_popover() {
        let dir = tempdir().unwrap();
        let mut app = make_app(&dir, Config::default());
        app.modal = Modal::DeleteRomConfirm {
            filename: "rom.z64".into(),
        };
        app.gear_menu_open_for_game = Some("fake".into());
        let _ = app.update(Message::TabSelected(Tab::Roms));
        assert!(matches!(app.modal, Modal::Closed));
        assert!(app.gear_menu_open_for_game.is_none());
    }

    #[tokio::test]
    async fn delete_rom_confirm_clears_assignments_across_games() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("roms")).unwrap();
        std::fs::write(dir.path().join("roms/shared.z64"), b"x").unwrap();
        let mut config = Config::default();
        config.set_assignment("fake", "oot", Some("shared.z64".to_string()));
        config.set_assignment("other", "slot", Some("shared.z64".to_string()));
        let mut app = make_app(&dir, config);
        app.refresh_rom_list();
        let _ = app.update(Message::DeleteRomConfirm("shared.z64".to_string()));
        assert!(app.config.assignment_for("fake", "oot").is_none());
        assert!(app.config.assignment_for("other", "slot").is_none());
        assert!(matches!(app.modal, Modal::Closed));
    }

    #[tokio::test]
    async fn cold_start_preselects_last_launched() {
        let dir = tempdir().unwrap();
        let config = Config {
            last_launched: Some(crate::config::schema::LastLaunched {
                game_slug: "fake".to_string(),
                tag: "v9".to_string(),
            }),
            ..Config::default()
        };
        let app = make_app(&dir, config);
        assert_eq!(app.selected_game_slug, "fake");
        assert_eq!(app.selected_tag_for("fake"), Some("v9"));
    }

    #[tokio::test]
    async fn versions_to_show_invalid_input_keeps_previous_value() {
        let dir = tempdir().unwrap();
        let mut app = make_app(&dir, Config::default());
        let original = app.config.versions_to_show;
        let _ = app.update(Message::VersionsToShowInputChanged("0".into()));
        let _ = app.update(Message::VersionsToShowSubmit);
        assert_eq!(app.config.versions_to_show, original);
        let _ = app.update(Message::VersionsToShowInputChanged("not-a-number".into()));
        let _ = app.update(Message::VersionsToShowSubmit);
        assert_eq!(app.config.versions_to_show, original);
        let _ = app.update(Message::VersionsToShowInputChanged("3".into()));
        let _ = app.update(Message::VersionsToShowSubmit);
        assert_eq!(app.config.versions_to_show, 3);
    }
}
