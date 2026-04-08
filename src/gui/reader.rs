use egui::{RichText, ScrollArea, Ui};
use egui_commonmark::CommonMarkViewer;

use super::app::{Focus, GuiApp};

/// Draw the central reader panel.
pub fn show(ui: &mut Ui, app: &mut GuiApp, ctx: &egui::Context) {
    let theme = app.gui_theme.clone();

    // ── Nothing loaded — welcome prompt ───────────────────────────────────────
    if !app.reader_is_open() {
        ui.add_space(ui.available_height() / 3.0);
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new("select a doc  ·  j/k to navigate  ·  Enter to open")
                    .color(theme.fg_dim)
                    .size(13.0)
                    .italics(),
            );
        });
        return;
    }

    // ── Title bar ─────────────────────────────────────────────────────────────
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.add_space(12.0);

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

    // ── Loading spinner ────────────────────────────────────────────────────────
    if app.reader_loading {
        ui.add_space(40.0);
        ui.vertical_centered(|ui| {
            ui.add(egui::Spinner::new().size(32.0).color(theme.accent));
            ui.add_space(8.0);
            ui.label(
                RichText::new(format!("Loading {}…", app.reader_title))
                    .color(theme.fg_dim)
                    .size(13.0),
            );
        });
        return;
    }

    // ── Content ───────────────────────────────────────────────────────────────
    if let Some(content) = app.reader_content.clone() {
        // Compute scroll offset to apply this frame.
        let mut offset = app.reader_scroll_offset + app.reader_scroll_delta;
        app.reader_scroll_delta = 0.0;

        let scroll_to_top = app.reader_scroll_to_top;
        let scroll_to_bottom = app.reader_scroll_to_bottom;
        app.reader_scroll_to_top = false;
        app.reader_scroll_to_bottom = false;

        if scroll_to_top {
            offset = 0.0;
        }

        let mut scroll = ScrollArea::vertical()
            .id_salt("reader_scroll")
            .auto_shrink([false, false]);

        if scroll_to_top || scroll_to_bottom || app.reader_scroll_delta != 0.0 {
            // Will be set below after we know max scroll
            scroll = scroll.vertical_scroll_offset(offset);
        } else if offset != app.reader_scroll_offset {
            scroll = scroll.vertical_scroll_offset(offset);
        }

        let output = scroll.show(ui, |ui| {
            let max_width = ui.available_width().min(860.0);
            ui.set_max_width(max_width);
            ui.add_space(8.0);

            CommonMarkViewer::new().show(ui, &mut app.cm_cache, &content);

            ui.add_space(40.0);

            // Expose content height via a zero-size rect at the bottom
            ui.min_rect().max.y
        });

        // Track current scroll offset
        app.reader_scroll_offset = output.state.offset.y;

        // Apply scroll-to-bottom (needs content height)
        if scroll_to_bottom {
            let content_height = output.inner;
            let visible_height = output.inner_rect.height();
            let max_offset = (content_height - visible_height).max(0.0);
            // Schedule offset for next frame
            app.reader_scroll_offset = max_offset;
            app.reader_scroll_delta = 0.0;
            ctx.request_repaint();
        }

        // Focus reader on click inside it
        if ui.rect_contains_pointer(output.inner_rect) {
            if ctx.input(|i| i.pointer.any_click()) {
                app.focus = Focus::Reader;
            }
        }
    }
}
