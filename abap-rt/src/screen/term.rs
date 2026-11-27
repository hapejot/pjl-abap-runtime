//! Terminal-Backend mit `crossterm` (Abschnitt 7). Alternativer Bildschirm und Rohmodus nur während einer
//! Dynpro-Sitzung. Wiederherstellung beim Ende der Sitzung, beim Beenden des Prozesses und bei einer Panik.

use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Once;

use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::event::{self, Event as CtEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::style::{Attribute, Color, Print, ResetColor, SetAttribute, SetForegroundColor};
use crossterm::terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{execute, queue};

use super::grid::{Cell, Grid, COLS, ROWS};
use super::terminal::{Event, Key, Terminal};

/// Ist eine Sitzung mit Alternativbildschirm und Rohmodus offen?
static ACTIVE: AtomicBool = AtomicBool::new(false);

/// Stellt Bildschirm und Eingabemodus wieder her, wenn eine Sitzung offen ist. Mehrfacher Aufruf ist harmlos.
pub fn restore() {
    if ACTIVE.swap(false, Ordering::SeqCst) {
        let mut out = io::stdout();
        let _ = execute!(
            out,
            ResetColor,
            SetAttribute(Attribute::Reset),
            Show,
            LeaveAlternateScreen
        );
        let _ = terminal::disable_raw_mode();
    }
}

extern "C" {
    fn atexit(callback: extern "C" fn()) -> i32;
}

/// Beim Beenden des Prozesses (auch `std::process::exit` bei Laufzeitfehlern) wiederherstellen.
extern "C" fn restore_at_exit() {
    restore();
}

static HOOKS: Once = Once::new();

/// Einmalig: Wiederherstellung beim Beenden des Prozesses und in der Panik-Behandlung.
fn install_hooks() {
    HOOKS.call_once(|| {
        // SAFETY: `atexit` erhält eine Funktion ohne Argumente, die von der C-Laufzeit beim Beenden aufgerufen
        // wird. `restore_at_exit` greift nur auf den atomaren Schalter `ACTIVE` zu und sperrt nichts.
        unsafe {
            atexit(restore_at_exit);
        }
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            restore();
            previous(info);
        }));
    });
}

fn io_error(err: io::Error) -> String {
    format!("terminal I/O failed: {err}")
}

/// Terminal über `crossterm`.
pub struct Crossterm {
    /// Zuletzt gezeichnetes Gitter; nur geänderte Zellen werden geschrieben.
    previous: Option<Grid>,
}

impl Default for Crossterm {
    fn default() -> Crossterm {
        Crossterm::new()
    }
}

impl Crossterm {
    pub fn new() -> Crossterm {
        Crossterm { previous: None }
    }
}

/// Farbe für die Farbnummern aus Abschnitt 8.3.
fn color_of(number: u8) -> Option<Color> {
    Some(match number {
        1 => Color::Grey,
        2 => Color::Blue,
        3 => Color::Green,
        4 => Color::Red,
        5 => Color::Yellow,
        6 => Color::Cyan,
        7 => Color::White,
        _ => return None,
    })
}

fn map_key(code: KeyCode, modifiers: KeyModifiers) -> Option<Key> {
    Some(match code {
        KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => Key::Interrupt,
        KeyCode::Char(_) if modifiers.contains(KeyModifiers::CONTROL) => return None,
        KeyCode::Char(c) => Key::Char(c),
        KeyCode::Enter => Key::Enter,
        KeyCode::Tab => Key::Tab,
        KeyCode::BackTab => Key::BackTab,
        KeyCode::Esc => Key::Esc,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Delete => Key::Delete,
        KeyCode::Up => Key::Up,
        KeyCode::Down => Key::Down,
        KeyCode::Left => Key::Left,
        KeyCode::Right => Key::Right,
        KeyCode::F(n) if (1..=12).contains(&n) => Key::F(n),
        _ => return None,
    })
}

impl Terminal for Crossterm {
    fn begin(&mut self) -> Result<(), String> {
        let (cols, rows) = terminal::size().map_err(io_error)?;
        if (cols as usize) < COLS || (rows as usize) < ROWS {
            return Err(format!(
                "terminal too small: {COLS}x{ROWS} required, found {cols}x{rows}"
            ));
        }
        install_hooks();
        self.previous = None;
        terminal::enable_raw_mode().map_err(io_error)?;
        ACTIVE.store(true, Ordering::SeqCst);
        let mut out = io::stdout();
        execute!(out, EnterAlternateScreen, Hide).map_err(io_error)?;
        Ok(())
    }

    fn end(&mut self) {
        restore();
        self.previous = None;
    }

    fn draw(&mut self, grid: &Grid, cursor: (usize, usize)) -> Result<(), String> {
        let mut out = io::stdout().lock();
        if self.previous.is_none() {
            queue!(out, Clear(ClearType::All)).map_err(io_error)?;
        }
        for row in 0..ROWS {
            for col in 0..COLS {
                let cell: Cell = grid.cell(row, col);
                let changed = match &self.previous {
                    Some(prev) => prev.cell(row, col) != cell,
                    None => true,
                };
                if changed {
                    write_cell(&mut out, row, col, cell).map_err(io_error)?;
                }
            }
        }
        queue!(
            out,
            ResetColor,
            SetAttribute(Attribute::Reset),
            MoveTo(cursor.1 as u16, cursor.0 as u16),
            Show
        )
        .map_err(io_error)?;
        out.flush().map_err(io_error)?;
        self.previous = Some(grid.clone());
        Ok(())
    }

    fn next_event(&mut self) -> Result<Event, String> {
        loop {
            if let CtEvent::Key(KeyEvent {
                code,
                modifiers,
                kind,
                ..
            }) = event::read().map_err(io_error)?
            {
                if kind == KeyEventKind::Release {
                    continue;
                }
                if let Some(key) = map_key(code, modifiers) {
                    return Ok(Event::Key(key));
                }
            }
        }
    }
}

impl Drop for Crossterm {
    fn drop(&mut self) {
        restore();
    }
}

/// Schreibt eine Zelle mit ihrer Darstellung.
fn write_cell(out: &mut impl Write, row: usize, col: usize, cell: Cell) -> io::Result<()> {
    queue!(
        out,
        MoveTo(col as u16, row as u16),
        ResetColor,
        SetAttribute(Attribute::Reset)
    )?;
    if cell.style.bold {
        queue!(out, SetAttribute(Attribute::Bold))?;
    }
    if cell.style.underline {
        queue!(out, SetAttribute(Attribute::Underlined))?;
    }
    if cell.style.reverse {
        queue!(out, SetAttribute(Attribute::Reverse))?;
    }
    if let Some(color) = color_of(cell.style.color) {
        queue!(out, SetForegroundColor(color))?;
    }
    queue!(out, Print(cell.ch))
}
