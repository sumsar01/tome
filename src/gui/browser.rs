use egui::{Color32, CornerRadius, Frame, Margin, RichText, ScrollArea, Sense, TextEdit, Ui};

use super::app::{Focus, GuiApp};

/// Draw the left-side browser panel: logo, filter bar, doc list.
pub fn show(ui: &mut Ui, app: &mut GuiApp, ctx: &egui::Context) {
    let theme = app.gui_theme.clone();

    // ── Logo / header ──────────────────────────────────────────────────────────
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.add_space(12.0);
        ui.vertical(|ui| {
            ui.label(RichText::new("  ,___,").color(theme.accent).monospace());
            ui.horizontal(|ui| {
                ui.label(RichText::new(" (o,o)").color(theme.accent).monospace());
                ui.add_space(4.0);
                ui.label(RichText::new("t o m e").color(theme.fg).strong().size(15.0));
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new(" {`\"'}").color(theme.accent).monospace());
                ui.add_space(4.0);
                ui.label(
                    RichText::new("docs for humans & AI")
                        .color(theme.fg_dim)
                        .size(11.0),
                );
            });
            ui.label(RichText::new(" -\"-\"-").color(theme.accent).monospace());
        });
    });

    ui.add_space(8.0);
    ui.separator();
    ui.add_space(6.0);

    // ── Filter bar ────────────────────────────────────────────────────────────
    let filter_focused = app.focus == Focus::Filter;
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label(RichText::new("/").color(theme.filter).strong());
        ui.add_space(2.0);
        let response = ui.add(
            TextEdit::singleline(&mut app.filter)
                .hint_text("filter…")
                .desired_width(f32::INFINITY)
                .frame(egui::Frame::NONE)
                .text_color(theme.filter),
        );
        if filter_focused {
            response.request_focus();
        }
        if response.gained_focus() {
            app.focus = Focus::Filter;
        }
        // Lost focus: only relinquish if focus wasn't explicitly taken by
        // another widget — check that we're not switching to Filter from here.
        if response.lost_focus() && app.focus == Focus::Filter {
            app.focus = Focus::Browser;
        }
    });

    ui.add_space(6.0);
    ui.separator();
    ui.add_space(4.0);

    // ── Doc count label ────────────────────────────────────────────────────────
    let aliases = app.filtered_aliases();
    let total = app.doc_aliases.len();
    let shown = aliases.len();
    let count_text = if shown < total {
        format!("{}/{} docs", shown, total)
    } else {
        format!("{} docs", total)
    };
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label(RichText::new(count_text).color(theme.fg_dim).size(11.0));
    });
    ui.add_space(4.0);

    let current_alias = app.reader_alias.clone();

    // Clone alias list to satisfy borrow checker
    let aliases: Vec<String> = aliases.iter().map(|s| s.to_string()).collect();

    // ── Doc list ──────────────────────────────────────────────────────────────
    ScrollArea::vertical()
        .id_salt("browser_list")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (i, alias) in aliases.iter().enumerate() {
                let is_selected = app.selected == Some(i);
                let is_open = current_alias.as_deref() == Some(alias.as_str());

                let row_bg = if is_selected {
                    theme.accent.linear_multiply(0.20)
                } else if is_open {
                    theme.accent.linear_multiply(0.10)
                } else {
                    Color32::TRANSPARENT
                };

                let text_color = if is_selected {
                    theme.fg
                } else if is_open {
                    theme.accent_light
                } else {
                    theme.fg_dim
                };

                let row = Frame::new()
                    .fill(row_bg)
                    .inner_margin(Margin::symmetric(8, 3))
                    .corner_radius(CornerRadius::same(4))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.add(
                            egui::Label::new(
                                RichText::new(alias.as_str()).color(text_color).size(13.0),
                            )
                            .sense(Sense::click())
                            .truncate(),
                        )
                    });

                let response = row.inner;

                // Scroll selected item into view
                if is_selected {
                    response.scroll_to_me(None);
                }

                if response.clicked() {
                    app.selected = Some(i);
                    app.open_doc(alias, ctx.clone());
                }

                if response.double_clicked() {
                    app.focus = Focus::Reader;
                }
            }
        });

    // Open selected doc on Enter
    let enter_pressed = ctx.input(|i| i.key_pressed(egui::Key::Enter));
    if enter_pressed && (app.focus == Focus::Browser || app.focus == Focus::Filter) {
        if let Some(idx) = app.selected {
            if let Some(alias) = aliases.get(idx) {
                app.open_doc(alias, ctx.clone());
                app.focus = Focus::Reader;
            }
        }
    }
}
