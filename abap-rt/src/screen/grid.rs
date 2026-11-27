//! Gitterpuffer 80 × 24 und Aufbau eines Dynpro-Bildes (Abschnitt 4.5, 7).
//!
//! Zeilenaufteilung: Zeile 1 Titel, Zeilen 2 bis 21 Dynpro-Bereich, Zeilen 22 und 23 Funktionstasten,
//! Zeile 24 Meldung. Ein Dynpro-Koordinatenpaar (Zeile, Spalte), 1-basiert, liegt daher auf Gitterzeile `Zeile`
//! (0-basiert) und Gitterspalte `Spalte - 1`.

use super::model::{
    Button, Dynpro, Frame, GuiStatus, Message, Text, COLOR_MASK, COLOR_SHIFT, INTENSIFIED,
};

/// Terminalbreite.
pub const COLS: usize = 80;
/// Terminalhöhe.
pub const ROWS: usize = 24;
/// Erste Zeile der Funktionstastenleiste (0-basiert, also Zeile 22).
const FKEY_FIRST_ROW: usize = 21;
/// Letzte Zeile der Funktionstastenleiste (0-basiert, also Zeile 23).
const FKEY_LAST_ROW: usize = 22;
/// Meldungszeile (0-basiert, also Zeile 24).
pub(crate) const MESSAGE_ROW: usize = 23;

/// Darstellung einer Zelle. Die Farben folgen Abschnitt 8.3, `color` 0 heißt ohne Farbe.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Style {
    pub underline: bool,
    pub bold: bool,
    pub reverse: bool,
    pub color: u8,
}

/// Eine Zelle des Gitters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    pub style: Style,
}

/// Gitter 80 × 24.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grid {
    cells: Vec<Cell>,
}

impl Default for Grid {
    fn default() -> Grid {
        Grid::new()
    }
}

impl Grid {
    /// Leeres Gitter (nur Leerzeichen).
    pub fn new() -> Grid {
        Grid {
            cells: vec![
                Cell {
                    ch: ' ',
                    style: Style::default()
                };
                ROWS * COLS
            ],
        }
    }

    pub fn cell(&self, row: usize, col: usize) -> Cell {
        self.cells[row * COLS + col]
    }

    /// Schreibt ein Zeichen. Positionen außerhalb des Gitters werden ignoriert.
    pub(crate) fn put(&mut self, row: usize, col: usize, ch: char, style: Style) {
        if row < ROWS && col < COLS {
            self.cells[row * COLS + col] = Cell { ch, style };
        }
    }

    /// Schreibt Text ab Position; was über den Rand geht, wird abgeschnitten.
    fn text(&mut self, row: usize, col: usize, text: &str, style: Style) {
        for (i, ch) in text.chars().enumerate() {
            self.put(row, col + i, ch, style);
        }
    }

    /// Zeile als Text (80 Zeichen, einschließlich Leerzeichen am Ende).
    pub fn line(&self, row: usize) -> String {
        (0..COLS).map(|col| self.cell(row, col).ch).collect()
    }
}

/// Gitterposition (Zeile, Spalte, 0-basiert) für eine Dynpro-Position (1-basiert).
pub fn dynpro_cell(row: u32, col: u32) -> (usize, usize) {
    (row as usize, (col.max(1) - 1) as usize)
}

/// Gitterposition des Cursors, begrenzt auf das Gitter.
pub fn cursor_cell(row: u32, col: u32) -> (usize, usize) {
    let (r, c) = dynpro_cell(row, col);
    (r.min(ROWS - 1), c.min(COLS - 1))
}

/// Darstellung aus den Attributbits: Hervorhebung und Farbe.
pub fn attr_style(attrs: u32) -> Style {
    Style {
        bold: attrs & INTENSIFIED != 0,
        color: ((attrs & COLOR_MASK) >> COLOR_SHIFT) as u8,
        ..Style::default()
    }
}

/// Baut das Bild eines Dynpros mit dem Titel aus dem Dynpro.
pub fn render(
    dynpro: &Dynpro,
    bufs: &[Vec<char>],
    status: Option<&GuiStatus>,
    message: Option<&Message>,
) -> Grid {
    render_titled(dynpro, &dynpro.title, bufs, None, status, message)
}

/// Baut das Bild eines Dynpros mit einem Titel. `title` ersetzt den Titel des Dynpros (`SET TITLEBAR`).
/// `bufs` enthält je Feld den Anzeigepuffer (Index wie in `dynpro.fields`).
///
/// `enabled`: Eingabebeschränkung nach E oder W (Abschnitt 12a). Felder außerhalb der Menge werden nur angezeigt,
/// ohne Unterstreichung. `None`: alle Eingabefelder sind eingabebereit.
pub fn render_titled(
    dynpro: &Dynpro,
    title: &str,
    bufs: &[Vec<char>],
    enabled: Option<&[bool]>,
    status: Option<&GuiStatus>,
    message: Option<&Message>,
) -> Grid {
    let mut grid = Grid::new();
    let title_style = Style {
        reverse: true,
        ..Style::default()
    };
    for col in 0..COLS {
        grid.put(0, col, ' ', title_style);
    }
    grid.text(0, 0, title, title_style);

    paint_content(&mut grid, dynpro, dynpro_cell, bufs, enabled, status);
    draw_fkeys(&mut grid, status);
    if let Some(message) = message {
        let style = Style {
            bold: matches!(message.kind, 'E' | 'A' | 'X'),
            ..Style::default()
        };
        grid.text(MESSAGE_ROW, 0, &message.text, style);
    }
    grid
}

/// Zeichnet die Elemente des Dynpros: Rahmen, Texte, Felder und Tasten. `place` bildet die Dynpro-Position
/// (1-basiert) auf die Gitterzelle ab. Im Vollbild ist das `dynpro_cell`, im Modalfenster die Innenfläche
/// (`window.rs`). `bufs`: Anzeigepuffer je Feld. `enabled`: Eingabebeschränkung (siehe `render_titled`).
pub(crate) fn paint_content(
    grid: &mut Grid,
    dynpro: &Dynpro,
    place: impl Fn(u32, u32) -> (usize, usize) + Copy,
    bufs: &[Vec<char>],
    enabled: Option<&[bool]>,
    status: Option<&GuiStatus>,
) {
    for frame in &dynpro.frames {
        draw_frame(grid, frame, place);
    }
    for text in &dynpro.texts {
        draw_text(grid, text, place);
    }
    for (index, field) in dynpro.fields.iter().enumerate() {
        if !field.is_visible() {
            continue;
        }
        let Some(buf) = bufs.get(index) else { continue };
        let editable =
            field.is_input() && enabled.is_none_or(|e| e.get(index).copied().unwrap_or(false));
        let style = Style {
            underline: editable,
            ..attr_style(field.attrs)
        };
        let (row, col) = place(field.row, field.col);
        for (k, &ch) in buf.iter().enumerate().take(field.width) {
            let shown = if field.is_password() && ch != ' ' {
                '*'
            } else {
                ch
            };
            grid.put(row, col + k, shown, style);
        }
    }
    for button in dynpro
        .buttons
        .iter()
        .filter(|b| !is_excluded(status, &b.fcode))
    {
        draw_button(grid, button, place);
    }
}

fn draw_text(grid: &mut Grid, text: &Text, place: impl Fn(u32, u32) -> (usize, usize)) {
    let (row, col) = place(text.row, text.col);
    grid.text(row, col, &text.text, attr_style(text.attrs));
}

fn draw_button(grid: &mut Grid, button: &Button, place: impl Fn(u32, u32) -> (usize, usize)) {
    let (row, col) = place(button.row, button.col);
    grid.text(row, col, &button.label(), Style::default());
}

/// Rahmen mit ASCII-Zeichen. Der Titel steht in der oberen Leiste, zwischen den Ecken.
fn draw_frame(grid: &mut Grid, frame: &Frame, place: impl Fn(u32, u32) -> (usize, usize)) {
    let (r1, c1) = place(frame.row1, frame.col1);
    let (r2, c2) = place(frame.row2, frame.col2);
    let style = Style::default();
    for col in c1..=c2 {
        grid.put(r1, col, '-', style);
        grid.put(r2, col, '-', style);
    }
    for row in r1..=r2 {
        grid.put(row, c1, '|', style);
        grid.put(row, c2, '|', style);
    }
    for (row, col) in [(r1, c1), (r1, c2), (r2, c1), (r2, c2)] {
        grid.put(row, col, '+', style);
    }
    if !frame.title.is_empty() && c2 > c1 + 1 {
        let label = format!(" {} ", frame.title);
        for (k, ch) in label.chars().enumerate() {
            if c1 + 1 + k < c2 {
                grid.put(r1, c1 + 1 + k, ch, style);
            }
        }
    }
}

/// Ist der Funktionscode im Status ausgeschlossen (`EXCLUDE`)? Ohne Status: nein.
fn is_excluded(status: Option<&GuiStatus>, fcode: &str) -> bool {
    status.is_some_and(|s| s.is_excluded(fcode))
}

/// Funktionstasten in den Zeilen 22 und 23: `F<n> <Text>`, aufsteigend nach Tastennummer, durch zwei Leerzeichen getrennt.
/// Eine Taste bricht nicht um. Passt sie nicht mehr in die Leiste, entfällt sie. Ausgeschlossene Codes erscheinen nicht.
fn draw_fkeys(grid: &mut Grid, status: Option<&GuiStatus>) {
    let Some(status) = status else { return };
    let mut keys: Vec<_> = status
        .fkeys
        .iter()
        .filter(|k| !k.text.is_empty() && !status.is_excluded(&k.fcode))
        .collect();
    keys.sort_by_key(|k| k.key);
    let mut row = FKEY_FIRST_ROW;
    let mut col = 0usize;
    let mut first = true;
    for key in keys {
        let item = format!("F{} {}", key.key, key.text);
        let width = item.chars().count();
        if !first {
            col += 2;
        }
        if col + width > COLS {
            row += 1;
            col = 0;
            if row > FKEY_LAST_ROW {
                break;
            }
        }
        grid.text(row, col, &item, Style::default());
        col += width;
        first = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screen::model::{AbapType, Field, INPUT, OUTPUT, PASSWORD};
    use crate::screen::model::{FKey, FKind, VarPtr};

    fn field(row: u32, col: u32, width: usize, attrs: u32) -> Field {
        Field {
            name: "F".into(),
            row,
            col,
            ty: AbapType::Char,
            length: width,
            decimals: 0,
            attrs,
            data: VarPtr(std::ptr::null_mut()),
            width,
        }
    }

    fn dynpro_with(fields: Vec<Field>) -> Dynpro {
        let mut d = Dynpro::new(100, false, 20, 80, "Kunde".into());
        d.fields = fields;
        d
    }

    fn buf(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    #[test]
    fn title_is_on_line_one_and_dynpro_starts_on_line_two() {
        let mut d = dynpro_with(vec![]);
        d.texts.push(Text {
            row: 1,
            col: 2,
            text: "Name".into(),
            attrs: 0,
        });
        let grid = render(&d, &[], None, None);
        assert!(grid.line(0).starts_with("Kunde"));
        assert_eq!(grid.line(1).find("Name"), Some(1));
    }

    #[test]
    fn input_field_is_underlined_and_output_field_is_not() {
        let d = dynpro_with(vec![field(2, 2, 4, INPUT), field(4, 2, 3, OUTPUT)]);
        let grid = render(&d, &[buf("AB  "), buf("XYZ")], None, None);
        assert_eq!(grid.line(2)[1..5].to_string(), "AB  ");
        assert!(grid.cell(2, 1).style.underline);
        assert!(!grid.cell(4, 1).style.underline);
        assert_eq!(grid.line(4)[1..4].to_string(), "XYZ");
    }

    #[test]
    fn invisible_field_is_not_drawn() {
        let d = dynpro_with(vec![field(2, 2, 4, crate::screen::model::INVISIBLE)]);
        let grid = render(&d, &[buf("SECR")], None, None);
        assert!(!grid.line(2).contains("SECR"));
    }

    #[test]
    fn password_field_shows_stars() {
        let d = dynpro_with(vec![field(2, 2, 4, INPUT | PASSWORD)]);
        let grid = render(&d, &[buf("ab  ")], None, None);
        assert_eq!(grid.line(2)[1..5].to_string(), "**  ");
    }

    #[test]
    fn function_keys_go_to_lines_22_and_23_and_message_to_line_24() {
        let mut d = dynpro_with(vec![]);
        d.buttons.push(Button {
            row: 9,
            col: 2,
            text: "Sichern".into(),
            fcode: "SAVE".into(),
        });
        let status = GuiStatus {
            name: "S".into(),
            fkeys: vec![
                FKey {
                    key: 3,
                    text: "Back".into(),
                    fcode: "BACK".into(),
                    kind: FKind::Back,
                },
                FKey {
                    key: 12,
                    text: "Cancel".into(),
                    fcode: "CANCEL".into(),
                    kind: FKind::Cancel,
                },
            ],
            excluded: vec![],
        };
        let message = Message {
            kind: 'E',
            text: "Enter a value".into(),
        };
        let grid = render(&d, &[], Some(&status), Some(&message));
        assert!(grid.line(21).starts_with("F3 Back"));
        assert!(grid.line(21).contains("F12 Cancel"));
        assert_eq!(grid.line(23).trim_end(), "Enter a value");
        // Drucktaste in Zeile 9 entspricht Gitterzeile 9
        assert!(grid.line(9).contains("[ Sichern ]"));
    }

    #[test]
    fn shift_function_key_13_is_shown_in_the_key_bar() {
        let d = dynpro_with(vec![]);
        let status = GuiStatus {
            name: "S".into(),
            fkeys: vec![FKey {
                key: 13,
                text: "Next".into(),
                fcode: "NEXT".into(),
                kind: FKind::None,
            }],
            excluded: vec![],
        };
        let grid = render(&d, &[], Some(&status), None);
        assert!(grid.line(21).starts_with("F13 Next"));
    }

    #[test]
    fn excluded_codes_are_not_drawn_as_button_or_key() {
        let mut d = dynpro_with(vec![]);
        d.buttons.push(Button {
            row: 9,
            col: 2,
            text: "Sichern".into(),
            fcode: "SAVE".into(),
        });
        d.buttons.push(Button {
            row: 10,
            col: 2,
            text: "Drucken".into(),
            fcode: "PRINT".into(),
        });
        let status = GuiStatus {
            name: "S".into(),
            fkeys: vec![
                FKey {
                    key: 3,
                    text: "Back".into(),
                    fcode: "BACK".into(),
                    kind: FKind::Back,
                },
                FKey {
                    key: 12,
                    text: "Cancel".into(),
                    fcode: "CANCEL".into(),
                    kind: FKind::Cancel,
                },
            ],
            excluded: vec!["SAVE".into(), "CANCEL".into()],
        };
        let grid = render(&d, &[], Some(&status), None);
        assert!(!grid.line(9).contains("Sichern"));
        assert!(grid.line(10).contains("[ Drucken ]"));
        assert!(grid.line(21).starts_with("F3 Back"));
        assert!(!grid.line(21).contains("Cancel"));
        assert!(!grid.line(22).contains("Cancel"));
    }

    #[test]
    fn titled_render_replaces_only_the_title_line() {
        let mut d = dynpro_with(vec![]);
        d.texts.push(Text {
            row: 1,
            col: 2,
            text: "Name".into(),
            attrs: 0,
        });
        let grid = render_titled(&d, "Neuer Titel", &[], None, None, None);
        assert!(grid.line(0).starts_with("Neuer Titel"));
        assert_eq!(grid.line(1).find("Name"), Some(1));
    }

    #[test]
    fn frame_is_drawn_with_title_in_top_border() {
        let mut d = dynpro_with(vec![]);
        d.frames.push(Frame {
            row1: 1,
            col1: 1,
            row2: 3,
            col2: 12,
            title: "Box".into(),
        });
        let grid = render(&d, &[], None, None);
        assert_eq!(grid.line(1)[0..12].to_string(), "+ Box -----+");
        assert_eq!(grid.line(2)[0..12].to_string(), "|          |");
        assert_eq!(grid.line(3)[0..12].to_string(), "+----------+");
    }

    #[test]
    fn cursor_cell_maps_dynpro_position_and_clamps() {
        assert_eq!(cursor_cell(1, 1), (1, 0));
        assert_eq!(cursor_cell(20, 80), (20, 79));
        assert_eq!(cursor_cell(20, 81), (20, 79));
    }

    #[test]
    fn grid_line_has_full_width() {
        assert_eq!(Grid::new().line(0).chars().count(), COLS);
    }
}
