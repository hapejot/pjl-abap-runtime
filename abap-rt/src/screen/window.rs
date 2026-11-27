//! Modalfenster (Spec 12c): Lage und Größe aus `CALL SCREEN n STARTING AT x y [ENDING AT x2 y2]`, Prüfung der
//! Elemente gegen die Innenfläche und das Bild über dem darunterliegenden Dynpro.
//!
//! Koordinaten: Spalte x und Zeile y (1-basiert) bezeichnen die linke obere Rahmenecke, `x2` und `y2` die rechte
//! untere. Im Gitter (0-basiert, siehe `grid`) liegt die obere linke Ecke bei (`top`, `left`) = (y − 1, x − 1).
//! Die Innenfläche hat `rows` × `cols` Zellen. Die Dynpro-Position (Zeile r, Spalte c, 1-basiert) liegt bei der
//! Gitterzelle (`top + r`, `left + c`).
//!
//! Annahme (Auftrag): Ohne ENDING AT ergibt sich die Innenfläche aus SIZE des modalen Dynpros (rows × cols).
//! Mit ENDING AT ist sie (y2 − y − 1) × (x2 − x − 1).

use super::grid::{paint_content, Grid, Style, COLS, MESSAGE_ROW, ROWS};
use super::model::{Dynpro, GuiStatus, Message};

/// Fenster eines modalen Dynpros im Gitter: Lage der oberen linken Rahmenecke und Größe der Innenfläche.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    pub top: usize,
    pub left: usize,
    pub rows: usize,
    pub cols: usize,
}

impl Window {
    /// Fensterlage aus den Angaben der ABI (Terminalkoordinaten, 1-basiert). `x2 = 0` und `y2 = 0` heißen: ohne
    /// ENDING AT, die Innenfläche ist `rows` × `cols` des Dynpros. Liegt das Fenster nicht im Terminal (80 × 24)
    /// oder ist die Innenfläche kleiner als 1 × 1, ist das ein Laufzeitfehler.
    pub fn resolve(
        x: u32,
        y: u32,
        x2: u32,
        y2: u32,
        rows: u32,
        cols: u32,
    ) -> Result<Window, String> {
        if !(1..=80).contains(&x) || !(1..=24).contains(&y) {
            return Err(format!(
                "CALL SCREEN STARTING AT {x} {y} is outside the terminal (columns 1 to 80, lines 1 to 24)"
            ));
        }
        let (inner_rows, inner_cols, right, bottom) = if x2 == 0 && y2 == 0 {
            let (rows, cols) = (u64::from(rows), u64::from(cols));
            (rows, cols, u64::from(x) + cols + 1, u64::from(y) + rows + 1)
        } else {
            if !(1..=80).contains(&x2) || !(1..=24).contains(&y2) {
                return Err(format!(
                    "CALL SCREEN ENDING AT {x2} {y2} is outside the terminal (columns 1 to 80, lines 1 to 24)"
                ));
            }
            let inner_rows = i64::from(y2) - i64::from(y) - 1;
            let inner_cols = i64::from(x2) - i64::from(x) - 1;
            if inner_rows < 1 || inner_cols < 1 {
                return Err(format!(
                    "window from column {x}, line {y} to column {x2}, line {y2} has an interior of {inner_rows} x {inner_cols}, at least 1 x 1 is needed"
                ));
            }
            (
                inner_rows as u64,
                inner_cols as u64,
                u64::from(x2),
                u64::from(y2),
            )
        };
        if right > 80 || bottom > 24 {
            return Err(format!(
                "window at column {x}, line {y} with interior {inner_rows} x {inner_cols} does not fit into the terminal (80 x 24)"
            ));
        }
        Ok(Window {
            top: (y - 1) as usize,
            left: (x - 1) as usize,
            rows: inner_rows as usize,
            cols: inner_cols as usize,
        })
    }

    /// Unterste Gitterzeile des Rahmens.
    pub fn bottom(&self) -> usize {
        self.top + self.rows + 1
    }

    /// Rechte Gitterspalte des Rahmens.
    pub fn right(&self) -> usize {
        self.left + self.cols + 1
    }

    /// Liegt die Gitterzelle im Fenster, Rahmen eingeschlossen?
    pub fn covers(&self, row: usize, col: usize) -> bool {
        (self.top..=self.bottom()).contains(&row) && (self.left..=self.right()).contains(&col)
    }

    /// Gitterzelle einer Dynpro-Position (1-basiert) innerhalb der Innenfläche.
    pub fn place(&self, row: u32, col: u32) -> (usize, usize) {
        (self.top + row as usize, self.left + col as usize)
    }

    /// Cursor im Gitter für eine Dynpro-Position, begrenzt auf das Gitter.
    pub fn cursor(&self, row: u32, col: u32) -> (usize, usize) {
        let (r, c) = self.place(row, col);
        (r.min(ROWS - 1), c.min(COLS - 1))
    }

    /// Passen alle sichtbaren Elemente des Dynpros in die Innenfläche? Sonst ein Laufzeitfehler.
    pub fn check_fit(&self, dynpro: &Dynpro) -> Result<(), String> {
        let inner_rows = self.rows;
        let inner_cols = self.cols;
        let fits = |what: String, row: u32, col: u32, width: usize| -> Result<(), String> {
            let inside = row >= 1
                && (row as usize) <= inner_rows
                && col >= 1
                && (col as usize) + width.max(1) - 1 <= inner_cols;
            if inside {
                Ok(())
            } else {
                Err(format!(
                    "{what} at line {row}, column {col} is outside the window interior of {inner_rows} x {inner_cols} (dynpro {})",
                    dynpro.label()
                ))
            }
        };
        for text in &dynpro.texts {
            fits(
                format!("text '{}'", text.text),
                text.row,
                text.col,
                text.text.chars().count(),
            )?;
        }
        for field in dynpro.fields.iter().filter(|f| f.is_visible()) {
            fits(
                format!("field {}", field.name),
                field.row,
                field.col,
                field.width,
            )?;
        }
        for button in &dynpro.buttons {
            fits(
                format!("button '{}'", button.text),
                button.row,
                button.col,
                button.label().chars().count(),
            )?;
        }
        for frame in &dynpro.frames {
            fits("frame corner".to_string(), frame.row1, frame.col1, 1)?;
            fits("frame corner".to_string(), frame.row2, frame.col2, 1)?;
        }
        Ok(())
    }
}

/// Inhalt, der im Fenster angezeigt wird: Titel, Anzeigepuffer, Eingabebeschränkung, GUI-Status und Meldung.
pub struct Content<'a> {
    /// Titel in der oberen Rahmenzeile (`TITLE` oder `SET TITLEBAR`)
    pub title: &'a str,
    /// Anzeigepuffer je Feld (Index wie in `dynpro.fields`)
    pub bufs: &'a [Vec<char>],
    /// Eingabebeschränkung nach E oder W (siehe `grid::render_titled`)
    pub enabled: Option<&'a [bool]>,
    /// GUI-Status des Fensters; steuert nur die ausgeschlossenen Tasten der Schaltflächen
    pub status: Option<&'a GuiStatus>,
    /// Meldung der Zeile 24 (die aktuelle des Programms)
    pub message: Option<&'a Message>,
}

/// Bild eines Dynpros im Fenster über dem darunterliegenden Bild `under` (Spec 12c).
///
/// Der Rahmen überdeckt die darunterliegenden Zellen, die Innenfläche wird geleert und mit den Elementen des
/// Dynpros gefüllt. Außerhalb des Fensters bleibt `under` sichtbar. Die Meldungszeile gehört zum Hauptbild: Ihre
/// Zellen außerhalb des Fensters zeigen die Meldung des Dynpros, innerhalb gewinnt das Fenster.
///
/// Die Funktionstastenleiste des Fensterstatus wird nicht gezeichnet (Entscheidung). Ihre Funktionscodes gelten
/// trotzdem, solange das Fenster oben liegt (`session`).
pub fn render_window(under: &Grid, window: &Window, dynpro: &Dynpro, content: &Content) -> Grid {
    let mut grid = under.clone();
    // Zeile 24 außerhalb des Fensters: nur die aktuelle Meldung, keine Reste des Hauptbildes
    for col in 0..COLS {
        if !window.covers(MESSAGE_ROW, col) {
            grid.put(MESSAGE_ROW, col, ' ', Style::default());
        }
    }
    // Fensterfläche leeren, dann Rahmen und Titel
    for row in window.top..=window.bottom() {
        for col in window.left..=window.right() {
            grid.put(row, col, ' ', Style::default());
        }
    }
    draw_border(&mut grid, window, content.title);
    paint_content(
        &mut grid,
        dynpro,
        |row, col| window.place(row, col),
        content.bufs,
        content.enabled,
        content.status,
    );
    if let Some(message) = content.message {
        let style = Style {
            bold: matches!(message.kind, 'E' | 'A' | 'X'),
            ..Style::default()
        };
        for (col, ch) in message.text.chars().enumerate().take(COLS) {
            if !window.covers(MESSAGE_ROW, col) {
                grid.put(MESSAGE_ROW, col, ch, style);
            }
        }
    }
    grid
}

/// Rahmen mit ASCII-Zeichen (wie `FRAME`, Abschnitt 8.3). Der Titel beginnt eine Zelle nach der linken oberen
/// Ecke und wird gekürzt, wenn er nicht in die obere Rahmenzeile passt.
fn draw_border(grid: &mut Grid, window: &Window, title: &str) {
    let style = Style::default();
    let (top, bottom, left, right) = (window.top, window.bottom(), window.left, window.right());
    for col in left..=right {
        grid.put(top, col, '-', style);
        grid.put(bottom, col, '-', style);
    }
    for row in top..=bottom {
        grid.put(row, left, '|', style);
        grid.put(row, right, '|', style);
    }
    for (row, col) in [(top, left), (top, right), (bottom, left), (bottom, right)] {
        grid.put(row, col, '+', style);
    }
    for (k, ch) in title.chars().take(window.cols).enumerate() {
        grid.put(top, left + 1 + k, ch, style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screen::model::{Dynpro, Field, Text, INPUT};

    /// Gitter mit einem Zeichen überall, als Hintergrund des Fensters.
    fn filled(ch: char) -> Grid {
        let mut grid = Grid::new();
        for row in 0..ROWS {
            for col in 0..COLS {
                grid.put(row, col, ch, Style::default());
            }
        }
        grid
    }

    /// Inhalt ohne Felder, Status und Meldung, nur mit Titel.
    fn plain(title: &str) -> Content<'_> {
        Content {
            title,
            bufs: &[],
            enabled: None,
            status: None,
            message: None,
        }
    }

    fn dynpro(rows: u32, cols: u32) -> Dynpro {
        Dynpro::new(200, true, rows, cols, String::new())
    }

    fn text(row: u32, col: u32, text: &str) -> Text {
        Text {
            row,
            col,
            text: text.to_string(),
            attrs: 0,
        }
    }

    fn ch(grid: &Grid, row: usize, col: usize) -> char {
        grid.cell(row, col).ch
    }

    #[test]
    fn outer_frame_corners_sit_at_the_given_terminal_position() {
        // STARTING AT 10 5 ENDING AT 60 15: Ecken in Spalte 10/60, Zeile 5/15 (1-basiert)
        let window = Window::resolve(10, 5, 60, 15, 20, 80).unwrap();
        assert_eq!(
            window,
            Window {
                top: 4,
                left: 9,
                rows: 9,
                cols: 49
            }
        );
        let grid = render_window(&filled('X'), &window, &dynpro(20, 80), &plain(""));
        assert_eq!(ch(&grid, 4, 9), '+');
        assert_eq!(ch(&grid, 4, 59), '+');
        assert_eq!(ch(&grid, 14, 9), '+');
        assert_eq!(ch(&grid, 14, 59), '+');
        assert_eq!(ch(&grid, 4, 30), '-');
        assert_eq!(ch(&grid, 14, 30), '-');
        assert_eq!(ch(&grid, 8, 9), '|');
        assert_eq!(ch(&grid, 8, 59), '|');
        // Außerhalb des Rahmens bleibt das Bild darunter
        assert_eq!(ch(&grid, 4, 8), 'X');
        assert_eq!(ch(&grid, 3, 30), 'X');
        assert_eq!(ch(&grid, 15, 30), 'X');
    }

    #[test]
    fn interior_is_blank_and_dynpro_elements_are_relative_to_it() {
        let window = Window::resolve(10, 5, 60, 15, 20, 80).unwrap();
        let mut d = dynpro(9, 49);
        d.texts.push(text(1, 1, "Hi"));
        let grid = render_window(&filled('X'), &window, &d, &plain(""));
        // Dynpro-Zeile 1, Spalte 1 liegt in der ersten Innenzeile (Zeile 6) und Innenspalte 11
        assert_eq!(ch(&grid, 5, 10), 'H');
        assert_eq!(ch(&grid, 5, 11), 'i');
        // Die übrige Innenfläche ist leer, auch dort, wo das Bild darunter Zeichen hatte
        assert_eq!(ch(&grid, 6, 20), ' ');
        assert_eq!(ch(&grid, 13, 58), ' ');
    }

    #[test]
    fn window_overlaps_function_key_and_message_lines_where_it_covers_them() {
        // Unterer Rahmen in Zeile 24, Spalten 10 bis 60: die Zeilen 22 und 23 werden in dieser Spanne überdeckt
        let window = Window::resolve(10, 15, 60, 24, 20, 80).unwrap();
        let mut under = filled('K');
        for col in 0..COLS {
            under.put(23, col, 'K', Style::default());
        }
        let message = Message {
            kind: 'S',
            text: "Hello, world".into(),
        };
        let content = Content {
            message: Some(&message),
            ..plain("")
        };
        let grid = render_window(&under, &window, &dynpro(9, 49), &content);
        // Außerhalb der Spanne bleibt die Funktionstastenleiste sichtbar (Zeile 22, 0-basiert 21)
        assert_eq!(ch(&grid, 21, 0), 'K');
        assert_eq!(ch(&grid, 21, 8), 'K');
        // Innerhalb der Spanne überdeckt der Rahmen die Zeilen 22 und 23
        assert_eq!(ch(&grid, 21, 9), '|');
        assert_eq!(ch(&grid, 21, 30), ' ');
        assert_eq!(ch(&grid, 23, 9), '+');
        assert_eq!(ch(&grid, 23, 30), '-');
        // Die Meldungszeile erscheint außerhalb des Fensters; zu dieser Zeile gehört kein Rest des Hintergrunds
        assert_eq!(ch(&grid, 23, 0), 'H');
        assert_eq!(ch(&grid, 23, 8), 'o');
        assert_eq!(ch(&grid, 23, 70), ' ');
    }

    #[test]
    fn title_starts_one_cell_after_the_corner_and_is_cut_to_the_width() {
        let window = Window::resolve(10, 5, 60, 15, 20, 80).unwrap();
        let grid = render_window(&filled('X'), &window, &dynpro(9, 49), &plain("Customer"));
        assert_eq!(ch(&grid, 4, 9), '+');
        assert_eq!(ch(&grid, 4, 10), 'C');
        assert_eq!(ch(&grid, 4, 17), 'r');
        // Kleines Fenster: der Titel passt nur in die Innenbreite (3 Zellen), die rechte Ecke bleibt
        let small = Window::resolve(10, 5, 14, 15, 9, 49).unwrap();
        assert_eq!(small.cols, 3);
        let grid = render_window(&filled('X'), &small, &dynpro(9, 3), &plain("Customer"));
        assert_eq!(ch(&grid, 4, 10), 'C');
        assert_eq!(ch(&grid, 4, 12), 's');
        assert_eq!(ch(&grid, 4, 13), '+');
    }

    #[test]
    fn positions_outside_the_terminal_are_runtime_errors() {
        assert!(Window::resolve(0, 5, 60, 15, 20, 80).is_err());
        assert!(Window::resolve(81, 5, 0, 0, 1, 1).is_err());
        assert!(Window::resolve(10, 0, 60, 15, 20, 80).is_err());
        assert!(Window::resolve(10, 25, 0, 0, 1, 1).is_err());
        let err = Window::resolve(10, 5, 81, 15, 20, 80).unwrap_err();
        assert!(
            err.contains("ENDING AT 81 15 is outside the terminal"),
            "{err}"
        );
        let err = Window::resolve(10, 5, 60, 25, 20, 80).unwrap_err();
        assert!(
            err.contains("ENDING AT 60 25 is outside the terminal"),
            "{err}"
        );
        // Ohne ENDING AT: das Fenster aus SIZE passt nicht mehr ins Terminal
        let err = Window::resolve(10, 5, 0, 0, 20, 80).unwrap_err();
        assert!(err.contains("does not fit into the terminal"), "{err}");
    }

    #[test]
    fn window_without_interior_is_a_runtime_error() {
        // Innenfläche 0 × 0
        let err = Window::resolve(10, 5, 11, 6, 20, 80).unwrap_err();
        assert!(err.contains("at least 1 x 1 is needed"), "{err}");
        // ENDING AT vor STARTING AT
        assert!(Window::resolve(10, 5, 9, 15, 20, 80).is_err());
        assert!(Window::resolve(10, 5, 60, 4, 20, 80).is_err());
    }

    #[test]
    fn without_ending_at_the_interior_is_the_dynpro_size() {
        let window = Window::resolve(1, 1, 0, 0, 5, 10).unwrap();
        assert_eq!(
            window,
            Window {
                top: 0,
                left: 0,
                rows: 5,
                cols: 10
            }
        );
        assert_eq!(window.bottom(), 6);
        assert_eq!(window.right(), 11);
    }

    #[test]
    fn element_wider_than_the_interior_is_a_runtime_error() {
        let window = Window {
            top: 4,
            left: 9,
            rows: 9,
            cols: 20,
        };
        let mut d = dynpro(9, 20);
        // Innenbreite 20: ab Spalte 18 passen drei Zeichen (Spalten 18 bis 20), vier nicht
        d.texts.push(text(1, 18, "abcd"));
        let err = window.check_fit(&d).unwrap_err();
        assert!(err.contains("text 'abcd' at line 1, column 18"), "{err}");
        let mut ok = dynpro(9, 20);
        ok.texts.push(text(1, 18, "abc"));
        assert!(window.check_fit(&ok).is_ok());
        let mut field = dynpro(9, 20);
        field.fields.push(Field {
            name: "ZNAME1".into(),
            row: 9,
            col: 1,
            ty: crate::screen::model::AbapType::Char,
            length: 3,
            decimals: 0,
            attrs: INPUT,
            data: crate::screen::model::VarPtr(std::ptr::null_mut()),
            width: 3,
        });
        assert!(window.check_fit(&field).is_ok());
        field.fields[0].row = 10;
        assert!(window.check_fit(&field).is_err());
    }
}
