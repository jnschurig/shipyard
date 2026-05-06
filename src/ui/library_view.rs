use iced::widget::{button, column, container, pick_list, row, scrollable, text};
use iced::{Element, Length};

use crate::app::{App, InstallState, Message};
use crate::config::schema::MIN_VERSIONS_TO_SHOW;
use crate::games as games_mod;

const GEAR_MENU_BUTTON_WIDTH: f32 = 210.0;
const PRIMARY_ACTION_BUTTON_WIDTH: f32 = 140.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GameChoice {
    slug: String,
    display_name: String,
}

impl std::fmt::Display for GameChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.display_name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VersionChoice {
    pub tag: String,
    installed: bool,
    latest: bool,
}

impl std::fmt::Display for VersionChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.tag)?;
        if self.latest {
            f.write_str(" (latest)")?;
        }
        if self.installed {
            f.write_str(" ✓")?;
        }
        Ok(())
    }
}

impl App {
    pub(crate) fn library_view(&self) -> Element<'_, Message> {
        let mut games_with_sort: Vec<(&'static str, GameChoice)> = games_mod::registry()
            .iter()
            .map(|g| {
                (
                    g.sort_name(),
                    GameChoice {
                        slug: g.slug().to_string(),
                        display_name: g.display_name().to_string(),
                    },
                )
            })
            .collect();
        games_with_sort.sort_by_key(|(s, _)| s.to_ascii_lowercase());
        let games: Vec<GameChoice> = games_with_sort.into_iter().map(|(_, c)| c).collect();
        let selected_game = games
            .iter()
            .find(|g| g.slug == self.selected_game_slug)
            .cloned()
            .or_else(|| games.first().cloned());

        let versions = self.versions_for_selected_game();
        let selected_version = self
            .selected_tag
            .as_ref()
            .and_then(|t| versions.iter().find(|v| v.tag == *t).cloned())
            .or_else(|| versions.first().cloned());

        let game_pick = pick_list(games, selected_game, |g: GameChoice| {
            Message::GameSelected(g.slug)
        });
        let version_pick = pick_list(
            versions.clone(),
            selected_version.clone(),
            |v: VersionChoice| Message::VersionSelected(v.tag),
        );

        let primary_label: String = match (&selected_version, self.selected_install_state()) {
            (Some(v), Some(InstallState::Installing)) => {
                match self.install_progress.get(&v.tag).copied().flatten() {
                    Some(pct) => format!("Installing… {pct}%"),
                    None => "Installing…".to_string(),
                }
            }
            (Some(v), _) if v.installed => "Launch".to_string(),
            (Some(_), _) => "Install".to_string(),
            (None, _) => "Launch".to_string(),
        };
        let mut primary =
            button(text(primary_label).size(14)).width(Length::Fixed(PRIMARY_ACTION_BUTTON_WIDTH));
        let installing = matches!(
            self.selected_install_state(),
            Some(InstallState::Installing)
        );
        if !installing && selected_version.is_some() {
            primary = primary.on_press(Message::PrimaryActionClicked);
        }

        let gear = button(text("⚙").size(16)).on_press(Message::ToggleGearMenu);

        let top_row = row![game_pick, version_pick, primary, gear].spacing(8);

        let mut body: iced::widget::Column<'_, Message> =
            column![text(self.game_title_label()).size(20), top_row].spacing(12);

        if self.gear_menu_open {
            body = body.push(self.gear_menu());
        }

        scrollable(body).height(Length::Fill).into()
    }

    fn gear_menu(&self) -> Element<'_, Message> {
        let installed = self.selected_install_state() == Some(InstallState::Installed);
        let tooltip_text = "Available only for installed versions.";

        let clear_btn = if installed {
            button(text("Clear Cache"))
                .width(Length::Fixed(GEAR_MENU_BUTTON_WIDTH))
                .on_press(Message::ClearCacheSelectedClicked)
        } else {
            button(text("Clear Cache")).width(Length::Fixed(GEAR_MENU_BUTTON_WIDTH))
        };
        let uninstall_btn = if installed {
            button(text("Uninstall"))
                .width(Length::Fixed(GEAR_MENU_BUTTON_WIDTH))
                .on_press(Message::UninstallSelectedClicked)
        } else {
            button(text("Uninstall")).width(Length::Fixed(GEAR_MENU_BUTTON_WIDTH))
        };
        let refresh_btn = button(text("Check for new versions"))
            .width(Length::Fixed(GEAR_MENU_BUTTON_WIDTH))
            .on_press(Message::ManualRefreshClicked);

        let clear_el: Element<_> = if installed {
            clear_btn.into()
        } else {
            iced::widget::tooltip(
                clear_btn,
                container(text(tooltip_text).size(12)).padding(6),
                iced::widget::tooltip::Position::Right,
            )
            .into()
        };
        let uninstall_el: Element<_> = if installed {
            uninstall_btn.into()
        } else {
            iced::widget::tooltip(
                uninstall_btn,
                container(text(tooltip_text).size(12)).padding(6),
                iced::widget::tooltip::Position::Right,
            )
            .into()
        };

        container(column![clear_el, uninstall_el, refresh_btn].spacing(4))
            .padding(8)
            .style(|theme: &iced::Theme| {
                let palette = theme.extended_palette();
                container::Style {
                    background: Some(iced::Background::Color(palette.background.weak.color)),
                    border: iced::Border {
                        color: palette.background.strong.color,
                        width: 1.0,
                        radius: 6.0.into(),
                    },
                    ..container::Style::default()
                }
            })
            .into()
    }

    fn game_title_label(&self) -> String {
        crate::app::game_for_slug(&self.selected_game_slug)
            .map(|g| g.display_name().to_string())
            .unwrap_or_else(|| "Shipyard".to_string())
    }

    pub(crate) fn selected_install_state(&self) -> Option<InstallState> {
        let tag = self.selected_tag.as_ref()?;
        self.install_states.get(tag).cloned()
    }

    /// Compute the version list for the Library dropdown: most recent N
    /// releases plus any installed version older than that window. Sorted with
    /// the latest first (matches GitHub's own ordering).
    pub(crate) fn versions_for_selected_game(&self) -> Vec<VersionChoice> {
        let n = self.config.versions_to_show.max(MIN_VERSIONS_TO_SHOW) as usize;
        let recent_tags: Vec<&str> = self
            .releases
            .iter()
            .take(n)
            .map(|r| r.tag_name.as_str())
            .collect();
        let mut tags: Vec<String> = recent_tags.iter().map(|s| s.to_string()).collect();
        for inst in &self.installed {
            if inst.game_slug == self.selected_game_slug && !tags.iter().any(|t| t == &inst.tag) {
                tags.push(inst.tag.clone());
            }
        }
        let latest_tag = self.releases.first().map(|r| r.tag_name.clone());
        tags.into_iter()
            .map(|tag| {
                let installed = self
                    .installed
                    .iter()
                    .any(|v| v.tag == tag && v.game_slug == self.selected_game_slug);
                let latest = latest_tag.as_ref() == Some(&tag);
                VersionChoice {
                    tag,
                    installed,
                    latest,
                }
            })
            .collect()
    }
}
