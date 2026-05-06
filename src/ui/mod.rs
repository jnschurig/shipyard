//! Per-screen view rendering for the GUI. Each submodule owns the rendering
//! of one tab/screen as `impl App` blocks. App state, message routing, and
//! `update` live in `crate::app`.

pub mod library_view;
pub mod modal;
pub mod roms_view;
pub mod settings_view;

use iced::widget::{button, text};
use iced::{Element, Length};

use crate::app::{Message, Tab};

pub(crate) const TAB_BUTTON_WIDTH: f32 = 90.0;

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
