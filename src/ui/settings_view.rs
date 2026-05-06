use iced::widget::{button, column, container, row, scrollable, text, text_input};
use iced::{Element, Length};

use crate::app::{App, Message};

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
                "GitHub rate limit: {r}/{lim}, resets at {}",
                reset.with_timezone(&chrono::Local).format("%H:%M:%S")
            ),
            (Some(r), Some(lim), None) => format!("GitHub rate limit: {r}/{lim}"),
            (Some(r), None, _) => format!("GitHub rate limit: {r} remaining"),
            _ => "GitHub rate limit: unknown".to_string(),
        };
        let token_status = if std::env::var("GITHUB_TOKEN").is_ok_and(|v| !v.is_empty()) {
            "GITHUB_TOKEN: set"
        } else {
            "GITHUB_TOKEN: not set"
        };

        let body: iced::widget::Column<'_, Message> = column![
            text("Versions to show").size(14),
            row![
                text_input("10", &self.versions_to_show_input)
                    .on_input(Message::VersionsToShowInputChanged)
                    .on_submit(Message::VersionsToShowSubmit)
                    .width(Length::Fixed(120.0)),
                button("Apply").on_press(Message::VersionsToShowSubmit),
            ]
            .spacing(6),
            text("Library root").size(14),
            text_input("path", &self.library_root_input).on_input(Message::LibraryRootInputChanged),
            text("Existing installs are not moved when you change this.").size(11),
            row![button("Save").on_press(Message::SaveSettings)].spacing(6),
            super::section_header("GitHub"),
            text(rate).size(12),
            text(token_status).size(12),
        ]
        .spacing(8);

        scrollable(body).height(Length::Fill).into()
    }
}
