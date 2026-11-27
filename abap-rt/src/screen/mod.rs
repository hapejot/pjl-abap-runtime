//! Dynpro-Simulation der Laufzeit (Phase 1). Der Compiler übersetzt `.dynp` in Aufrufe der C-ABI aus `abi`
//! (Prolog) und in Aufrufe für `CALL SCREEN` und Module. Die Laufzeit parst keine Layoutdatei.
//!
//! Module:
//! - `model`: Dynpro, Felder, Texte, Tasten, Rahmen, GUI-Status, Modul-Bindungen.
//! - `value`: Konvertierung ABAP-Wert <-> Anzeigetext und Prüfung der Eingabe (4.7, 6.3, 6.4).
//! - `grid`: Gitter 80 x 24 und Aufbau des Bildes (4.5, 7).
//! - `input`: Cursor, Tastenbelegung, Funktionscodes (6.2).
//! - `session`: Schleife je Dynpro (6.1), Aktionen aus Modulen (6.5).
//! - `terminal`: Trait für das Terminal. `term` (crossterm) und `headless` (Skript, Tests).
//! - `abi`: die C-ABI-Funktionen.
//!
//! Der gesamte Zustand liegt in `STATE`. Er wird nie gesperrt, während ein Modul läuft.

pub mod abi;
pub mod grid;
pub mod headless;
pub mod input;
pub mod model;
pub mod session;
pub mod term;
pub mod terminal;
pub mod value;

use std::io::{self, IsTerminal, Write};
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use crossterm::cursor::{Hide, Show};
use crossterm::execute;
use crossterm::terminal::{self as ct, EnterAlternateScreen, LeaveAlternateScreen};
use model::{Dynpro, GuiStatus, Message, VarPtr};
use terminal::Terminal;

/// Navigationsaktion aus einem Modul (`SET SCREEN`, `LEAVE SCREEN`, `LEAVE TO SCREEN`). Die letzte gilt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Nav {
    Leave,
    LeaveTo(u32),
    SetScreen(u32),
}

/// Adressen von `sy-ucomm` und `sy-dynnr` aus dem Prolog.
#[derive(Clone, Copy)]
pub(crate) struct SyVars {
    pub ucomm: VarPtr,
    pub ucomm_len: usize,
    pub dynnr: VarPtr,
    pub dynnr_len: usize,
}

/// Gesamter Zustand der Dynpro-Laufzeit.
pub(crate) struct State {
    pub dynpros: Vec<Arc<Dynpro>>,
    pub statuses: Vec<Arc<GuiStatus>>,
    pub building_dynpro: Option<Dynpro>,
    pub building_status: Option<GuiStatus>,
    pub sy: Option<SyVars>,
    /// Nummern der Dynpros, die gerade aktiv sind (äußerstes zuerst).
    pub stack: Vec<u32>,
    pub pending_nav: Option<Nav>,
    pub pending_status: Option<Arc<GuiStatus>>,
    pub message: Option<Message>,
    pub terminal: Option<Box<dyn Terminal>>,
}

impl State {
    const fn new() -> State {
        State {
            dynpros: Vec::new(),
            statuses: Vec::new(),
            building_dynpro: None,
            building_status: None,
            sy: None,
            stack: Vec::new(),
            pending_nav: None,
            pending_status: None,
            message: None,
            terminal: None,
        }
    }
}

static STATE: Mutex<State> = Mutex::new(State::new());

/// Sperrt den Zustand. Vergiftung durch eine frühere Panik wird ignoriert, der Zustand bleibt nutzbar.
pub(crate) fn state() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|poison| poison.into_inner())
}

/// Terminal für die erste Sitzung: Headless bei `ABAPRT_SCREEN_SCRIPT`, sonst crossterm bei einem Terminal.
pub(crate) fn open_terminal() -> Result<Box<dyn Terminal>, String> {
    if let Some(script) = std::env::var_os("ABAPRT_SCREEN_SCRIPT").filter(|s| !s.is_empty()) {
        let out = std::env::var_os("ABAPRT_SCREEN_OUT").filter(|s| !s.is_empty());
        let terminal = headless::Headless::open(Path::new(&script), out.as_deref().map(Path::new))?;
        return Ok(Box::new(terminal));
    }
    if io::stdin().is_terminal() && io::stdout().is_terminal() {
        return Ok(Box::new(term::Crossterm::new()));
    }
    Err(
        "dynpro screens need a terminal, or ABAPRT_SCREEN_SCRIPT for the headless backend"
            .to_string(),
    )
}

/// Laufzeitfehler: Terminal wiederherstellen, Meldung auf stderr, Status 1. Ruft nie `state()` auf.
pub(crate) fn fatal(message: &str) -> ! {
    term::restore();
    let _ = io::stdout().flush();
    eprintln!("Runtime error: {message}");
    std::process::exit(1)
}

/// Gibt das Ergebnis aus oder beendet das Programm mit Laufzeitfehler. Die Zeile des Skripts wird ergänzt.
/// Darf nicht aufgerufen werden, während der Zustand gesperrt ist.
pub(crate) fn or_fatal<T>(result: Result<T, String>) -> T {
    match result {
        Ok(value) => value,
        Err(message) => {
            let line = state().terminal.as_ref().and_then(|t| t.location());
            match line {
                Some(line) => fatal(&format!("{message} (screen script line {line})")),
                None => fatal(&message),
            }
        }
    }
}

/// Phase 0: Linktest für `crossterm` in einer kompilierten `.exe`. Öffnet den Alternativbildschirm kurz.
/// Rückgabe: 0 bei Erfolg, 1 wenn das Terminal nicht verfügbar ist oder die Ausgabe scheitert.
#[no_mangle]
pub extern "C" fn abap_rt_screen_probe() -> i32 {
    match probe() {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("Screen probe failed: {e}");
            1
        }
    }
}

fn probe() -> io::Result<()> {
    let mut out = io::stdout();
    ct::enable_raw_mode()?;
    let shown = (|| {
        execute!(out, EnterAlternateScreen, Hide)?;
        out.flush()?;
        std::thread::sleep(std::time::Duration::from_millis(300));
        execute!(out, Show, LeaveAlternateScreen)
    })();
    let restored = ct::disable_raw_mode();
    shown.and(restored)
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    /// Die Tests dieses Moduls teilen den globalen Zustand; sie laufen nacheinander.
    pub(crate) static LOCK: Mutex<()> = Mutex::new(());

    /// Setzt den Zustand auf den Anfangsstand zurück und sperrt ihn für den Test.
    pub(crate) fn exclusive() -> MutexGuard<'static, ()> {
        let guard = LOCK.lock().unwrap_or_else(|p| p.into_inner());
        *state() = State::new();
        guard
    }
}
