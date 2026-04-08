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
    let app = GuiApp::new(cfg, db);
    run_eframe(app)
}

/// Entry point: open the GUI directly on a specific alias.
pub async fn run_reader(cfg: Config, db: Db, alias: &str) -> Result<()> {
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
            // Register image loaders for egui_extras (PNG, JPEG, SVG, etc.)
            egui_extras::install_image_loaders(&cc.egui_ctx);

            // Apply initial theme
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

        // Open alias on first logic tick (run_reader path)
        if let Some(alias) = app.reader_alias.clone() {
            if app.reader_content.is_none() && !app.reader_loading {
                app.open_doc(&alias, ctx.clone());
                app.focus = Focus::Reader;
            }
        }

        // Drain background fetch results
        app.poll_fetches();

        // Expire status messages
        app.tick_status();

        // Global keyboard shortcuts
        let reader_open = app.reader_content.is_some() || app.reader_loading;
        ctx.input(|i| {
            // Escape: close reader
            if i.key_pressed(egui::Key::Escape)
                && reader_open
                && app.focus == Focus::Reader
            {
                app.go_back();
            }
            // T: cycle theme (not when typing in filter)
            if i.key_pressed(egui::Key::T) && app.focus != Focus::Filter {
                app.cycle_theme(ctx);
            }
            // Tab: toggle ToC sidebar
            if i.key_pressed(egui::Key::Tab) && reader_open {
                app.toc_visible = !app.toc_visible;
            }
        });
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let app = &mut self.inner;
        let ctx = ui.ctx().clone();

        // ── Status bar (bottom) ────────────────────────────────────────────────
        if !app.status.is_empty() {
            let status_text = app.status.clone();
            let status_color = app.gui_theme.status;
            egui::Panel::bottom("status_bar")
                .show_separator_line(false)
                .show_inside(ui, |ui| {
                    ui.add_space(3.0);
                    ui.horizontal(|ui| {
                        ui.add_space(10.0);
                        ui.label(
                            egui::RichText::new(&status_text)
                                .color(status_color)
                                .size(11.0),
                        );
                    });
                    ui.add_space(3.0);
                });
        }

        // ── Browser side panel (always shown) ─────────────────────────────────
        egui::Panel::left("browser_panel")
            .default_size(260.0)
            .min_size(180.0)
            .max_size(400.0)
            .resizable(true)
            .show_inside(ui, |ui| {
                browser::show(ui, app, &ctx);
            });

        let reader_open = app.reader_content.is_some() || app.reader_loading;

        // ── ToC side panel (shown when reader is open and toc_visible) ─────────
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

        // ── Central reader panel ───────────────────────────────────────────────
        egui::CentralPanel::default().show_inside(ui, |ui| {
            reader::show(ui, app, &ctx);
        });
    }
}
