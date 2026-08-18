use std::{
    io::{self, stdout},
    path::PathBuf,
};

use anyhow::{Context, Result};
use crossterm::{
    event::{
        DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
        EnableFocusChange, EnableMouseCapture,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use tted::editor::Editor;

struct TerminalGuard;
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            stdout(),
            DisableMouseCapture,
            DisableBracketedPaste,
            DisableFocusChange,
            LeaveAlternateScreen,
            crossterm::cursor::Show
        );
    }
}

fn main() -> Result<()> {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    if arguments
        .iter()
        .any(|argument| argument == "--help" || argument == "-h")
    {
        println!("TTED {}", env!("CARGO_PKG_VERSION"));
        println!("Usage: tted [PATH ...]");
        println!("Open files as tabs, or open a directory as the workspace.");
        return Ok(());
    }
    if arguments
        .iter()
        .any(|argument| argument == "--version" || argument == "-V")
    {
        println!("tted {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let log_path = tted::diagnostics::init();
    if let Some(path) = &log_path {
        eprintln!("TTED diagnostics: {}", path.display());
    }
    let paths = arguments.into_iter().map(PathBuf::from).collect();
    enable_raw_mode().context("enable terminal raw mode")?;
    execute!(
        stdout(),
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste,
        EnableFocusChange,
        crossterm::cursor::Show
    )
    .context("initialize terminal")?;
    let guard = TerminalGuard;
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::CrosstermBackend::new(io::stdout()))?;
    terminal.clear()?;
    let result = Editor::new(paths).run(&mut terminal);
    tted::diagnostics::log(if result.is_ok() {
        "editor exited normally"
    } else {
        "editor exited with an error"
    });
    drop(terminal);
    drop(guard);
    result
}
