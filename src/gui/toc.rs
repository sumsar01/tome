use egui::{RichText, ScrollArea, Ui};

use super::app::GuiApp;

/// Draw the table-of-contents right sidebar.
pub fn show(ui: &mut Ui, app: &mut GuiApp) {
    let theme = app.gui_theme.clone();

    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label(
            RichText::new("Contents")
                .color(theme.fg_dim)
                .size(11.0)
                .strong(),
        );
    });
    ui.add_space(4.0);
    ui.separator();
    ui.add_space(4.0);

    if app.toc.is_empty() {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            ui.label(
                RichText::new("No headings")
                    .color(theme.fg_dim)
                    .size(11.0)
                    .italics(),
            );
        });
        return;
    }

    ScrollArea::vertical()
        .id_salt("toc_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (idx, (level, text)) in app.toc.iter().enumerate() {
                let indent = (*level as f32 - 1.0) * 10.0;
                let size = match level {
                    1 => 13.5,
                    2 => 13.0,
                    _ => 12.0,
                };
                let color = match level {
                    1 => theme.accent,
                    2 => theme.fg,
                    _ => theme.fg_dim,
                };

                ui.horizontal(|ui| {
                    ui.add_space(8.0 + indent);
                    let label = ui.add(
                        egui::Label::new(RichText::new(text.as_str()).color(color).size(size))
                            .sense(egui::Sense::click())
                            .truncate(),
                    );

                    if label.clicked() {
                        app.toc_scroll_to = Some(idx);
                    }

                    if label.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                });
                ui.add_space(2.0);
            }
        });
}
