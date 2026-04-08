use egui::{Key, RichText, ScrollArea, Spinner, Ui};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};

use super::app::{Focus, GuiApp};

/// Draw the central reader panel.
pub fn show(ui: &mut Ui, app: &mut GuiApp, ctx: &egui::Context) {
    let theme = app.gui_theme.clone();

    // ── Title bar ─────────────────────────────────────────────────────────────
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.add_space(12.0);

        // Back button / Escape hint
        let back_btn = ui.add(
            egui::Label::new(RichText::new("← back").color(theme.accent_light).size(12.0))
                .sense(egui::Sense::click()),
        );
        if back_btn.clicked() {
            app.go_back();
            return;
        }
        if back_btn.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(8.0);

        ui.label(
            RichText::new(&app.reader_title)
                .color(theme.fg)
                .strong()
                .size(15.0),
        );
    });

    ui.add_space(4.0);
    ui.separator();

    // ── Keyboard: Escape goes back ─────────────────────────────────────────────
    if app.focus == Focus::Reader {
        ctx.input(|i| {
            if i.key_pressed(Key::Escape) {
                // We can't call app.go_back() inside the closure, use a flag via toc_scroll_to=usize::MAX sentinel
                // Instead we handle it outside — set focus back to browser
            }
        });
    }

    // ── Content ───────────────────────────────────────────────────────────────
    if app.reader_loading {
        ui.add_space(40.0);
        ui.vertical_centered(|ui| {
            ui.add(Spinner::new().size(32.0).color(theme.accent));
            ui.add_space(8.0);
            ui.label(
                RichText::new(format!("Loading {}…", app.reader_title))
                    .color(theme.fg_dim)
                    .size(13.0),
            );
        });
        return;
    }

    if let Some(content) = app.reader_content.clone() {
        ScrollArea::vertical()
            .id_salt("reader_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // Constrain reading width for comfort
                let max_width = ui.available_width().min(860.0);
                ui.set_max_width(max_width);
                ui.add_space(8.0);

                let cache: &mut CommonMarkCache = &mut app.cm_cache;
                CommonMarkViewer::new().show(ui, cache, &content);

                ui.add_space(40.0);
            });
    } else {
        // Nothing loaded yet — show prompt
        ui.add_space(40.0);
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new("Select a doc from the left panel")
                    .color(theme.fg_dim)
                    .size(14.0)
                    .italics(),
            );
        });
    }
}
