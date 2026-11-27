//! Dynpro-Laufzeit: Terminal-Anbindung mit `crossterm`.
//! Phase 0: Linktest. Die Funktion öffnet den Alternativbildschirm im Rohmodus, wartet kurz
//! und stellt das Terminal wieder her.

use crossterm::{cursor, execute, terminal};
use std::io::{self, Write};
use std::time::Duration;

/// Linktest für `crossterm` in einer kompilierten `.exe`.
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
    terminal::enable_raw_mode()?;
    let shown = (|| {
        execute!(out, terminal::EnterAlternateScreen, cursor::Hide)?;
        out.flush()?;
        std::thread::sleep(Duration::from_millis(300));
        execute!(out, cursor::Show, terminal::LeaveAlternateScreen)
    })();
    let restored = terminal::disable_raw_mode();
    shown.and(restored)
}
