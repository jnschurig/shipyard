use iced::widget::{button, column, container, pick_list, row, scrollable, text, text_input};
use iced::{Element, Length};

use crate::app::{App, Message};
use crate::config::schema::ThemePreference;
use crate::ui::{TABLE_ROW_PADDING, table_card_style, table_row_separator};

const THEME_OPTIONS: [ThemePreference; 3] = [
    ThemePreference::Dark,
    ThemePreference::Light,
    ThemePreference::System,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ThemeOption(ThemePreference);

impl std::fmt::Display for ThemeOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self.0 {
            ThemePreference::Dark => "Dark",
            ThemePreference::Light => "Light",
            ThemePreference::System => "System",
        };
        f.write_str(s)
    }
}

const SETTINGS_LABEL_WIDTH: f32 = 200.0;

impl App {
    pub(crate) fn mods_view(&self) -> Element<'_, Message> {
        container(text("Mod management is coming soon™.").size(16))
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    pub(crate) fn settings_view(&self) -> Element<'_, Message> {
        let rate = match (
            self.rate_limit.remaining,
            self.rate_limit.limit,
            self.rate_limit.reset_at,
        ) {
            (Some(r), Some(lim), Some(reset)) => format!(
                "{r}/{lim}, resets at {}",
                reset.with_timezone(&chrono::Local).format("%H:%M:%S")
            ),
            (Some(r), Some(lim), None) => format!("{r}/{lim}"),
            (Some(r), None, _) => format!("{r} remaining"),
            _ => "unknown".to_string(),
        };
        let token_status = if std::env::var("GITHUB_TOKEN").is_ok_and(|v| !v.is_empty()) {
            "set"
        } else {
            "not set"
        };

        let theme_options: Vec<ThemeOption> =
            THEME_OPTIONS.iter().copied().map(ThemeOption).collect();
        let theme_pick = pick_list(theme_options, Some(ThemeOption(self.config.theme)), |opt| {
            Message::ThemeChanged(opt.0)
        })
        .width(Length::Fixed(160.0));

        let general_card = container(
            column![
                settings_row("Theme", theme_pick.into()),
                table_row_separator(),
                settings_row(
                    "Versions to show",
                    row![
                        text_input("10", &self.versions_to_show_input)
                            .on_input(Message::VersionsToShowInputChanged)
                            .on_submit(Message::VersionsToShowSubmit)
                            .width(Length::Fixed(120.0)),
                        button("Apply").on_press(Message::VersionsToShowSubmit),
                    ]
                    .spacing(6)
                    .into(),
                ),
                table_row_separator(),
                settings_row(
                    "Library root",
                    column![
                        text_input("path", &self.library_root_input)
                            .on_input(Message::LibraryRootInputChanged),
                        text("Existing installs are not moved when you change this.").size(11),
                    ]
                    .spacing(4)
                    .into(),
                ),
            ]
            .spacing(0),
        )
        .width(Length::Fill)
        .style(table_card_style);

        let github_card = container(
            column![
                settings_row("GitHub rate limit", text(rate).size(13).into()),
                table_row_separator(),
                settings_row("GITHUB_TOKEN", text(token_status).size(13).into()),
            ]
            .spacing(0),
        )
        .width(Length::Fill)
        .style(table_card_style);

        let body: iced::widget::Column<'_, Message> = column![
            general_card,
            row![button("Save").on_press(Message::SaveSettings)].spacing(6),
            super::section_header("GitHub"),
            github_card,
        ]
        .spacing(12);

        scrollable(body).height(Length::Fill).into()
    }
}

fn settings_row<'a>(label: &'a str, control: Element<'a, Message>) -> Element<'a, Message> {
    container(
        row![
            text(label)
                .size(13)
                .width(Length::Fixed(SETTINGS_LABEL_WIDTH)),
            container(control).width(Length::Fill),
        ]
        .spacing(12)
        .align_y(iced::Alignment::Center),
    )
    .padding(TABLE_ROW_PADDING)
    .into()
}
