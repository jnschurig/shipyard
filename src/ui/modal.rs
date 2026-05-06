use iced::widget::{button, column, row, text};
use iced::{Element, Length};

use crate::app::{App, Message, Modal};

const MODAL_BUTTON_WIDTH: f32 = 96.0;

impl App {
    pub(crate) fn modal_view(&self) -> Element<'_, Message> {
        match &self.modal {
            Modal::Closed => column![].into(),
            Modal::ClearCachedConfirm {
                tag,
                game_slug,
                planned,
            } => {
                let copy =
                    "These files live in this install's directory and only affect this version.";
                let mut col = column![
                    text(format!("Clear cached assets for {game_slug} {tag}?")).size(14),
                    text(copy).size(12),
                ]
                .spacing(6);
                for p in planned {
                    col = col
                        .push(text(format!("• {} ({} bytes)", p.path.display(), p.size)).size(12));
                }
                let tag_owned = tag.clone();
                col.push(
                    row![
                        button("Confirm")
                            .width(Length::Fixed(MODAL_BUTTON_WIDTH))
                            .on_press(Message::ClearCachedAssetsConfirm(tag_owned)),
                        button("Cancel")
                            .width(Length::Fixed(MODAL_BUTTON_WIDTH))
                            .on_press(Message::ClearCachedAssetsCancel),
                    ]
                    .spacing(6),
                )
                .into()
            }
            Modal::DeleteRomConfirm { filename } => {
                let f = filename.clone();
                column![
                    text(format!("Delete ROM \"{filename}\"?")).size(14),
                    text("This removes the file from your ROM library and clears any slot assignments referencing it.")
                        .size(12),
                    row![
                        button("Delete")
                            .width(Length::Fixed(MODAL_BUTTON_WIDTH))
                            .on_press(Message::DeleteRomConfirm(f)),
                        button("Cancel")
                            .width(Length::Fixed(MODAL_BUTTON_WIDTH))
                            .on_press(Message::DeleteRomCancel),
                    ]
                    .spacing(6),
                ]
                .spacing(6)
                .into()
            }
            Modal::UninstallConfirm { tag } => {
                let t = tag.clone();
                column![
                    text(format!("Uninstall {tag}?")).size(14),
                    text("This deletes the install directory.").size(12),
                    row![
                        button("Uninstall")
                            .width(Length::Fixed(MODAL_BUTTON_WIDTH))
                            .on_press(Message::UninstallClicked(t)),
                        button("Cancel")
                            .width(Length::Fixed(MODAL_BUTTON_WIDTH))
                            .on_press(Message::ClearCachedAssetsCancel),
                    ]
                    .spacing(6),
                ]
                .spacing(6)
                .into()
            }
        }
    }
}
