mod app;
mod browser;
mod reader;
mod theme;
mod toc;

use anyhow::Result;
use eframe::{egui, NativeOptions};
use egui::ViewportBuilder;

use crate::{config::Config, db::Db};
use app::{Focus, GuiApp};

/// Entry point: open the browser in the GUI.
pub async fn run(cfg: Config, db: Db) -> Result<()> {
    // Pre-warm credential caches *before* the window opens so that any macOS
    // keychain permission dialog fires here in the terminal context, not while
    // the GPU window is running (which would cause repeated prompts).
    GuiApp::prewarm_credentials();
    run_eframe(GuiApp::new(cfg, db))
}

/// Entry point: open the GUI directly on a specific alias.
pub async fn run_reader(cfg: Config, db: Db, alias: &str) -> Result<()> {
    GuiApp::prewarm_credentials();
    let mut app = GuiApp::new(cfg, db);
    app.reader_alias = Some(alias.to_string());
    app.reader_title = alias.to_string();
    run_eframe(app)
}

fn run_eframe(app: GuiApp) -> Result<()> {
    let options = NativeOptions {
        viewport: ViewportBuilder::default()
            .with_title("tome")
            .with_min_inner_size([700.0, 500.0])
            .with_inner_size([1200.0, 800.0]),
        ..Default::default()
    };

    eframe::run_native(
        "tome",
        options,
        Box::new(move |cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            cc.egui_ctx.set_global_style(theme::theme_style(&app.theme_name));
            Ok(Box::new(TomeApp { inner: app }))
        }),
    )
    .map_err(|e| anyhow::anyhow!("egui error: {e}"))?;

    Ok(())
}

/// Wrapper that implements `eframe::App`.
struct TomeApp {
    inner: GuiApp,
}

impl eframe::App for TomeApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let app = &mut self.inner;

        // ── Open alias on first tick (run_reader path) ────────────────────────
        if let Some(alias) = app.reader_alias.clone() {
            if app.reader_content.is_none() && !app.reader_loading {
                app.open_doc(&alias, ctx.clone());
                app.focus = Focus::Reader;
            }
        }

        // ── Drain background fetch results ────────────────────────────────────
        app.poll_fetches();

        // ── Expire status messages ────────────────────────────────────────────
        app.tick_status();

        // ── Global keyboard shortcuts ─────────────────────────────────────────
        let reader_open = app.reader_is_open();
        ctx.input(|i| {
            // q — quit (never when typing in filter)
            if i.key_pressed(egui::Key::Q) && app.focus != Focus::Filter {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }

            // Escape — close reader (always, regardless of focus)
            if i.key_pressed(egui::Key::Escape) {
                if app.focus == Focus::Filter {
                    if !app.filter.is_empty() {
                        app.filter.clear();
                    } else {
                        app.focus = Focus::Browser;
                    }
                } else if reader_open {
                    app.go_back();
                }
            }

            // T — cycle theme (not while typing)
            if i.key_pressed(egui::Key::T) && app.focus != Focus::Filter {
                app.cycle_theme(ctx);
            }

            // Tab — toggle ToC (only while reader is open)
            if i.key_pressed(egui::Key::Tab) && reader_open && app.focus != Focus::Filter {
                app.toc_visible = !app.toc_visible;
            }

            // Vim scroll keys — only when reader is open and not in filter
            if reader_open && app.focus != Focus::Filter {
                if i.key_pressed(egui::Key::J) || i.key_pressed(egui::Key::ArrowDown) {
                    app.reader_scroll_delta += 40.0;
                }
                if i.key_pressed(egui::Key::K) || i.key_pressed(egui::Key::ArrowUp) {
                    app.reader_scroll_delta -= 40.0;
                }
                if i.key_pressed(egui::Key::D) {
                    app.reader_scroll_delta += 300.0;
                }
                if i.key_pressed(egui::Key::U) {
                    app.reader_scroll_delta -= 300.0;
                }
                if i.key_pressed(egui::Key::G) && i.modifiers.shift {
                    app.reader_scroll_to_bottom = true;
                }
                if i.key_pressed(egui::Key::G) && !i.modifiers.shift {
                    app.reader_scroll_to_top = true;
                }
            }

            // Browser j/k navigation (only in browser, not reader)
            if !reader_open && app.focus != Focus::Filter {
                let aliases = app.filtered_aliases();
                let count = aliases.len();
                if i.key_pressed(egui::Key::J) || i.key_pressed(egui::Key::ArrowDown) {
                    app.selected = Some(match app.selected {
                        None => 0,
                        Some(n) => (n + 1).min(count.saturating_sub(1)),
                    });
                }
                if i.key_pressed(egui::Key::K) || i.key_pressed(egui::Key::ArrowUp) {
                    app.selected = Some(match app.selected {
                        None => 0,
                        Some(n) => n.saturating_sub(1),
                    });
                }
            }

            // / — focus filter from anywhere (except when already in filter)
            if i.key_pressed(egui::Key::Slash) && app.focus != Focus::Filter {
                app.focus = Focus::Filter;
            }
        });
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let app = &mut self.inner;
        let ctx = ui.ctx().clone();

        // ── Help bar (always at bottom) ───────────────────────────────────────
        let reader_open = app.reader_is_open();
        let help_text = if !app.status.is_empty() {
            app.status.clone()
        } else if reader_open {
            "Esc back  ·  j/k scroll  ·  d/u half-page  ·  g/G top/bottom  ·  Tab ToC  ·  T theme  ·  q quit".to_string()
        } else {
            "j/k move  ·  Enter open  ·  / filter  ·  T theme  ·  q quit".to_string()
        };
        let help_color = if !app.status.is_empty() {
            app.gui_theme.status
        } else {
            app.gui_theme.fg_dim
        };

        egui::Panel::bottom("help_bar")
            .exact_height(22.0)
            .show_separator_line(true)
            .show_inside(ui, |ui| {
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new(&help_text)
                            .color(help_color)
                            .size(11.0),
                    );
                });
            });

        // ── Browser side panel (always shown) ─────────────────────────────────
        egui::Panel::left("browser_panel")
            .default_size(260.0)
            .min_size(180.0)
            .max_size(400.0)
            .resizable(true)
            .show_inside(ui, |ui| {
                browser::show(ui, app, &ctx);
            });

        // ── ToC side panel ────────────────────────────────────────────────────
        if reader_open && app.toc_visible && !app.toc.is_empty() {
            egui::Panel::right("toc_panel")
                .default_size(200.0)
                .min_size(140.0)
                .max_size(300.0)
                .resizable(true)
                .show_inside(ui, |ui| {
                    toc::show(ui, app);
                });
        }

        // ── Central reader / welcome panel ────────────────────────────────────
        egui::CentralPanel::default().show_inside(ui, |ui| {
            reader::show(ui, app, &ctx);
        });
    }
}
