//! Terminal-Schnittstelle für die Dynpro-Schleife. Zwei Implementierungen: `term` (crossterm) und `headless`
//! (Eingabeskript, Tests).

use super::grid::Grid;

/// Taste, die die Eingabe auswertet (Abschnitt 6.2 und 9.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    /// Druckbares Zeichen, überschreibt das Feld am Cursor.
    Char(char),
    Enter,
    Tab,
    BackTab,
    /// Funktionstaste F1 bis F12.
    F(u8),
    Esc,
    Backspace,
    Delete,
    Up,
    Down,
    Left,
    Right,
    /// Strg+C im Rohmodus. Beendet das Programm mit Fehler.
    Interrupt,
}

/// Ereignis aus der Eingabe.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Key(Key),
    /// Cursor auf ein Feld setzen (nur das Eingabeskript, Abschnitt 9.3).
    Field(String),
    /// Das Eingabeskript ist zu Ende.
    Eof,
}

/// Anzeige und Eingabe eines Dynpros. Alle Fehler sind englische Texte für die Laufzeitmeldung.
pub trait Terminal: Send {
    /// Beginnt eine Sitzung mit Bildschirm (erster `CALL SCREEN` ohne aktives Dynpro).
    fn begin(&mut self) -> Result<(), String> {
        Ok(())
    }

    /// Beendet die Sitzung (letzter Dynpro-Aufruf beendet).
    fn end(&mut self) {}

    /// Zeichnet das Gitter. `cursor` ist (Zeile, Spalte), 0-basiert.
    fn draw(&mut self, grid: &Grid, cursor: (usize, usize)) -> Result<(), String>;

    /// Wartet auf das nächste Ereignis.
    fn next_event(&mut self) -> Result<Event, String>;

    /// Zeile des Eingabeskripts für Fehlermeldungen, falls das Backend eines hat.
    fn location(&self) -> Option<usize> {
        None
    }
}
