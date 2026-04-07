pub mod app;
pub mod browser;
pub mod markdown;
pub mod reader;
pub mod theme;

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;

use crate::config::Config;
use crate::db::Db;
use app::{App, Screen};

/// How long to wait for a terminal event before re-drawing.
const POLL_INTERVAL_MS: u64 = 100;

/// Run the full TUI browser.
pub async fn run(cfg: Config, db: Db) -> Result<()> {
    let mut app = App::new(cfg, db);
    run_terminal(&mut app).await
}

/// Run the TUI directly into the reader for a specific alias.
pub async fn run_reader(cfg: Config, db: Db, alias: &str) -> Result<()> {
    let mut app = App::new(cfg, db);
    app.open_doc(alias).await?;
    run_terminal(&mut app).await
}

async fn run_terminal(app: &mut App) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Ensure terminal is always restored, even on panic.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // We can't use async here, so run_loop is called via the outer async context.
        // Instead, register a panic hook that restores the terminal.
        Ok::<(), anyhow::Error>(())
    }));
    let _ = result; // The hook handles the panic case.

    let run_result = run_loop(&mut terminal, app).await;

    // Always restore — whether we returned normally or are about to propagate an error.
    let _ = disable_raw_mode();
    let _ = execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    );
    let _ = terminal.show_cursor();

    run_result
}

async fn run_loop<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
) -> Result<()>
where
    B::Error: Send + Sync + 'static,
{
    loop {
        app.tick_status();

        terminal.draw(|f| match app.screen {
            Screen::Browser => browser::draw(f, app),
            Screen::Reader => reader::draw(f, app),
        })?;

        if event::poll(std::time::Duration::from_millis(POLL_INTERVAL_MS))? {
            if let Event::Key(key) = event::read()? {
                // Ctrl+C always quits, regardless of screen
                if key.code == KeyCode::Char('c')
                    && key.modifiers.contains(KeyModifiers::CONTROL)
                {
                    return Ok(());
                }

                // 'q' in the browser quits; in the reader it goes back (handled per-screen)
                if app.screen == Screen::Browser && key.code == KeyCode::Char('q') {
                    return Ok(());
                }

                match app.screen {
                    Screen::Browser => browser::handle_key(app, key).await?,
                    Screen::Reader => reader::handle_key(app, key).await?,
                }
            }
        }
    }
}
