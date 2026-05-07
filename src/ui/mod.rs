//! Per-screen view rendering for the GUI. Each submodule owns the rendering
//! of one tab/screen as `impl App` blocks. App state, message routing, and
//! `update` live in `crate::app`.

pub mod library_view;
pub mod modal;
pub mod roms_view;
pub mod settings_view;

use iced::widget::{button, container, horizontal_rule, text};
use iced::{Element, Length};

use crate::app::{Message, Tab};

pub(crate) const TAB_BUTTON_WIDTH: f32 = 90.0;
pub(crate) const TABLE_ROW_PADDING: [u16; 2] = [8, 12];

pub(crate) fn tab_button(label: &str, selected: bool, tab: Tab) -> Element<'_, Message> {
    let mut b = button(text(label)).width(Length::Fixed(TAB_BUTTON_WIDTH));
    if !selected {
        b = b.on_press(Message::TabSelected(tab));
    }
    b.into()
}

pub(crate) fn section_header(s: &str) -> Element<'_, Message> {
    text(s).size(15).into()
}

/// Container style used by the bordered/tinted "card" tables across the app
/// (library view, slot assignments, imported ROMs).
pub(crate) fn table_card_style(theme: &iced::Theme) -> iced::widget::container::Style {
    let palette = theme.extended_palette();
    let base = palette.background.base.color;
    let delta = if palette.is_dark { 0.04 } else { 0.06 };
    let bg = iced::Color {
        r: (base.r - delta).max(0.0),
        g: (base.g - delta).max(0.0),
        b: (base.b - delta).max(0.0),
        a: 1.0,
    };
    iced::widget::container::Style {
        background: Some(iced::Background::Color(bg)),
        border: iced::Border {
            color: palette.background.strong.color,
            width: 1.0,
            radius: 6.0.into(),
        },
        ..iced::widget::container::Style::default()
    }
}

/// Horizontal separator between rows inside a table card.
pub(crate) fn table_row_separator<'a>() -> Element<'a, Message> {
    container(horizontal_rule(1)).padding([0, 8]).into()
}
