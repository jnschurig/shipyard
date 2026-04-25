use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use iced::widget::{
    button, column, container, opaque, pick_list, row, scrollable, stack, text, text_input,
};
use iced::{Element, Length, Task};

use crate::config::{Config, Diagnostic};
use crate::games::{self as games_mod, Game};
use crate::github::{self, RateLimitStatus, Release};
use crate::launcher::{self, LaunchHandle};
use crate::library::{self, InstallRequest, InstalledVersion};
use crate::platform::Platform;
use crate::roms::cached_assets;
use crate::roms::library::{self as rom_library, RomEntry};

#[derive(Debug, Clone)]
pub enum InstallState {
    Idle,
    Installing,
    Installed,
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Library,
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
    ReleasesLoaded(Result<(Vec<Release>, RateLimitStatus), String>),
    InstallClicked(String),
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
    AssignSlotChanged {
        game_slug: String,
        slot_id: String,
        filename: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub enum Modal {
    Closed,
    ClearCachedConfirm {
        tag: String,
        game_slug: String,
        planned: Vec<crate::roms::cached_assets::PlannedClear>,
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
    pub cache_dir: PathBuf,
    pub game: &'static dyn Game,
    pub platform: &'static dyn Platform,
    pub client: Arc<github::Client>,
    pub startup_diagnostics: Vec<Diagnostic>,
}

pub struct App {
    config: Config,
    config_path: PathBuf,
    library_root: PathBuf,
    download_dir: PathBuf,
    #[allow(dead_code)]
    cache_dir: PathBuf,
    game: &'static dyn Game,
    platform: &'static dyn Platform,
    client: Arc<github::Client>,

    installed: Vec<InstalledVersion>,
    releases: Vec<Release>,
    install_states: HashMap<String, InstallState>,
    running: HashMap<String, LaunchHandle>,
    rate_limit: RateLimitStatus,
    banners: Vec<Banner>,
    tab: Tab,
    modal: Modal,

    library_root_input: String,

    rom_library_root: PathBuf,
    roms: Vec<RomEntry>,
}

impl App {
    pub fn new(deps: AppDeps) -> (Self, Task<Message>) {
        let AppDeps {
            config,
            config_path,
            library_root,
            rom_library_root,
            download_dir,
            cache_dir,
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

        let app = Self {
            config,
            config_path,
            library_root,
            download_dir,
            cache_dir,
            game,
            platform,
            client: client.clone(),
            installed,
            releases: Vec::new(),
            install_states,
            running: HashMap::new(),
            rate_limit: RateLimitStatus::default(),
            banners,
            tab: Tab::Library,
            modal: Modal::Closed,
            library_root_input,
            rom_library_root,
            roms,
        };

        let client_for_fetch = client;
        let repo = app.game.repo_slug().to_string();
        let releases_task = Task::perform(
            async move {
                client_for_fetch
                    .list_releases(&repo)
                    .await
                    .map_err(|e| e.to_string())
            },
            Message::ReleasesLoaded,
        );

        (app, releases_task)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::TabSelected(t) => {
                self.tab = t;
                Task::none()
            }
            Message::ReleasesLoaded(Ok((releases, rl))) => {
                for r in &releases {
                    self.install_states
                        .entry(r.tag_name.clone())
                        .or_insert(InstallState::Idle);
                }
                self.releases = releases;
                self.rate_limit = rl;
                Task::none()
            }
            Message::ReleasesLoaded(Err(e)) => {
                if e.contains("rate limited") {
                    self.banners.push(Banner::RateLimited(e));
                } else {
                    self.banners.push(Banner::Error(format!("releases: {e}")));
                }
                Task::none()
            }
            Message::InstallClicked(tag) => {
                if matches!(
                    self.install_states.get(&tag),
                    Some(InstallState::Installing) | Some(InstallState::Installed)
                ) {
                    return Task::none();
                }
                let Some(release) = self.releases.iter().find(|r| r.tag_name == tag).cloned()
                else {
                    return Task::none();
                };
                self.install_states
                    .insert(tag.clone(), InstallState::Installing);

                let client = self.client.clone();
                let game = self.game;
                let platform = self.platform;
                let library_root = self.library_root.clone();
                let download_dir = self.download_dir.clone();
                let tag_out = tag.clone();

                Task::perform(
                    async move {
                        library::install(
                            &client,
                            InstallRequest {
                                game,
                                release: &release,
                                platform,
                                library_root: &library_root,
                                destination_override: None,
                                download_dir: &download_dir,
                            },
                            None,
                        )
                        .await
                        .map(|(v, _)| v)
                        .map_err(|e| e.to_string())
                    },
                    move |res| Message::InstallFinished(tag_out.clone(), res),
                )
            }
            Message::InstallFinished(tag, Ok(version)) => {
                self.install_states
                    .insert(tag.clone(), InstallState::Installed);
                if !self.installed.iter().any(|v| v.tag == version.tag) {
                    self.installed.push(version);
                }
                Task::none()
            }
            Message::InstallFinished(tag, Err(e)) => {
                self.install_states
                    .insert(tag.clone(), InstallState::Failed(e.clone()));
                self.banners
                    .push(Banner::Error(format!("install {tag} failed: {e}")));
                Task::none()
            }
            Message::UninstallClicked(tag) => {
                if self
                    .running
                    .get_mut(&tag)
                    .is_some_and(|h| h.is_running())
                {
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
                if self
                    .running
                    .get_mut(&tag)
                    .is_some_and(|h| h.is_running())
                {
                    return Task::none();
                }
                let Some(installed) = self.installed.iter().find(|v| v.tag == tag).cloned() else {
                    return Task::none();
                };
                match launcher::launch(
                    &installed,
                    self.game,
                    self.platform,
                    &self.config,
                    &self.rom_library_root,
                ) {
                    Ok(handle) => {
                        self.running.insert(tag, handle);
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
                if let Err(e) = self.config.save_to(&self.config_path) {
                    self.banners.push(Banner::Error(format!("save config: {e}")));
                } else {
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
                    self.banners.push(Banner::Info(format!(
                        "{tag}: no cached assets to clear"
                    )));
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
                    self.banners.push(Banner::Error(format!(
                        "clear {}: {e}",
                        path.display()
                    )));
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
                if let Some((game_slug, slot_id)) = self.find_assignment_for(&filename) {
                    let display = display_name_for_slot(&game_slug, &slot_id)
                        .unwrap_or_else(|| format!("{game_slug}/{slot_id}"));
                    self.banners.push(Banner::Error(format!(
                        "cannot delete {filename}: assigned to {display}"
                    )));
                    return Task::none();
                }
                if let Err(e) = rom_library::delete(&self.rom_library_root, &filename) {
                    self.banners
                        .push(Banner::Error(format!("delete {filename}: {e}")));
                } else {
                    self.banners.push(Banner::Info(format!("deleted {filename}")));
                    self.refresh_rom_list();
                }
                Task::none()
            }
            Message::AssignSlotChanged {
                game_slug,
                slot_id,
                filename,
            } => {
                self.config
                    .set_assignment(&game_slug, &slot_id, filename.clone());
                if let Err(e) = self.config.save_to(&self.config_path) {
                    self.banners
                        .push(Banner::Error(format!("save config: {e}")));
                }
                Task::none()
            }
        }
    }

    fn refresh_rom_list(&mut self) {
        self.roms = rom_library::list(&self.rom_library_root).unwrap_or_default();
    }

    fn find_assignment_for(&self, filename: &str) -> Option<(String, String)> {
        for (game_slug, slots) in &self.config.slot_assignments {
            for (slot_id, fname) in slots {
                if fname == filename {
                    return Some((game_slug.clone(), slot_id.clone()));
                }
            }
        }
        None
    }

    pub fn view(&self) -> Element<'_, Message> {
        let tabs = row![
            tab_button("Library", self.tab == Tab::Library, Tab::Library),
            tab_button("Settings", self.tab == Tab::Settings, Tab::Settings),
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

    fn library_view(&self) -> Element<'_, Message> {
        let rows = self.releases.iter().map(|r| {
            let state = self
                .install_states
                .get(&r.tag_name)
                .cloned()
                .unwrap_or(InstallState::Idle);
            let status_label = match &state {
                InstallState::Idle => "available".to_string(),
                InstallState::Installing => "installing…".to_string(),
                InstallState::Installed => "installed".to_string(),
                InstallState::Failed(e) => format!("failed: {e}"),
            };
            let action: Element<_> = match &state {
                InstallState::Installed => {
                    let install_dir = self
                        .installed
                        .iter()
                        .find(|v| v.tag == r.tag_name)
                        .map(|v| v.path.clone());
                    let launch_btn = button("Launch")
                        .on_press(Message::LaunchClicked(r.tag_name.clone()));
                    let has_cached = install_dir
                        .as_deref()
                        .map(|p| {
                            cached_assets::scan_cached_assets(self.game, p, self.platform)
                                .iter()
                                .any(|c| c.status.is_present())
                        })
                        .unwrap_or(false);
                    let mut clear_btn = button("Clear cache");
                    if has_cached {
                        clear_btn = clear_btn
                            .on_press(Message::ClearCachedAssetsClicked(r.tag_name.clone()));
                    }
                    row![
                        launch_btn,
                        clear_btn,
                        button("Uninstall")
                            .on_press(Message::UninstallClicked(r.tag_name.clone())),
                    ]
                    .spacing(6)
                    .into()
                }
                InstallState::Installing => text("…").into(),
                _ => button("Install")
                    .on_press(Message::InstallClicked(r.tag_name.clone()))
                    .into(),
            };
            row![
                text(&r.tag_name).width(Length::FillPortion(2)),
                text(status_label).width(Length::FillPortion(2)),
                action,
            ]
            .spacing(12)
            .into()
        });
        scrollable(column(rows).spacing(6)).height(Length::Fill).into()
    }

    fn settings_view(&self) -> Element<'_, Message> {
        let rate = match self.rate_limit.remaining {
            Some(r) => format!(
                "GitHub rate limit: {r}/{}",
                self.rate_limit.limit.unwrap_or(0)
            ),
            None => "GitHub rate limit: unknown".to_string(),
        };
        let token_status = if std::env::var("GITHUB_TOKEN").is_ok_and(|v| !v.is_empty()) {
            "GITHUB_TOKEN: set"
        } else {
            "GITHUB_TOKEN: not set"
        };

        let mut body: iced::widget::Column<'_, Message> = column![
            text("Library root").size(14),
            text_input("path", &self.library_root_input)
                .on_input(Message::LibraryRootInputChanged),
            row![button("Save").on_press(Message::SaveSettings)].spacing(6),
        ]
        .spacing(8);

        body = body.push(section_header("Cached assets per install"));
        if self.installed.is_empty() {
            body = body.push(text("(no installed versions)").size(12));
        } else {
            for v in &self.installed {
                let game = match game_for_slug(&v.game_slug) {
                    Some(g) => g,
                    None => continue,
                };
                let cached = cached_assets::scan_cached_assets(game, &v.path, self.platform);
                let summary = cached
                    .iter()
                    .filter_map(|c| match &c.status {
                        cached_assets::CachedAssetStatus::Present { filename, .. } => {
                            Some(filename.to_string())
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                let summary_text = if summary.is_empty() {
                    "(no cached files)".to_string()
                } else {
                    summary
                };
                body = body.push(
                    column![
                        text(format!("{} {} — {}", game.display_name(), v.tag, v.path.display()))
                            .size(13),
                        text(summary_text).size(12),
                    ]
                    .spacing(4),
                );
            }
        }

        body = body.push(section_header("ROM Library"));
        body = body.push(
            row![button("Import ROM…").on_press(Message::ImportRomClicked)].spacing(6),
        );
        if self.roms.is_empty() {
            body = body.push(text("(no ROMs imported)").size(12));
        } else {
            for r in &self.roms {
                let filename = r.filename.clone();
                body = body.push(
                    row![
                        text(format!("{} ({} bytes)", r.filename, r.size))
                            .width(Length::Fill)
                            .size(12),
                        button("Delete").on_press(Message::DeleteRomClicked(filename)),
                    ]
                    .spacing(6),
                );
            }
        }

        body = body.push(section_header("Slot Assignments"));
        for game in games_mod::registry() {
            body = body.push(text(game.display_name()).size(13));
            for slot in game.slots() {
                let current = self
                    .config
                    .assignment_for(game.slug(), slot.id)
                    .map(|s| s.to_string());
                let options: Vec<SlotChoice> = std::iter::once(SlotChoice::unassigned())
                    .chain(self.roms.iter().map(|r| SlotChoice::filename(&r.filename)))
                    .collect();
                let selected = match &current {
                    Some(name) => SlotChoice::filename(name),
                    None => SlotChoice::unassigned(),
                };
                let game_slug = game.slug().to_string();
                let slot_id = slot.id.to_string();
                let picker = pick_list(options, Some(selected), move |c: SlotChoice| {
                    Message::AssignSlotChanged {
                        game_slug: game_slug.clone(),
                        slot_id: slot_id.clone(),
                        filename: c.into_filename(),
                    }
                });
                body = body.push(
                    row![text(slot.display_name).width(Length::Fill).size(12), picker]
                        .spacing(6),
                );
            }
        }

        body = body.push(text(rate).size(12));
        body = body.push(text(token_status).size(12));

        scrollable(body).height(Length::Fill).into()
    }

    fn modal_view(&self) -> Element<'_, Message> {
        match &self.modal {
            Modal::Closed => column![].into(),
            Modal::ClearCachedConfirm { tag, game_slug, planned } => {
                let copy = "These files live in this install's directory and only affect this version.";
                let mut col = column![
                    text(format!("Clear cached assets for {game_slug} {tag}?")).size(14),
                    text(copy).size(12),
                ]
                .spacing(6);
                for p in planned {
                    col = col.push(
                        text(format!("• {} ({} bytes)", p.path.display(), p.size)).size(12),
                    );
                }
                let tag_owned = tag.clone();
                col.push(
                    row![
                        button("Confirm").on_press(Message::ClearCachedAssetsConfirm(tag_owned)),
                        button("Cancel").on_press(Message::ClearCachedAssetsCancel),
                    ]
                    .spacing(6),
                )
                .into()
            }
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

fn section_header(s: &str) -> Element<'_, Message> {
    text(s).size(15).into()
}

fn game_for_slug(slug: &str) -> Option<&'static dyn Game> {
    games_mod::registry()
        .iter()
        .copied()
        .find(|g| g.slug() == slug)
}

fn display_name_for_slot(game_slug: &str, slot_id: &str) -> Option<String> {
    let game = game_for_slug(game_slug)?;
    let slot = game.slots().iter().find(|s| s.id == slot_id)?;
    Some(format!("{} / {}", game.display_name(), slot.display_name))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotChoice {
    filename: Option<String>,
}

impl SlotChoice {
    fn unassigned() -> Self {
        Self { filename: None }
    }
    fn filename(name: &str) -> Self {
        Self {
            filename: Some(name.to_string()),
        }
    }
    fn into_filename(self) -> Option<String> {
        self.filename
    }
}

impl std::fmt::Display for SlotChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.filename {
            Some(name) => f.write_str(name),
            None => f.write_str("(unassigned)"),
        }
    }
}

fn tab_button(label: &str, selected: bool, tab: Tab) -> Element<'_, Message> {
    let mut b = button(text(label));
    if !selected {
        b = b.on_press(Message::TabSelected(tab));
    }
    b.into()
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
        fn extract(&self, _archive: &Path, dest: &Path) -> anyhow::Result<()> {
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
        let client =
            Arc::new(github::Client::with_base(cache_path, _server.uri()).unwrap());

        let mut config = Config::default();
        config.library_root = Some(library_root.clone());

        let (mut app, _startup) = App::new(AppDeps {
            config,
            config_path,
            library_root: library_root.clone(),
            rom_library_root: dir.path().join("roms"),
            download_dir: download_dir.clone(),
            cache_dir: dir.path().to_path_buf(),
            game: static_game(),
            platform: static_platform(),
            client: client.clone(),
            startup_diagnostics: vec![],
        });

        let fetched = client
            .list_releases("fake/repo")
            .await
            .map_err(|e| e.to_string());
        let _ = app.update(Message::ReleasesLoaded(fetched));
        assert_eq!(app.releases.len(), 1);
        assert!(matches!(
            app.install_state("1.0.0"),
            Some(InstallState::Idle)
        ));

        let _task = app.update(Message::InstallClicked("1.0.0".into()));
        assert!(matches!(
            app.install_state("1.0.0"),
            Some(InstallState::Installing)
        ));
        let release = app.releases[0].clone();
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
        let args_file = library_root.join("1.0.0").join("args.txt");
        let args = std::fs::read_to_string(&args_file).expect("mock game should have run");
        // Launcher passes no extra args beyond what the binary itself sees.
        assert!(!args.contains("--baserompath"), "no rom flag should be passed");
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
            cache_dir: dir.path().to_path_buf(),
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
        let client =
            Arc::new(github::Client::with_base(dir.path().join("etags.json"), _server.uri()).unwrap());

        let mut config = Config::default();
        config.library_root = Some(library_root.clone());

        let (mut app, _startup) = App::new(AppDeps {
            config,
            config_path: config_path.clone(),
            library_root: library_root.clone(),
            rom_library_root: rom_library_root.clone(),
            download_dir: dir.path().join("dl"),
            cache_dir: dir.path().to_path_buf(),
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
        let _ = app.update(Message::ReleasesLoaded(fetched));
        let release = app.releases[0].clone();
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

        let install_dir = library_root.join("1.0.0");
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
        assert!(!install_dir.join("oot.z64").exists(), "symlink should be gone");
    }
}
