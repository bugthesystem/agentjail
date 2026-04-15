//! Terminal UI runner.

use crate::app::{App, View};
use crate::ui;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io::{self, Stdout};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

type Tui = Terminal<CrosstermBackend<Stdout>>;

/// Run TUI with owned App (simple mode).
pub async fn run(app: &mut App) -> anyhow::Result<()> {
    let mut terminal = setup_terminal()?;
    let result = event_loop_simple(&mut terminal, app).await;
    restore_terminal(&mut terminal)?;
    result
}

/// Run TUI with shared App (for demo mode).
pub async fn run_shared(app: Arc<Mutex<App>>) -> anyhow::Result<()> {
    let mut terminal = setup_terminal()?;
    let result = event_loop_shared(&mut terminal, app).await;
    restore_terminal(&mut terminal)?;
    result
}

fn setup_terminal() -> anyhow::Result<Tui> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    Ok(Terminal::new(CrosstermBackend::new(stdout))?)
}

fn restore_terminal(terminal: &mut Tui) -> anyhow::Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;
    Ok(())
}

async fn event_loop_simple(terminal: &mut Tui, app: &mut App) -> anyhow::Result<()> {
    loop {
        terminal.draw(|f| ui::render(f, app))?;
        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press { continue; }
                match app.view {
                    View::List => handle_list_key(app, key.code),
                    View::Detail => handle_detail_key(app, key.code),
                }
                if app.should_quit { break; }
            }
    }
    Ok(())
}

async fn event_loop_shared(terminal: &mut Tui, app: Arc<Mutex<App>>) -> anyhow::Result<()> {
    loop {
        {
            let app = app.lock().await;
            terminal.draw(|f| ui::render(f, &app))?;
        }
        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press { continue; }
                let mut app = app.lock().await;
                match app.view {
                    View::List => handle_list_key(&mut app, key.code),
                    View::Detail => handle_detail_key(&mut app, key.code),
                }
                if app.should_quit { break; }
            }
    }
    Ok(())
}

fn handle_list_key(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Char('q') => app.should_quit = true,
        KeyCode::Char('j') | KeyCode::Down => app.next(),
        KeyCode::Char('k') | KeyCode::Up => app.prev(),
        KeyCode::Enter => app.view = View::Detail,
        KeyCode::Char('K') => app.kill_selected(),
        KeyCode::Char('C') => app.clear_completed(),
        _ => {}
    }
}

fn handle_detail_key(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Char('q') => app.should_quit = true,
        KeyCode::Esc => app.view = View::List,
        KeyCode::Char('j') | KeyCode::Down => app.scroll_down(),
        KeyCode::Char('k') | KeyCode::Up => app.scroll_up(),
        KeyCode::Char('G') => app.scroll_end(),
        KeyCode::Char('K') => app.kill_selected(),
        _ => {}
    }
}
