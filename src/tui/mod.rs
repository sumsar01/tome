pub mod app;
pub mod browser;
pub mod reader;

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;

use crate::config::Config;
use app::{App, Screen};

/// Run the full TUI browser.
pub async fn run(cfg: Config) -> Result<()> {
    let mut app = App::new(cfg);
    run_terminal(&mut app).await
}

/// Run the TUI directly into the reader for a specific alias.
pub async fn run_reader(cfg: Config, alias: &str) -> Result<()> {
    let mut app = App::new(cfg);
    app.open_doc(alias).await?;
    run_terminal(&mut app).await
}

async fn run_terminal(app: &mut App) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_loop(&mut terminal, app).await;

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

async fn run_loop<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
) -> Result<()> {
    loop {
        terminal.draw(|f| match app.screen {
            Screen::Browser => browser::draw(f, app),
            Screen::Reader => reader::draw(f, app),
        })?;

        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                // Global quit
                if key.code == KeyCode::Char('q')
                    || (key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL))
                {
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
