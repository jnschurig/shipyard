use iced::widget::{button, column, pick_list, row, scrollable, text};
use iced::{Element, Length};

use crate::app::{App, Message};
use crate::games as games_mod;

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
                for r in &self.roms {
                    let filename = r.filename.clone();
                    body = body.push(
                        row![
                            text(r.filename.clone()).width(Length::Fill).size(12),
                            button(text("✕")).on_press(Message::DeleteRomClicked(filename)),
                        ]
                        .spacing(6),
                    );
                }
            }
        }

        body = body.push(super::section_header("Slot Assignments"));
        for game in games_mod::registry() {
            body = body.push(text(game.rom_group_name()).size(14));
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
                    row![text(slot.display_name).width(Length::Fill).size(12), picker].spacing(6),
                );
            }
        }

        scrollable(body).height(Length::Fill).into()
    }
}
