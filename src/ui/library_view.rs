use iced::widget::{button, column, container, pick_list, row, scrollable, text};
use iced::{Element, Length};

use crate::app::{App, InstallState, Message};
use crate::config::schema::MIN_VERSIONS_TO_SHOW;
use crate::games::{self as games_mod, Game};

const GEAR_MENU_BUTTON_WIDTH: f32 = 210.0;
const PRIMARY_ACTION_BUTTON_WIDTH: f32 = 140.0;
const GAME_LABEL_WIDTH: f32 = 220.0;
const VERSION_PICK_WIDTH: f32 = 240.0;
// Sum of widgets in the action row (label + version + primary + ~gear)
// plus the 8px spacings between them. Used to right-anchor the gear menu
// directly under the gear button regardless of card width.
const ACTION_ROW_WIDTH: f32 = GAME_LABEL_WIDTH + VERSION_PICK_WIDTH + PRIMARY_ACTION_BUTTON_WIDTH
    + 40.0  // gear button natural width
    + 3.0 * 8.0;

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
        let mut games: Vec<&'static dyn Game> = games_mod::registry().to_vec();
        games.sort_by_key(|g| g.sort_name().to_ascii_lowercase());

        let mut table: iced::widget::Column<'_, Message> = column![].spacing(0);
        let mut first = true;
        for game in games {
            if !first {
                table = table.push(crate::ui::table_row_separator());
            }
            first = false;
            table = table.push(self.library_row(game));
        }

        let table_card = container(table)
            .width(Length::Fixed(ACTION_ROW_WIDTH + 24.0))
            .style(crate::ui::table_card_style);

        scrollable(column![table_card].spacing(12))
            .height(Length::Fill)
            .into()
    }

    fn library_row(&self, game: &'static dyn Game) -> Element<'_, Message> {
        let slug = game.slug().to_string();
        let versions = self.versions_for_game(game.slug());
        let selected_version = self
            .selected_tag_for(game.slug())
            .and_then(|t| versions.iter().find(|v| v.tag == t).cloned())
            .or_else(|| versions.first().cloned());

        let install_state = self.install_state_for_game(game.slug());
        let primary_label: String = match (&selected_version, &install_state) {
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

        let slug_for_v = slug.clone();
        let version_pick = pick_list(
            versions,
            selected_version.clone(),
            move |v: VersionChoice| Message::VersionSelected {
                game_slug: slug_for_v.clone(),
                tag: v.tag,
            },
        )
        .width(Length::Fixed(VERSION_PICK_WIDTH));

        let mut primary =
            button(text(primary_label).size(14)).width(Length::Fixed(PRIMARY_ACTION_BUTTON_WIDTH));
        let installing = matches!(install_state, Some(InstallState::Installing));
        if !installing && selected_version.is_some() {
            primary = primary.on_press(Message::PrimaryActionClicked(slug.clone()));
        }
        let gear = button(text("⚙").size(16)).on_press(Message::ToggleGearMenu(slug.clone()));

        let label = text(game.display_name())
            .size(14)
            .width(Length::Fixed(GAME_LABEL_WIDTH));
        let row_el = row![label, version_pick, primary, gear]
            .spacing(8)
            .align_y(iced::Alignment::Center);

        let mut col: iced::widget::Column<'_, Message> =
            column![container(row_el).padding([8, 12])];
        if self.gear_menu_open_for_game.as_deref() == Some(game.slug()) {
            let anchored = container(self.gear_menu_for(&slug))
                .width(Length::Fixed(ACTION_ROW_WIDTH))
                .align_x(iced::alignment::Horizontal::Right);
            col = col.push(container(anchored).padding(iced::Padding {
                top: 0.0,
                right: 12.0,
                bottom: 8.0,
                left: 12.0,
            }));
        }
        col.into()
    }

    fn gear_menu_for(&self, slug: &str) -> Element<'_, Message> {
        let installed = self.install_state_for_game(slug) == Some(InstallState::Installed);
        let tooltip_text = "Available only for installed versions.";

        let clear_btn = if installed {
            button(text("Clear Cache"))
                .width(Length::Fixed(GEAR_MENU_BUTTON_WIDTH))
                .on_press(Message::ClearCacheSelectedClicked(slug.to_string()))
        } else {
            button(text("Clear Cache")).width(Length::Fixed(GEAR_MENU_BUTTON_WIDTH))
        };
        let uninstall_btn = if installed {
            button(text("Uninstall"))
                .width(Length::Fixed(GEAR_MENU_BUTTON_WIDTH))
                .on_press(Message::UninstallSelectedClicked(slug.to_string()))
        } else {
            button(text("Uninstall")).width(Length::Fixed(GEAR_MENU_BUTTON_WIDTH))
        };
        let refresh_btn = button(text("Check for new versions"))
            .width(Length::Fixed(GEAR_MENU_BUTTON_WIDTH))
            .on_press(Message::ManualRefreshClicked(slug.to_string()));

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

    /// Compute the version list for a game's row dropdown: most recent N
    /// releases plus any installed version older than that window. Sorted
    /// with the latest first (matches GitHub's own ordering).
    pub(crate) fn versions_for_game(&self, slug: &str) -> Vec<VersionChoice> {
        let n = self.config.versions_to_show.max(MIN_VERSIONS_TO_SHOW) as usize;
        let releases = self.releases_for(slug);
        let recent_tags: Vec<&str> = releases
            .iter()
            .take(n)
            .map(|r| r.tag_name.as_str())
            .collect();
        let mut tags: Vec<String> = recent_tags.iter().map(|s| s.to_string()).collect();
        for inst in &self.installed {
            if inst.game_slug == slug && !tags.iter().any(|t| t == &inst.tag) {
                tags.push(inst.tag.clone());
            }
        }
        let latest_tag = releases.first().map(|r| r.tag_name.clone());
        tags.into_iter()
            .map(|tag| {
                let installed = self
                    .installed
                    .iter()
                    .any(|v| v.tag == tag && v.game_slug == slug);
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
