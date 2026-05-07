use iced::widget::{button, column, container, pick_list, row, scrollable, text};
use iced::{Element, Length};

use crate::app::{App, Message};
use crate::games as games_mod;
use crate::ui::{TABLE_ROW_PADDING, table_card_style, table_row_separator};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SlotChoice {
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

impl App {
    pub(crate) fn roms_view(&self) -> Element<'_, Message> {
        let import_btn = button(text("Import Rom")).on_press(Message::ImportRomClicked);
        let mut body: iced::widget::Column<'_, Message> = column![import_btn].spacing(12);

        let expander_label = if self.imported_roms_expanded {
            format!("▾ Imported Roms ({})", self.roms.len())
        } else {
            format!("▸ Imported Roms ({})", self.roms.len())
        };
        body = body.push(
            button(text(expander_label).size(14)).on_press(Message::ToggleImportedRomsExpander),
        );

        if self.imported_roms_expanded {
            if self.roms.is_empty() {
                body = body.push(text("(none)").size(12));
            } else {
                let mut roms_table: iced::widget::Column<'_, Message> = column![].spacing(0);
                let mut first = true;
                for r in &self.roms {
                    if !first {
                        roms_table = roms_table.push(table_row_separator());
                    }
                    first = false;
                    let filename = r.filename.clone();
                    let delete_btn = button(text("X").size(14).font(iced::Font {
                        weight: iced::font::Weight::Bold,
                        ..iced::Font::DEFAULT
                    }))
                    .on_press(Message::DeleteRomClicked(filename))
                    .style(|theme: &iced::Theme, status| {
                        let palette = theme.extended_palette();
                        let bg = match status {
                            button::Status::Hovered | button::Status::Pressed => iced::Color {
                                r: 0.75,
                                g: 0.18,
                                b: 0.18,
                                a: 1.0,
                            },
                            _ => iced::Color {
                                r: 0.65,
                                g: 0.13,
                                b: 0.13,
                                a: 1.0,
                            },
                        };
                        button::Style {
                            background: Some(iced::Background::Color(bg)),
                            text_color: iced::Color::WHITE,
                            border: iced::Border {
                                color: palette.background.strong.color,
                                width: 0.0,
                                radius: 4.0.into(),
                            },
                            ..button::Style::default()
                        }
                    });
                    let row_el = row![
                        text(r.filename.clone()).width(Length::Fill).size(13),
                        delete_btn,
                    ]
                    .spacing(12)
                    .align_y(iced::Alignment::Center);
                    roms_table = roms_table.push(container(row_el).padding(TABLE_ROW_PADDING));
                }
                body = body.push(
                    container(roms_table)
                        .width(Length::Fill)
                        .style(table_card_style),
                );
            }
        }

        body = body.push(super::section_header("Slot Assignments"));

        let mut table: iced::widget::Column<'_, Message> = column![].spacing(0);
        let mut first = true;
        for game in games_mod::registry() {
            for slot in game.slots() {
                if !first {
                    table = table.push(table_row_separator());
                }
                first = false;

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
                })
                .width(Length::Fixed(360.0));

                let label = text(slot.display_name).size(13).width(Length::Fixed(240.0));
                let row_el = row![label, text("").width(Length::Fill), picker]
                    .spacing(12)
                    .align_y(iced::Alignment::Center);
                table = table.push(container(row_el).padding(TABLE_ROW_PADDING));
            }
        }

        let table_card = container(table).width(Length::Fill).style(table_card_style);
        body = body.push(table_card);

        scrollable(body).height(Length::Fill).into()
    }
}
