//! Eingabe im Dynpro: Cursor, Tippen, Löschen, Pfeile, Tab-Folge, Funktionscodes (Abschnitte 6.2 bis 6.4).
//!
//! Der Cursor liegt in Dynpro-Koordinaten (1-basiert). Eingabefelder haben einen Anzeigepuffer, der beim
//! Laden aus den ABAP-Variablen gefüllt wird und bis zur Übertragung unverändert bleibt.

use super::model::{Button, Dynpro, FKind, Field, GuiStatus};
use super::terminal::Key;
use super::value::{self, InputError, Value};

/// Meldungstext zu einem Prüfungsfehler (Zeile 24, Abschnitt 6.4). Englisch wie alle Laufzeitmeldungen.
pub fn fault_text(field: &Field, error: InputError) -> String {
    let name = &field.name;
    match error {
        InputError::Required => format!("Field {name} is required"),
        InputError::Date => format!("Invalid date in field {name}"),
        InputError::Time => format!("Invalid time in field {name}"),
        InputError::Number => format!("Invalid number in field {name}"),
        InputError::OutOfRange => format!("Value out of range in field {name}"),
    }
}

/// Position im Dynpro-Bereich, 1-basiert.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pos {
    pub row: u32,
    pub col: u32,
}

/// Ergebnis eines Tastendrucks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Nichts zu tun (Zeichen, Cursorbewegung, abgewiesene Eingabe).
    Nothing,
    /// Übertragung zum PAI mit diesem Funktionscode und Typ aus dem GUI-Status.
    Transfer { fcode: String, kind: FKind },
    /// Strg+C.
    Interrupt,
}

/// Ein Feld, das bei der Übertragung geschrieben wird.
#[derive(Debug, PartialEq)]
pub struct Transfer {
    pub index: usize,
    pub value: Value,
}

/// Erster Fehler bei der Prüfung (Abschnitt 6.4).
#[derive(Debug, PartialEq, Eq)]
pub struct Fault {
    pub index: usize,
    pub error: InputError,
}

/// Editor für ein Dynpro. `cursor` ist `None`, bis der Cursor gesetzt oder das erste Eingabefeld gewählt wird.
pub struct Editor {
    cursor: Option<Pos>,
    bufs: Vec<Vec<char>>,
    /// Eingabemarkierung je Feld (Index wie in `dynpro.fields`). Gesetzt bei jeder Änderung durch den Benutzer,
    /// zurückgesetzt bei jedem PBO-Lauf (Abschnitt 12a).
    marks: Vec<bool>,
    /// Eingabebeschränkung nach E oder W aus einem Feld- oder Kettenmodul (Abschnitt 12a). `None`: alle Felder
    /// sind eingabebereit. Sonst gilt je Feld Index, `true` für eingabebereite Felder.
    enabled: Option<Vec<bool>>,
}

impl Default for Editor {
    fn default() -> Editor {
        Editor::new()
    }
}

impl Editor {
    /// Editor ohne Puffer. `reload` füllt die Puffer.
    pub fn new() -> Editor {
        Editor {
            cursor: None,
            bufs: Vec::new(),
            marks: Vec::new(),
            enabled: None,
        }
    }

    /// Setzt alle Eingabemarkierungen zurück. Wird vor den PBO-Modulen aufgerufen.
    pub fn clear_marks(&mut self, dynpro: &Dynpro) {
        self.marks = vec![false; dynpro.fields.len()];
    }

    /// Hat der Benutzer das Feld seit dem letzten PBO geändert?
    pub fn marked(&self, index: usize) -> bool {
        self.marks.get(index).copied().unwrap_or(false)
    }

    /// Setzt die Eingabemarkierung des Feldes.
    fn mark(&mut self, index: usize) {
        if self.marks.len() <= index {
            self.marks.resize(index + 1, false);
        }
        self.marks[index] = true;
    }

    /// Beschränkt die Eingabe auf die Felder `indices` (Abschnitt 12a, nach E oder W). Der Cursor steht auf dem
    /// ersten dieser Felder.
    pub fn restrict(&mut self, dynpro: &Dynpro, indices: &[usize]) {
        let mut enabled = vec![false; dynpro.fields.len()];
        for &index in indices {
            if let Some(slot) = enabled.get_mut(index) {
                *slot = true;
            }
        }
        if let Some(&first) = indices.first() {
            let field = &dynpro.fields[first];
            self.focus(Pos {
                row: field.row,
                col: field.col,
            });
        }
        self.enabled = Some(enabled);
    }

    /// Hebt die Eingabebeschränkung auf.
    pub fn lift_restriction(&mut self) {
        self.enabled = None;
    }

    /// Eingabebeschränkung für die Anzeige: `None` ohne Beschränkung.
    pub fn restriction(&self) -> Option<&[bool]> {
        self.enabled.as_deref()
    }

    /// Ist das Feld eingabebereit (Eingabefeld und nicht durch eine Beschränkung gesperrt)?
    pub fn editable(&self, dynpro: &Dynpro, index: usize) -> bool {
        editable_in(dynpro, self.restriction(), index)
    }

    /// Lädt die Anzeigepuffer aus den ABAP-Variablen (nach dem PBO). Der Cursor bleibt erhalten.
    pub fn reload(&mut self, dynpro: &Dynpro) -> Result<(), &'static str> {
        let mut bufs = Vec::with_capacity(dynpro.fields.len());
        for field in &dynpro.fields {
            if !field.is_visible() {
                bufs.push(vec![' '; field.width]);
                continue;
            }
            // SAFETY: Vertrag des Prologs: `field.data` zeigt auf `mem_len` Bytes der Programmvariablen.
            let value = unsafe { value::read(field)? };
            bufs.push(value::format(&value, field.width));
        }
        self.bufs = bufs;
        Ok(())
    }

    /// Anzeigepuffer je Feld (Index wie in `dynpro.fields`).
    pub fn bufs(&self) -> &[Vec<char>] {
        &self.bufs
    }

    /// Aktueller Cursor. Ohne gesetzten Cursor: erstes Eingabefeld, sonst (1, 1).
    pub fn cursor(&self, dynpro: &Dynpro) -> Pos {
        self.cursor.unwrap_or_else(|| {
            first_input(dynpro, self.restriction())
                .map(|i| Pos {
                    row: dynpro.fields[i].row,
                    col: dynpro.fields[i].col,
                })
                .unwrap_or(Pos { row: 1, col: 1 })
        })
    }

    /// Setzt den Cursor (z. B. nach einem Fehler auf das Feld).
    pub fn focus(&mut self, pos: Pos) {
        self.cursor = Some(pos);
    }

    /// Setzt den Cursor auf ein Feld (nur Eingabeskript). Unbekannte Namen sind ein Fehler.
    pub fn goto(&mut self, dynpro: &Dynpro, name: &str) -> Result<(), String> {
        let index = dynpro
            .field_index(name)
            .filter(|&i| dynpro.fields[i].is_visible())
            .ok_or_else(|| format!("Field {name} is not on dynpro {}", dynpro.label()))?;
        let field = &dynpro.fields[index];
        self.focus(Pos {
            row: field.row,
            col: field.col,
        });
        Ok(())
    }

    /// Verarbeitet einen Tastendruck. `status` liefert die Funktionstasten.
    pub fn handle(&mut self, dynpro: &Dynpro, status: Option<&GuiStatus>, key: Key) -> Outcome {
        let mut pos = self.cursor(dynpro);
        let outcome = match key {
            Key::Char(ch) => {
                self.type_char(dynpro, &mut pos, ch);
                Outcome::Nothing
            }
            Key::Backspace => {
                self.backspace(dynpro, &mut pos);
                Outcome::Nothing
            }
            Key::Delete => {
                self.delete(dynpro, pos);
                Outcome::Nothing
            }
            Key::Left => {
                pos.col = pos.col.saturating_sub(1).max(1);
                Outcome::Nothing
            }
            Key::Right => {
                if pos.col < dynpro.cols {
                    pos.col += 1;
                }
                Outcome::Nothing
            }
            Key::Up => {
                pos.row = pos.row.saturating_sub(1).max(1);
                Outcome::Nothing
            }
            Key::Down => {
                if pos.row < dynpro.rows {
                    pos.row += 1;
                }
                Outcome::Nothing
            }
            Key::Tab => {
                pos = step(dynpro, pos, true, self.restriction());
                Outcome::Nothing
            }
            Key::BackTab => {
                pos = step(dynpro, pos, false, self.restriction());
                Outcome::Nothing
            }
            Key::Enter => {
                let fcode = button_at(dynpro, pos)
                    .map(|b: &Button| b.fcode.clone())
                    .unwrap_or_default();
                transfer(status, fcode)
            }
            Key::F(n) => {
                let fcode = status
                    .and_then(|s| s.fkey(n as u32))
                    .map(|k| k.fcode.clone())
                    .unwrap_or_default();
                transfer(status, fcode)
            }
            Key::Esc => {
                let fcode = match status.and_then(|s| s.by_fcode("CANCEL")) {
                    Some(_) => "CANCEL".to_string(),
                    None => String::new(),
                };
                transfer(status, fcode)
            }
            Key::Interrupt => Outcome::Interrupt,
        };
        self.cursor = Some(pos);
        outcome
    }

    /// Überschreibt das Zeichen an der Cursorposition, wenn das Feld es zulässt.
    fn type_char(&mut self, dynpro: &Dynpro, pos: &mut Pos, ch: char) {
        let Some(index) = field_at(dynpro, *pos) else {
            return;
        };
        if !self.editable(dynpro, index) {
            return;
        }
        let field = &dynpro.fields[index];
        let offset = (pos.col - field.col) as usize;
        if value::accepts(field, &self.bufs[index], offset, ch) {
            self.bufs[index][offset] = ch;
            self.mark(index);
            pos.col += 1;
        }
    }

    /// Löscht das Zeichen links vom Cursor; der Rest rückt nach links, hinten entsteht ein Leerzeichen.
    fn backspace(&mut self, dynpro: &Dynpro, pos: &mut Pos) {
        if pos.col <= 1 {
            return;
        }
        let left = Pos {
            row: pos.row,
            col: pos.col - 1,
        };
        if let Some(index) = field_at(dynpro, left) {
            let field = &dynpro.fields[index];
            if self.editable(dynpro, index)
                && remove_at(&mut self.bufs[index], (left.col - field.col) as usize)
            {
                self.mark(index);
            }
        }
        pos.col -= 1;
    }

    /// Löscht das Zeichen unter dem Cursor; der Rest rückt nach links.
    fn delete(&mut self, dynpro: &Dynpro, pos: Pos) {
        if let Some(index) = field_at(dynpro, pos) {
            let field = &dynpro.fields[index];
            if self.editable(dynpro, index)
                && remove_at(&mut self.bufs[index], (pos.col - field.col) as usize)
            {
                self.mark(index);
            }
        }
    }

    /// Prüft alle Eingabefelder (6.4). Beim ersten Fehler wird er zurückgegeben. Bei Erfolg die Werte der Felder.
    pub fn check(&self, dynpro: &Dynpro) -> Result<Vec<Transfer>, Fault> {
        let mut out = Vec::new();
        for index in 0..dynpro.fields.len() {
            let field = &dynpro.fields[index];
            if !field.is_input() {
                continue;
            }
            let buf = &self.bufs[index];
            let value = value::parse(field, buf).map_err(|error| Fault { index, error })?;
            if field.is_required() && value::is_initial(field, &value) {
                return Err(Fault {
                    index,
                    error: InputError::Required,
                });
            }
            out.push(Transfer { index, value });
        }
        Ok(out)
    }

    /// Bei B, C, E (Abschnitt 6.4): Prüfungen entfallen. Gültige Werte werden übernommen, ungültige bleiben
    /// unverändert (Festlegung, siehe Bericht).
    pub fn check_lenient(&self, dynpro: &Dynpro) -> Vec<Transfer> {
        (0..dynpro.fields.len())
            .filter(|&index| dynpro.fields[index].is_input())
            .filter_map(|index| {
                value::parse(&dynpro.fields[index], &self.bufs[index])
                    .ok()
                    .map(|value| Transfer { index, value })
            })
            .collect()
    }
}

/// Funktionscode mit Typ aus dem GUI-Status. Ein ausgeschlossener Code (`EXCLUDE`) wird wie eine unbelegte
/// Taste behandelt: leerer Code, Typ keiner.
fn transfer(status: Option<&GuiStatus>, fcode: String) -> Outcome {
    let fcode = match status {
        Some(s) if s.is_excluded(&fcode) => String::new(),
        _ => fcode,
    };
    let kind = status
        .and_then(|s| s.by_fcode(&fcode))
        .map_or(FKind::None, |k| k.kind);
    Outcome::Transfer { fcode, kind }
}

/// Entfernt das Zeichen an `offset`, der Rest rückt nach links, hinten kommt ein Leerzeichen.
/// Liefert `false`, wenn `offset` außerhalb des Puffers liegt und nichts geändert wurde.
fn remove_at(buf: &mut Vec<char>, offset: usize) -> bool {
    if offset < buf.len() {
        buf.remove(offset);
        buf.push(' ');
        true
    } else {
        false
    }
}

/// Ist das Feld eingabebereit: Eingabefeld und, bei einer Beschränkung, in der Menge `enabled`.
fn editable_in(dynpro: &Dynpro, enabled: Option<&[bool]>, index: usize) -> bool {
    dynpro.fields[index].is_input()
        && enabled.is_none_or(|e| e.get(index).copied().unwrap_or(false))
}

/// Index des sichtbaren Feldes, das die Position enthält.
pub(crate) fn field_at(dynpro: &Dynpro, pos: Pos) -> Option<usize> {
    dynpro.fields.iter().position(|f| {
        f.is_visible()
            && f.row == pos.row
            && pos.col >= f.col
            && ((pos.col - f.col) as usize) < f.width
    })
}

/// Drucktaste unter der Cursorposition.
fn button_at(dynpro: &Dynpro, pos: Pos) -> Option<&Button> {
    dynpro.buttons.iter().find(|b| {
        let width = b.label().chars().count() as u32;
        b.row == pos.row && pos.col >= b.col && pos.col < b.col + width
    })
}

/// Erstes eingabebereites Feld in Reihenfolge (Zeile, Spalte).
fn first_input(dynpro: &Dynpro, enabled: Option<&[bool]>) -> Option<usize> {
    input_order(dynpro, enabled).first().copied()
}

/// Eingabebereite Felder, sortiert nach (Zeile, Spalte).
fn input_order(dynpro: &Dynpro, enabled: Option<&[bool]>) -> Vec<usize> {
    let mut order: Vec<usize> = (0..dynpro.fields.len())
        .filter(|&i| editable_in(dynpro, enabled, i))
        .collect();
    order.sort_by_key(|&i| (dynpro.fields[i].row, dynpro.fields[i].col));
    order
}

/// Tab (vorwärts) und BTAB (rückwärts) zum nächsten bzw. vorherigen eingabebereiten Feld.
/// Liegt der Cursor in einem Feld, zählt dessen Anfang als Bezugspunkt. Ohne Eingabefeld bleibt der Cursor.
fn step(dynpro: &Dynpro, pos: Pos, forward: bool, enabled: Option<&[bool]>) -> Pos {
    let order = input_order(dynpro, enabled);
    if order.is_empty() {
        return pos;
    }
    let anchor = match field_at(dynpro, pos) {
        Some(i) => (dynpro.fields[i].row, dynpro.fields[i].col),
        None => (pos.row, pos.col),
    };
    let start = |i: usize| (dynpro.fields[i].row, dynpro.fields[i].col);
    let target = if forward {
        order
            .iter()
            .copied()
            .find(|&i| start(i) > anchor)
            .unwrap_or(order[0])
    } else {
        order
            .iter()
            .rev()
            .copied()
            .find(|&i| start(i) < anchor)
            .unwrap_or(order[order.len() - 1])
    };
    Pos {
        row: dynpro.fields[target].row,
        col: dynpro.fields[target].col,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screen::model::{
        AbapType, FKey, Field, Module, PaiEntry, VarPtr, INPUT, INVISIBLE, OUTPUT, REQUIRED,
    };
    use crate::screen::value::display_width;

    fn field(row: u32, col: u32, ty: AbapType, length: usize, attrs: u32) -> Field {
        Field {
            name: format!("F{row}{col}"),
            row,
            col,
            ty,
            length,
            decimals: 0,
            attrs,
            data: VarPtr(std::ptr::null_mut()),
            width: display_width(ty, length, 0),
        }
    }

    fn dynpro(fields: Vec<Field>) -> Dynpro {
        let mut d = Dynpro::new(100, false, 20, 80, String::new());
        d.fields = fields;
        d
    }

    /// Editor mit leeren Puffern (Zeichen = Leerzeichen) für alle Felder.
    fn editor_for(d: &Dynpro) -> Editor {
        Editor {
            cursor: None,
            bufs: d.fields.iter().map(|f| vec![' '; f.width]).collect(),
            marks: Vec::new(),
            enabled: None,
        }
    }

    fn text(e: &Editor, index: usize) -> String {
        e.bufs()[index].iter().collect()
    }

    fn status() -> GuiStatus {
        GuiStatus {
            name: "STATUS".into(),
            fkeys: vec![
                FKey {
                    key: 3,
                    text: "Back".into(),
                    fcode: "BACK".into(),
                    kind: FKind::Back,
                },
                FKey {
                    key: 5,
                    text: "Save".into(),
                    fcode: "SAVE".into(),
                    kind: FKind::None,
                },
                FKey {
                    key: 12,
                    text: "Cancel".into(),
                    fcode: "CANCEL".into(),
                    kind: FKind::Cancel,
                },
            ],
            excluded: vec![],
        }
    }

    #[test]
    fn typing_overwrites_and_moves_cursor_inside_the_field() {
        let d = dynpro(vec![field(2, 2, AbapType::Char, 4, INPUT)]);
        let mut e = editor_for(&d);
        e.focus(Pos { row: 2, col: 2 });
        e.handle(&d, None, Key::Char('a'));
        e.handle(&d, None, Key::Char('b'));
        assert_eq!(text(&e, 0), "ab  ");
        assert_eq!(e.cursor(&d), Pos { row: 2, col: 4 });
    }

    #[test]
    fn typing_past_the_field_end_is_rejected() {
        let d = dynpro(vec![field(2, 2, AbapType::Char, 2, INPUT)]);
        let mut e = editor_for(&d);
        e.focus(Pos { row: 2, col: 2 });
        for ch in ['a', 'b', 'c'] {
            e.handle(&d, None, Key::Char(ch));
        }
        assert_eq!(text(&e, 0), "ab");
        assert_eq!(e.cursor(&d).col, 4);
    }

    #[test]
    fn typing_on_output_field_is_ignored() {
        let d = dynpro(vec![field(2, 2, AbapType::Char, 3, OUTPUT)]);
        let mut e = editor_for(&d);
        e.focus(Pos { row: 2, col: 2 });
        e.handle(&d, None, Key::Char('x'));
        assert_eq!(text(&e, 0), "   ");
    }

    #[test]
    fn typing_rejects_characters_that_do_not_fit_the_type() {
        let d = dynpro(vec![field(2, 2, AbapType::Num, 3, INPUT)]);
        let mut e = editor_for(&d);
        e.focus(Pos { row: 2, col: 2 });
        e.handle(&d, None, Key::Char('a'));
        assert_eq!(text(&e, 0), "   ");
        e.handle(&d, None, Key::Char('7'));
        assert_eq!(text(&e, 0), "7  ");
    }

    #[test]
    fn backspace_removes_left_and_shifts_rest() {
        let d = dynpro(vec![field(2, 2, AbapType::Char, 4, INPUT)]);
        let mut e = editor_for(&d);
        e.bufs[0] = "abcd".chars().collect();
        e.focus(Pos { row: 2, col: 4 });
        e.handle(&d, None, Key::Backspace);
        // Cursor auf 'c' (Spalte 4): gelöscht wird das Zeichen links davon, 'b'.
        assert_eq!(text(&e, 0), "acd ");
        assert_eq!(e.cursor(&d).col, 3);
    }

    #[test]
    fn delete_removes_under_cursor() {
        let d = dynpro(vec![field(2, 2, AbapType::Char, 4, INPUT)]);
        let mut e = editor_for(&d);
        e.bufs[0] = "abcd".chars().collect();
        e.focus(Pos { row: 2, col: 3 });
        e.handle(&d, None, Key::Delete);
        assert_eq!(text(&e, 0), "acd ");
        assert_eq!(e.cursor(&d).col, 3);
    }

    #[test]
    fn arrows_move_cursor_within_dynpro_area() {
        let d = dynpro(vec![]);
        let mut e = editor_for(&d);
        e.focus(Pos { row: 1, col: 1 });
        e.handle(&d, None, Key::Left);
        e.handle(&d, None, Key::Up);
        assert_eq!(e.cursor(&d), Pos { row: 1, col: 1 });
        for _ in 0..100 {
            e.handle(&d, None, Key::Right);
            e.handle(&d, None, Key::Down);
        }
        assert_eq!(e.cursor(&d), Pos { row: 20, col: 80 });
    }

    #[test]
    fn tab_and_backtab_visit_input_fields_in_order_and_wrap() {
        let d = dynpro(vec![
            field(4, 2, AbapType::Char, 3, INPUT),
            field(2, 30, AbapType::Char, 3, INPUT),
            field(2, 2, AbapType::Char, 3, OUTPUT),
            field(2, 10, AbapType::Char, 3, INPUT),
        ]);
        let mut e = editor_for(&d);
        // Ausgangspunkt: erstes Eingabefeld in Reihenfolge ist (2,10).
        assert_eq!(e.cursor(&d), Pos { row: 2, col: 10 });
        e.handle(&d, None, Key::Tab);
        assert_eq!(e.cursor(&d), Pos { row: 2, col: 30 });
        e.handle(&d, None, Key::Tab);
        assert_eq!(e.cursor(&d), Pos { row: 4, col: 2 });
        e.handle(&d, None, Key::Tab);
        assert_eq!(e.cursor(&d), Pos { row: 2, col: 10 });
        e.handle(&d, None, Key::BackTab);
        assert_eq!(e.cursor(&d), Pos { row: 4, col: 2 });
        e.handle(&d, None, Key::BackTab);
        assert_eq!(e.cursor(&d), Pos { row: 2, col: 30 });
    }

    #[test]
    fn tab_from_inside_a_field_goes_to_the_next_one() {
        let d = dynpro(vec![
            field(2, 2, AbapType::Char, 5, INPUT),
            field(2, 12, AbapType::Char, 5, INPUT),
        ]);
        let mut e = editor_for(&d);
        e.focus(Pos { row: 2, col: 4 });
        e.handle(&d, None, Key::Tab);
        assert_eq!(e.cursor(&d), Pos { row: 2, col: 12 });
        e.focus(Pos { row: 2, col: 4 });
        e.handle(&d, None, Key::BackTab);
        assert_eq!(e.cursor(&d), Pos { row: 2, col: 12 });
    }

    #[test]
    fn enter_on_button_sends_its_function_code() {
        let mut d = dynpro(vec![]);
        d.buttons.push(crate::screen::model::Button {
            row: 9,
            col: 2,
            text: "Save".into(),
            fcode: "SAVE".into(),
        });
        let mut e = editor_for(&d);
        e.focus(Pos { row: 9, col: 4 });
        assert_eq!(
            e.handle(&d, None, Key::Enter),
            Outcome::Transfer {
                fcode: "SAVE".into(),
                kind: FKind::None
            }
        );
        e.focus(Pos { row: 9, col: 20 });
        assert_eq!(
            e.handle(&d, None, Key::Enter),
            Outcome::Transfer {
                fcode: String::new(),
                kind: FKind::None
            }
        );
    }

    #[test]
    fn function_keys_use_status_and_empty_code_when_unassigned() {
        let d = dynpro(vec![]);
        let mut e = editor_for(&d);
        let st = status();
        assert_eq!(
            e.handle(&d, Some(&st), Key::F(3)),
            Outcome::Transfer {
                fcode: "BACK".into(),
                kind: FKind::Back
            }
        );
        assert_eq!(
            e.handle(&d, Some(&st), Key::F(7)),
            Outcome::Transfer {
                fcode: String::new(),
                kind: FKind::None
            }
        );
        assert_eq!(
            e.handle(&d, None, Key::F(3)),
            Outcome::Transfer {
                fcode: String::new(),
                kind: FKind::None
            }
        );
    }

    #[test]
    fn escape_sends_cancel_only_when_status_defines_it() {
        let d = dynpro(vec![]);
        let mut e = editor_for(&d);
        assert_eq!(
            e.handle(&d, Some(&status()), Key::Esc),
            Outcome::Transfer {
                fcode: "CANCEL".into(),
                kind: FKind::Cancel
            }
        );
        assert_eq!(
            e.handle(&d, None, Key::Esc),
            Outcome::Transfer {
                fcode: String::new(),
                kind: FKind::None
            }
        );
    }

    #[test]
    fn excluded_function_key_sends_empty_code_and_no_type() {
        let d = dynpro(vec![]);
        let mut e = editor_for(&d);
        let mut st = status();
        st.excluded.push("BACK".into());
        assert_eq!(
            e.handle(&d, Some(&st), Key::F(3)),
            Outcome::Transfer {
                fcode: String::new(),
                kind: FKind::None
            }
        );
    }

    #[test]
    fn excluded_cancel_sends_empty_code_for_escape() {
        let d = dynpro(vec![]);
        let mut e = editor_for(&d);
        let mut st = status();
        st.excluded.push("CANCEL".into());
        assert_eq!(
            e.handle(&d, Some(&st), Key::Esc),
            Outcome::Transfer {
                fcode: String::new(),
                kind: FKind::None
            }
        );
    }

    #[test]
    fn excluded_button_sends_empty_code_on_enter() {
        let mut d = dynpro(vec![]);
        d.buttons.push(crate::screen::model::Button {
            row: 9,
            col: 2,
            text: "Save".into(),
            fcode: "SAVE".into(),
        });
        let mut e = editor_for(&d);
        let mut st = status();
        st.excluded.push("SAVE".into());
        e.focus(Pos { row: 9, col: 4 });
        assert_eq!(
            e.handle(&d, Some(&st), Key::Enter),
            Outcome::Transfer {
                fcode: String::new(),
                kind: FKind::None
            }
        );
    }

    #[test]
    fn shift_function_key_f13_sends_the_code_of_its_status_entry() {
        let d = dynpro(vec![]);
        let mut e = editor_for(&d);
        let mut st = status();
        st.fkeys.push(FKey {
            key: 13,
            text: "Next".into(),
            fcode: "NEXT".into(),
            kind: FKind::None,
        });
        assert_eq!(
            e.handle(&d, Some(&st), Key::F(13)),
            Outcome::Transfer {
                fcode: "NEXT".into(),
                kind: FKind::None
            }
        );
        assert_eq!(
            e.handle(&d, Some(&st), Key::F(14)),
            Outcome::Transfer {
                fcode: String::new(),
                kind: FKind::None
            }
        );
    }

    #[test]
    fn check_reports_required_and_type_errors_with_field_index() {
        let mut req = field(2, 2, AbapType::Char, 3, INPUT | REQUIRED);
        req.name = "REQ".into();
        let date = field(4, 2, AbapType::Date, 8, INPUT);
        let d = dynpro(vec![req, date]);
        let mut e = editor_for(&d);
        assert_eq!(
            e.check(&d),
            Err(Fault {
                index: 0,
                error: InputError::Required
            })
        );
        e.bufs[0] = "ok ".chars().collect();
        e.bufs[1] = "20231301".chars().collect();
        assert_eq!(
            e.check(&d),
            Err(Fault {
                index: 1,
                error: InputError::Date
            })
        );
        e.bufs[1] = "20240229".chars().collect();
        let ok = e.check(&d).unwrap();
        assert_eq!(ok.len(), 2);
        assert_eq!(ok[0].index, 0);
    }

    #[test]
    fn check_skips_output_and_invisible_fields() {
        let d = dynpro(vec![
            field(2, 2, AbapType::Int, 4, OUTPUT | REQUIRED),
            field(3, 2, AbapType::Int, 4, INVISIBLE | REQUIRED),
        ]);
        let e = editor_for(&d);
        assert_eq!(e.check(&d).unwrap(), vec![]);
    }

    #[test]
    fn lenient_check_keeps_valid_values_and_drops_invalid_ones() {
        let d = dynpro(vec![
            field(2, 2, AbapType::Int, 4, INPUT | REQUIRED),
            field(3, 2, AbapType::Int, 4, INPUT),
        ]);
        let mut e = editor_for(&d);
        e.bufs[0] = "   x".chars().collect();
        e.bufs[1] = "42".chars().collect();
        let values = e.check_lenient(&d);
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].index, 1);
        assert_eq!(values[0].value, Value::Int(42));
    }

    #[test]
    fn goto_moves_cursor_and_rejects_unknown_fields() {
        let d = dynpro(vec![field(7, 3, AbapType::Char, 3, INPUT)]);
        let mut e = editor_for(&d);
        e.goto(&d, "f73").unwrap();
        assert_eq!(e.cursor(&d), Pos { row: 7, col: 3 });
        assert!(e.goto(&d, "NOPE").is_err());
    }

    #[test]
    fn first_input_is_default_cursor_and_modules_are_not_touched() {
        extern "C" fn noop() {}
        let mut d = dynpro(vec![
            field(5, 5, AbapType::Char, 3, OUTPUT),
            field(6, 1, AbapType::Char, 3, INPUT),
        ]);
        d.pai.push(PaiEntry::Module(Module {
            func: noop,
            at_exit: false,
        }));
        let e = editor_for(&d);
        assert_eq!(e.cursor(&d), Pos { row: 6, col: 1 });
    }

    #[test]
    fn typing_and_deleting_set_the_mark_but_moving_does_not() {
        let d = dynpro(vec![
            field(2, 2, AbapType::Char, 3, INPUT),
            field(3, 2, AbapType::Num, 3, INPUT),
        ]);
        let mut e = editor_for(&d);
        e.clear_marks(&d);
        e.focus(Pos { row: 2, col: 2 });
        e.handle(&d, None, Key::Right);
        e.handle(&d, None, Key::Down);
        assert!(!e.marked(0) && !e.marked(1));
        // Ein Zeichen, das der Typ nicht zulässt, ist keine Änderung.
        e.focus(Pos { row: 3, col: 2 });
        e.handle(&d, None, Key::Char('a'));
        assert!(!e.marked(1));
        // Überschreiben mit demselben Zeichen zählt.
        e.focus(Pos { row: 2, col: 2 });
        e.handle(&d, None, Key::Char(' '));
        assert!(e.marked(0));
        // Backspace löscht links vom Cursor: das ist das Feld mit der Markierung.
        e.focus(Pos { row: 3, col: 3 });
        e.handle(&d, None, Key::Backspace);
        assert!(e.marked(1));
        e.clear_marks(&d);
        assert!(!e.marked(0) && !e.marked(1));
    }

    #[test]
    fn restriction_blocks_typing_and_tab_outside_the_enabled_fields() {
        let d = dynpro(vec![
            field(2, 2, AbapType::Char, 3, INPUT),
            field(4, 2, AbapType::Char, 3, INPUT),
            field(6, 2, AbapType::Char, 3, INPUT),
        ]);
        let mut e = editor_for(&d);
        e.restrict(&d, &[1]);
        // Der Cursor steht auf dem eingabebereiten Feld.
        assert_eq!(e.cursor(&d), Pos { row: 4, col: 2 });
        e.handle(&d, None, Key::Char('x'));
        assert_eq!(text(&e, 1), "x  ");
        assert_eq!(text(&e, 0), "   ");
        // Tab bleibt beim einzigen eingabebereiten Feld.
        e.handle(&d, None, Key::Tab);
        assert_eq!(e.cursor(&d), Pos { row: 4, col: 2 });
        // Feld 0 ist gesperrt: Zeichen werden verworfen, keine Markierung.
        e.focus(Pos { row: 2, col: 2 });
        e.handle(&d, None, Key::Char('y'));
        assert_eq!(text(&e, 0), "   ");
        assert!(!e.marked(0));
        assert!(e.marked(1));
        e.lift_restriction();
        e.handle(&d, None, Key::Char('z'));
        assert_eq!(text(&e, 0), "z  ");
    }
}
