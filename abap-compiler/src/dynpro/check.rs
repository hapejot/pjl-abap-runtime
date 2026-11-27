//! Prüfungen beim Übersetzen (Spec 4.6): `.dynp` gegen das ABAP-Programm, Typen nach 4.7.
//! Alle Verletzungen werden gesammelt; Warnungen lassen die Übersetzung nicht scheitern.

use super::*;
use crate::lower::{Ir, ScreenTarget, Stmt};
use std::collections::{HashMap, HashSet};

/// Prüft eine `.dynp`-Datei gegen das übersetzte Programm `ir`. `file` fehlt, wenn keine Datei gefunden wurde;
/// dann meldet jeder `CALL SCREEN`-artige Verweis einen Fehler. `abap_file` ist der Name des ABAP-Programms.
pub fn check(file: Option<&DynproFile>, ir: &Ir, abap_file: &str) -> Vec<DynproError> {
    let Some(f) = file else {
        return ir
            .screen_refs
            .iter()
            .map(|r| {
                DynproError::error(
                    abap_file,
                    r.line,
                    1,
                    format!(
                        "{} needs a dynpro definition: place '<name>.dynp' next to the program or pass --dynp",
                        r.what
                    ),
                )
            })
            .collect();
    };
    let fname = f.file.as_str();
    let mut out = Vec::new();

    // Doppelte Dynpro-Nummern und GUI-Status-Namen
    let mut dynpros: HashMap<u32, usize> = HashMap::new();
    let mut statuses: HashMap<String, usize> = HashMap::new();
    for item in &f.items {
        match item {
            DynproItem::Dynpro(d) => {
                if dynpros.insert(d.number, d.line).is_some() {
                    out.push(DynproError::error(
                        fname,
                        d.line,
                        1,
                        format!("DYNPRO {:04} is defined more than once", d.number),
                    ));
                }
            }
            DynproItem::Status(s) => {
                if statuses.insert(s.name.to_lowercase(), s.line).is_some() {
                    out.push(DynproError::error(
                        fname,
                        s.line,
                        1,
                        format!("GUISTATUS {} is defined more than once", s.name),
                    ));
                }
            }
        }
    }

    let ctx = Ctx {
        file: fname,
        globals: global_names(ir),
        field_types: &ir.field_types,
        modules: ir
            .modules
            .iter()
            .map(|m| (m.phase, m.name.clone()))
            .collect(),
        statuses: &statuses,
    };
    for item in &f.items {
        match item {
            DynproItem::Dynpro(d) => check_dynpro(d, &ctx, &mut out),
            DynproItem::Status(s) => check_status(s, fname, &mut out),
        }
    }

    // Verweise aus dem ABAP-Programm
    let mut used = HashSet::new();
    let mut calls = false;
    for r in &ir.screen_refs {
        match &r.target {
            ScreenTarget::Dynpro(n) => {
                calls |= r.what == "CALL SCREEN";
                if dynpros.contains_key(n) {
                    used.insert(*n);
                } else {
                    out.push(DynproError::error(
                        abap_file,
                        r.line,
                        1,
                        format!(
                            "{} refers to DYNPRO {n:04}, which is not defined in {fname}",
                            r.what
                        ),
                    ));
                }
            }
            ScreenTarget::Status(name) => {
                if !statuses.contains_key(name) {
                    out.push(DynproError::error(
                        abap_file,
                        r.line,
                        1,
                        format!("GUI status '{name}' is not defined in {fname}"),
                    ));
                }
            }
        }
    }
    if !dynpros.is_empty() && !calls {
        out.push(DynproError::warning(
            fname,
            1,
            1,
            "The program has no CALL SCREEN: the DYNPRO definitions are not used",
        ));
    }
    for item in &f.items {
        if let DynproItem::Dynpro(d) = item {
            if !used.contains(&d.number) {
                out.push(DynproError::warning(
                    fname,
                    d.line,
                    1,
                    format!(
                        "DYNPRO {:04} is never called (CALL SCREEN, SET SCREEN, LEAVE TO SCREEN)",
                        d.number
                    ),
                ));
            }
        }
    }
    out
}

/// Kontext einer Prüfung: Namen aus dem Programm und die Typen der Variablen.
struct Ctx<'a> {
    file: &'a str,
    /// Globale Variablen (Kleinschreibung): `DATA` auf Programmebene und in MODULE-Blöcken
    globals: HashSet<String>,
    field_types: &'a HashMap<String, FieldType>,
    /// Definierte MODULEs als (Phase, Name)
    modules: HashSet<(Phase, String)>,
    statuses: &'a HashMap<String, usize>,
}

/// Belegter Bereich einer Zeile, für die Überlappungsprüfung.
struct Span {
    row: usize,
    first: usize,
    last: usize,
    line: usize,
    kw_col: usize,
    what: String,
}

/// Namen aller globalen Variablen: einfache `DATA`-Anweisungen auf Programmebene und in MODULEs.
fn global_names(ir: &Ir) -> HashSet<String> {
    let mut names = HashSet::new();
    let top = ir.stmts.iter();
    let modules = ir.modules.iter().flat_map(|m| m.body.iter());
    for s in top.chain(modules) {
        if let Stmt::Data { name, .. } = s {
            names.insert(name.to_lowercase());
        }
    }
    names
}

fn check_dynpro(d: &Dynpro, c: &Ctx, out: &mut Vec<DynproError>) {
    let num = format!("DYNPRO {:04}", d.number);
    if d.rows == 0 || d.rows > 20 || d.cols == 0 || d.cols > 80 {
        out.push(DynproError::error(
            c.file,
            d.line,
            1,
            format!(
                "SIZE {} {} of {num} is outside 1 to 20 rows and 1 to 80 columns",
                d.rows, d.cols
            ),
        ));
        return;
    }
    if let Some(status) = &d.status {
        if !c.statuses.contains_key(&status.to_lowercase()) {
            out.push(DynproError::error(
                c.file,
                d.status_line,
                1,
                format!("GUI status '{status}' is not defined in {}", c.file),
            ));
        }
    }
    for fe in &d.flow {
        let kind = match fe.phase {
            Phase::Pbo => "OUTPUT",
            Phase::Pai => "INPUT",
        };
        if !c.modules.contains(&(fe.phase, fe.module.to_lowercase())) {
            out.push(DynproError::error(
                c.file,
                fe.line,
                1,
                format!(
                    "MODULE '{}' is not defined in the program as MODULE ... {kind} (needed by {num})",
                    fe.module
                ),
            ));
        }
    }

    let mut spans: Vec<Span> = Vec::new();
    let mut names: HashSet<String> = HashSet::new();
    for el in &d.elements {
        match &el.kind {
            ElementKind::Text { text, .. } => {
                let width = text.chars().count();
                if width == 0 {
                    out.push(DynproError::error(
                        c.file,
                        el.line,
                        el.kw_col,
                        "Text must not be empty",
                    ));
                    continue;
                }
                place(c, d, el, width, format!("text '{text}'"), &mut spans, out);
            }
            ElementKind::Field { name, .. } => {
                let lower = name.to_lowercase();
                if !names.insert(lower.clone()) {
                    out.push(DynproError::error(
                        c.file,
                        el.line,
                        el.kw_col,
                        format!("Field '{name}' is defined more than once in {num}"),
                    ));
                }
                if !c.globals.contains(&lower) {
                    out.push(DynproError::error(
                        c.file,
                        el.line,
                        el.kw_col,
                        format!("Field '{name}' is not a global variable of the program"),
                    ));
                    continue;
                }
                let ty = c
                    .field_types
                    .get(&lower)
                    .copied()
                    .unwrap_or(FieldType::Other);
                if ty == FieldType::String {
                    out.push(DynproError::error(
                        c.file,
                        el.line,
                        el.kw_col,
                        format!("Type string is not allowed for dynpro field '{name}'"),
                    ));
                    continue;
                }
                match ty.width() {
                    Some(width) => {
                        place(c, d, el, width, format!("field '{name}'"), &mut spans, out)
                    }
                    None => out.push(DynproError::error(
                        c.file,
                        el.line,
                        el.kw_col,
                        format!(
                            "Type of dynpro field '{name}' is not supported (allowed: c, n, d, t, i, p)"
                        ),
                    )),
                }
            }
            ElementKind::Button { text, code } => {
                check_code(c.file, el.line, el.kw_col, code, out);
                let width = text.chars().count() + 2;
                place(c, d, el, width, format!("button '{text}'"), &mut spans, out);
            }
            ElementKind::Frame { row2, col2, .. } => {
                if *row2 < el.row || *col2 < el.col {
                    out.push(DynproError::error(
                        c.file,
                        el.line,
                        el.kw_col,
                        "FRAME corners are reversed",
                    ));
                } else if el.row == 0 || el.col == 0 || *row2 > d.rows || *col2 > d.cols {
                    out.push(DynproError::error(
                        c.file,
                        el.line,
                        el.kw_col,
                        format!("FRAME does not fit in {num} (SIZE {} {})", d.rows, d.cols),
                    ));
                }
            }
        }
    }

    for (i, a) in spans.iter().enumerate() {
        for b in &spans[i + 1..] {
            if a.row == b.row && a.first <= b.last && b.first <= a.last {
                out.push(DynproError::error(
                    c.file,
                    b.line,
                    b.kw_col,
                    format!("{} overlaps {} in row {}", b.what, a.what, b.row),
                ));
            }
        }
    }
}

/// Prüft Lage und Breite eines Elements im Dynpro-Bereich und merkt den belegten Bereich.
fn place(
    c: &Ctx,
    d: &Dynpro,
    el: &Element,
    width: usize,
    what: String,
    spans: &mut Vec<Span>,
    out: &mut Vec<DynproError>,
) {
    if el.row == 0 || el.col == 0 || el.row > d.rows || el.col + width - 1 > d.cols {
        out.push(DynproError::error(
            c.file,
            el.line,
            el.kw_col,
            format!(
                "{what} at row {} column {} does not fit in DYNPRO {:04} (SIZE {} {})",
                el.row, el.col, d.number, d.rows, d.cols
            ),
        ));
        return;
    }
    spans.push(Span {
        row: el.row,
        first: el.col,
        last: el.col + width - 1,
        line: el.line,
        kw_col: el.kw_col,
        what,
    });
}

/// Funktionscode: nicht leer und höchstens so lang wie `sy-ucomm`.
fn check_code(file: &str, line: usize, col: usize, code: &str, out: &mut Vec<DynproError>) {
    if code.is_empty() {
        out.push(DynproError::error(
            file,
            line,
            col,
            "Function code must not be empty",
        ));
    } else if code.chars().count() > UCOMM_LENGTH {
        out.push(DynproError::error(
            file,
            line,
            col,
            format!(
                "Function code '{code}' is longer than {UCOMM_LENGTH} characters (length of sy-ucomm)"
            ),
        ));
    }
}

fn check_status(s: &GuiStatus, file: &str, out: &mut Vec<DynproError>) {
    let mut seen = HashSet::new();
    for k in &s.keys {
        if !seen.insert(k.key) {
            out.push(DynproError::error(
                file,
                k.line,
                1,
                format!(
                    "F{} is defined more than once in GUISTATUS {}",
                    k.key, s.name
                ),
            ));
        }
        check_code(file, k.line, 1, &k.code, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lower::parse_source;

    const ABAP: &str = "\
REPORT zdemo.
DATA zkunnr TYPE c LENGTH 10.
DATA zname1 TYPE c LENGTH 30.
DATA zcount TYPE i.
DATA zdate TYPE d.
START-OF-SELECTION.
  CALL SCREEN 0100.
MODULE status_0100 OUTPUT.
  SET PF-STATUS 'STATUS_0100'.
ENDMODULE.
MODULE user_command_0100 INPUT.
  LEAVE SCREEN.
ENDMODULE.
";

    const DYNP_OK: &str = "\
DYNPRO 0100
  SIZE 20 80
  TITLE 'Kunde anlegen'
  STATUS STATUS_0100
  TEXT 2 2 'Kundennummer'
  FIELD ZKUNNR 2 20 INPUT REQUIRED
  TEXT 4 2 'Name'
  FIELD ZNAME1 4 20 INPUT
  BUTTON 9 2 'Sichern' CODE SAVE
  FLOW
    PROCESS BEFORE OUTPUT
      MODULE STATUS_0100
    PROCESS AFTER INPUT
      MODULE USER_COMMAND_0100
  ENDFLOW
ENDDYNPRO
GUISTATUS STATUS_0100
  FKEY F3 'Back' CODE BACK TYPE B
ENDGUISTATUS
";

    /// Übersetzt das ABAP-Programm und prüft die Dynpro-Datei dagegen.
    fn run(abap: &str, dynp: &str) -> Vec<DynproError> {
        let ir = parse_source(abap).expect("ABAP program translates");
        let file = parse_dynp("prog.dynp", dynp).expect("dynpro file parses");
        check(Some(&file), &ir, "prog.abap")
    }

    fn messages(errs: &[DynproError]) -> Vec<String> {
        errs.iter().map(|e| e.message.clone()).collect()
    }

    #[test]
    fn valid_program_and_dynpro_produce_no_diagnostics() {
        assert!(run(ABAP, DYNP_OK).is_empty());
    }

    #[test]
    fn field_not_declared_as_global_variable_is_an_error() {
        let dynp = DYNP_OK.replace("FIELD ZNAME1", "FIELD ZFEHLT");
        let errs = run(ABAP, &dynp);
        assert_eq!(
            messages(&errs),
            vec!["Field 'ZFEHLT' is not a global variable of the program".to_string()]
        );
        assert_eq!(errs[0].line, 8);
    }

    #[test]
    fn string_field_is_rejected_with_type_message() {
        let abap = ABAP.replace("DATA zname1 TYPE c LENGTH 30.", "DATA zname1 TYPE string.");
        let errs = run(&abap, DYNP_OK);
        assert_eq!(
            messages(&errs),
            vec!["Type string is not allowed for dynpro field 'ZNAME1'".to_string()]
        );
    }

    #[test]
    fn field_defined_twice_in_one_dynpro_is_an_error() {
        let dynp = DYNP_OK.replace("TEXT 4 2 'Name'", "FIELD ZKUNNR 4 2 OUTPUT");
        let errs = run(ABAP, &dynp);
        assert!(messages(&errs)
            .iter()
            .any(|m| m == "Field 'ZKUNNR' is defined more than once in DYNPRO 0100"));
    }

    #[test]
    fn overlapping_text_and_field_are_rejected() {
        // Feld ZKUNNR (Breite 10) steht in Zeile 2 ab Spalte 5 und überlappt den Text ab Spalte 2
        let dynp = DYNP_OK.replace("FIELD ZKUNNR 2 20", "FIELD ZKUNNR 2 5");
        let errs = run(ABAP, &dynp);
        assert!(messages(&errs)
            .iter()
            .any(|m| m.contains("overlaps") && m.contains("row 2")));
    }

    #[test]
    fn element_outside_size_is_rejected() {
        let dynp = DYNP_OK.replace("BUTTON 9 2", "BUTTON 25 2");
        let errs = run(ABAP, &dynp);
        assert!(messages(&errs)
            .iter()
            .any(|m| m.contains("button 'Sichern' at row 25") && m.contains("does not fit")));
    }

    #[test]
    fn size_above_20_by_80_is_rejected() {
        let dynp = DYNP_OK.replace("SIZE 20 80", "SIZE 21 80");
        let errs = run(ABAP, &dynp);
        assert!(messages(&errs)
            .iter()
            .any(|m| m.contains("SIZE 21 80") && m.contains("outside")));
    }

    #[test]
    fn call_screen_without_dynpro_definition_is_an_error_in_the_program() {
        let abap = ABAP.replace("CALL SCREEN 0100.", "CALL SCREEN 0200.");
        let errs = run(&abap, DYNP_OK);
        let e = errs
            .iter()
            .find(|e| !e.warning)
            .expect("an error for the missing dynpro");
        assert_eq!((e.file.as_str(), e.line), ("prog.abap", 7));
        assert!(e.message.contains("refers to DYNPRO 0200"));
    }

    #[test]
    fn flow_module_missing_for_phase_is_an_error() {
        // USER_COMMAND_0100 existiert nur als INPUT-Modul; PBO verlangt OUTPUT
        let dynp = DYNP_OK.replace("MODULE STATUS_0100", "MODULE USER_COMMAND_0100");
        let errs = run(ABAP, &dynp);
        assert!(messages(&errs)
            .iter()
            .any(|m| m.contains("'USER_COMMAND_0100' is not defined") && m.contains("OUTPUT")));
    }

    #[test]
    fn unused_dynpro_is_a_warning_not_an_error() {
        let dynp = format!("{DYNP_OK}DYNPRO 0200\nENDDYNPRO\n");
        let errs = run(ABAP, &dynp);
        assert_eq!(errs.len(), 1);
        assert!(errs[0].warning);
        assert!(errs[0].message.contains("DYNPRO 0200 is never called"));
    }

    #[test]
    fn set_pf_status_without_guistatus_is_an_error() {
        let dynp = DYNP_OK.replace("GUISTATUS STATUS_0100", "GUISTATUS ANDERER");
        let errs = run(ABAP, &dynp);
        assert!(messages(&errs)
            .iter()
            .any(|m| m.contains("GUI status 'status_0100' is not defined")));
    }

    #[test]
    fn function_code_longer_than_70_characters_is_rejected() {
        let code = "X".repeat(71);
        let dynp = DYNP_OK.replace("CODE SAVE", &format!("CODE {code}"));
        let errs = run(ABAP, &dynp);
        assert!(messages(&errs)
            .iter()
            .any(|m| m.contains("longer than 70 characters")));
    }

    #[test]
    fn dynpro_file_without_call_screen_gives_warning() {
        let abap = ABAP.replace("  CALL SCREEN 0100.\n", "");
        let errs = run(&abap, DYNP_OK);
        assert!(errs.iter().all(|e| e.warning));
        assert!(messages(&errs).iter().any(|m| m.contains("no CALL SCREEN")));
    }

    #[test]
    fn missing_dynp_file_reports_each_call_screen() {
        let ir = parse_source(ABAP).expect("ABAP program translates");
        let errs = check(None, &ir, "prog.abap");
        // CALL SCREEN und SET PF-STATUS: beide brauchen die Datei
        assert_eq!(errs.len(), 2);
        assert_eq!(errs[0].line, 7);
        assert!(errs[0]
            .message
            .contains("CALL SCREEN needs a dynpro definition"));
        assert!(errs[1]
            .message
            .contains("SET PF-STATUS needs a dynpro definition"));
    }

    #[test]
    fn numeric_types_have_expected_width_and_abi_codes() {
        assert_eq!(FieldType::Numc(4).abi_code(), Some(1));
        assert_eq!(FieldType::Date.width(), Some(8));
        assert_eq!(FieldType::Time.abi_length(), Some(6));
        assert_eq!(FieldType::Int.abi_length(), Some(10));
        let packed = FieldType::Packed {
            bytes: 3,
            decimals: 2,
        };
        assert_eq!(packed.abi_length(), Some(5));
        assert_eq!(packed.width(), Some(7));
        assert_eq!(FieldType::String.abi_code(), None);
    }
}
