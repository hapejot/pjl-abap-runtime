//! Prüfungen beim Übersetzen (Spec 4.6): `.dynp` gegen das ABAP-Programm, Typen nach 4.7.
//! Alle Verletzungen werden gesammelt; Warnungen lassen die Übersetzung nicht scheitern.

use super::*;
use crate::lower::{Ir, ScreenTarget, Stmt, Ty, MODAL_CALL};
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
        ir,
        file: fname,
        globals: global_names(ir),
        field_types: &ir.field_types,
        modules: ir
            .modules
            .iter()
            .map(|m| (m.phase, m.name.clone()))
            .collect(),
    };
    for item in &f.items {
        match item {
            DynproItem::Dynpro(d) => check_dynpro(d, &ctx, &mut out),
            DynproItem::Status(s) => check_status(s, fname, &mut out),
        }
    }

    // Alle Feldnamen aller Dynpros (kleingeschrieben) für `SET CURSOR FIELD`. Welches Dynpro zur Laufzeit
    // aktuell ist, weiß der Compiler nicht; deshalb genügt, dass der Name in der Datei vorkommt.
    let dynpro_fields: HashSet<String> = f
        .items
        .iter()
        .filter_map(|item| match item {
            DynproItem::Dynpro(d) => Some(d),
            DynproItem::Status(_) => None,
        })
        .flat_map(|d| d.elements.iter())
        .filter_map(|el| match &el.kind {
            ElementKind::Field { name, .. } => Some(name.to_lowercase()),
            _ => None,
        })
        .collect();

    // Dynpros mit `KIND MODAL` (für `CALL SCREEN ... STARTING AT`, Spec 12c)
    let modal_dynpros: HashSet<u32> = f
        .items
        .iter()
        .filter_map(|item| match item {
            DynproItem::Dynpro(d) if d.modal => Some(d.number),
            _ => None,
        })
        .collect();

    // Verweise aus dem ABAP-Programm
    let mut used = HashSet::new();
    let mut calls = false;
    for r in &ir.screen_refs {
        match &r.target {
            ScreenTarget::Dynpro(n) => {
                calls |= r.what.starts_with("CALL SCREEN");
                if dynpros.contains_key(n) {
                    used.insert(*n);
                    if r.what == MODAL_CALL && !modal_dynpros.contains(n) {
                        out.push(DynproError::error(
                            abap_file,
                            r.line,
                            1,
                            format!(
                                "{MODAL_CALL} needs a modal dynpro: DYNPRO {n:04} in {fname} is not KIND MODAL"
                            ),
                        ));
                    }
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
            ScreenTarget::Field(name) => {
                if !dynpro_fields.contains(name) {
                    out.push(DynproError::error(
                        abap_file,
                        r.line,
                        1,
                        format!(
                            "SET CURSOR FIELD '{}' is not a FIELD of any DYNPRO in {fname}",
                            name.to_uppercase()
                        ),
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
    /// Übersetzte Programm-IR: Strukturen und Typen der Komponenten
    ir: &'a Ir,
    file: &'a str,
    /// Globale Variablen (Kleinschreibung): `DATA` auf Programmebene und in MODULE-Blöcken
    globals: HashSet<String>,
    field_types: &'a HashMap<String, FieldType>,
    /// Definierte MODULEs als (Phase, Name)
    modules: HashSet<(Phase, String)>,
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
    // AT EXIT-COMMAND zählt nur bei MODULE ohne Feldbezug; Modul der Ketten zählt nicht mit (Spec 12a).
    let exit_entries: Vec<&FlowEntry> = d
        .flow
        .iter()
        .filter_map(|item| match item {
            FlowItem::Module(fe) if fe.at_exit => Some(fe),
            _ => None,
        })
        .collect();
    if exit_entries.len() > 1 {
        out.push(DynproError::error(
            c.file,
            exit_entries[1].line,
            1,
            "Only one AT EXIT-COMMAND module is allowed per DYNPRO".to_string(),
        ));
    }
    check_flow(d, c, out);

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
                let ty = if lower.contains('-') {
                    match component(c.ir, name) {
                        Ok((_, _, ty)) => ty,
                        Err(msg) => {
                            out.push(DynproError::error(c.file, el.line, el.kw_col, msg));
                            continue;
                        }
                    }
                } else {
                    if !c.globals.contains(&lower) {
                        out.push(DynproError::error(
                            c.file,
                            el.line,
                            el.kw_col,
                            format!("Field '{name}' is not a global variable of the program"),
                        ));
                        continue;
                    }
                    c.field_types
                        .get(&lower)
                        .copied()
                        .unwrap_or(FieldType::Other)
                };
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

/// Prüfung der Ablauflogik (Spec 4.6, 12a): Module, `FIELD`-Bezüge, Bedingungen und Doppelungen.
/// Die Reihenfolge der Meldungen folgt der Quelltextfolge.
fn check_flow(d: &Dynpro, c: &Ctx, out: &mut Vec<DynproError>) {
    let num = format!("DYNPRO {:04}", d.number);
    let element_fields: HashSet<String> = d
        .elements
        .iter()
        .filter_map(|el| match &el.kind {
            ElementKind::Field { name, .. } => Some(name.to_lowercase()),
            _ => None,
        })
        .collect();
    // Jedes Feld höchstens in einer FIELD-Anweisung der Ablauflogik (auch in einer CHAIN)
    let mut flow_fields: HashSet<String> = HashSet::new();
    for item in &d.flow {
        match item {
            FlowItem::Module(fe) => {
                check_flow_module(c, &num, fe.phase, &fe.module, fe.line, out);
                // ON INPUT / ON REQUEST gilt nur für FIELD, ON CHAIN-* nur für MODULE in einer CHAIN
                if fe.cond != Cond::Plain {
                    out.push(cond_error(c, fe.line, fe.cond));
                }
            }
            FlowItem::Field(ff) => {
                let lower = ff.name.to_lowercase();
                if !element_fields.contains(&lower) {
                    out.push(DynproError::error(
                        c.file,
                        ff.line,
                        1,
                        format!("FIELD '{}' in the flow is not a FIELD of {num}", ff.name),
                    ));
                }
                if !flow_fields.insert(lower) {
                    out.push(DynproError::error(
                        c.file,
                        ff.line,
                        1,
                        format!(
                            "Field '{}' is in more than one FIELD statement of {num}",
                            ff.name
                        ),
                    ));
                }
                if let Some(m) = &ff.module {
                    check_flow_module(c, &num, Phase::Pai, m, ff.line, out);
                }
                if matches!(ff.cond, Cond::ChainInput | Cond::ChainRequest) {
                    out.push(cond_error(c, ff.line, ff.cond));
                }
            }
            FlowItem::ChainModule(cm) => {
                check_flow_module(c, &num, Phase::Pai, &cm.module, cm.line, out);
            }
        }
    }
    // Wertehilfe (Spec 12b): das Feld ist ein FIELD des Dynpros, das Modul ein INPUT-Modul
    for vr in &d.value_requests {
        if !element_fields.contains(&vr.name.to_lowercase()) {
            out.push(DynproError::error(
                c.file,
                vr.line,
                1,
                format!(
                    "FIELD '{}' in PROCESS ON VALUE-REQUEST is not a FIELD of {num}",
                    vr.name
                ),
            ));
        }
        check_flow_module(c, &num, Phase::Pai, &vr.module, vr.line, out);
    }
}

/// Das MODULE muss als INPUT- bzw. OUTPUT-Modul im Programm existieren (Spec 4.6).
fn check_flow_module(
    c: &Ctx,
    num: &str,
    phase: Phase,
    module: &str,
    line: usize,
    out: &mut Vec<DynproError>,
) {
    if !c.modules.contains(&(phase, module.to_lowercase())) {
        let kind = match phase {
            Phase::Pbo => "OUTPUT",
            Phase::Pai => "INPUT",
        };
        out.push(DynproError::error(
            c.file,
            line,
            1,
            format!(
                "MODULE '{module}' is not defined in the program as MODULE ... {kind} (needed by {num})"
            ),
        ));
    }
}

/// Bedingung an der falschen Stelle: `ON INPUT`/`ON REQUEST` nur bei FIELD, `ON CHAIN-*` nur bei MODULE in einer CHAIN.
fn cond_error(c: &Ctx, line: usize, cond: Cond) -> DynproError {
    let message = match cond {
        Cond::Input | Cond::Request => format!("{} is only allowed for FIELD", cond.text()),
        _ => format!("{} is only allowed for MODULE in a CHAIN", cond.text()),
    };
    DynproError::error(c.file, line, 1, message)
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
    // EXCLUDE: Länge wie bei Funktionscodes. Ein Code ohne FKEY im selben Status ist nur ein Hinweis (Warnung).
    for e in &s.excludes {
        check_code(file, e.line, 1, &e.code, out);
        if !s.keys.iter().any(|k| k.code == e.code) {
            out.push(DynproError::warning(
                file,
                e.line,
                1,
                format!(
                    "EXCLUDE '{}' matches no FKEY code in GUISTATUS {}",
                    e.code, s.name
                ),
            ));
        }
    }
}

/// Strukturkomponente `struktur-komponente` eines Dynpro-Feldes (Spec 4.6, nur eine Ebene).
/// Ergebnis: Index der globalen Struktur, Index der Komponente und der Dynpro-Typ aus der Deklaration.
/// `name` steht so in der `.dynp`, wie der Autor es geschrieben hat; Fehler sind englische Meldungen.
/// Der Typ muss dem Compiler bekannt sein (`DATA ... BEGIN OF` oder `TYPES ... BEGIN OF` auf Programmebene).
pub(crate) fn component(ir: &Ir, name: &str) -> Result<(usize, usize, FieldType), String> {
    let Some((base, comp)) = name.split_once('-') else {
        return Err(format!("Field '{name}' is not a structure component"));
    };
    let Some(struct_idx) = global_struct(ir, base) else {
        return Err(format!(
            "Field '{name}' is not a component of a global structure of the program"
        ));
    };
    let comp = comp.to_lowercase();
    let Some(index) = ir.structs[struct_idx]
        .fields
        .iter()
        .position(|(n, _)| *n == comp)
    else {
        return Err(format!(
            "Field '{name}' is not a component of the global structure '{base}'"
        ));
    };
    let ty = ir
        .struct_field_types
        .get(&struct_idx)
        .and_then(|types| types.get(index))
        .copied()
        .unwrap_or(FieldType::Other);
    if ty == FieldType::Other {
        return Err(format!(
            "Type of dynpro field '{name}' is not supported yet: the compiler does not know the type of this component"
        ));
    }
    Ok((struct_idx, index, ty))
}

/// Index der globalen Struktur `base` (Kleinschreibung unwichtig), falls der Name eine Struktur bezeichnet.
fn global_struct(ir: &Ir, base: &str) -> Option<usize> {
    let base = base.to_lowercase();
    let top = ir.stmts.iter();
    let modules = ir.modules.iter().flat_map(|m| m.body.iter());
    top.chain(modules).find_map(|s| match s {
        Stmt::Data {
            name,
            ty: Ty::Struct(i),
            ..
        } if name.to_lowercase() == base => Some(*i),
        _ => None,
    })
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
        assert_eq!(errs[0].line, 7);
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
    fn second_exit_command_module_in_one_dynpro_is_an_error() {
        let dynp = DYNP_OK.replace(
            "      MODULE USER_COMMAND_0100\n",
            "      MODULE USER_COMMAND_0100\n      MODULE USER_COMMAND_0100 AT EXIT-COMMAND\n      MODULE USER_COMMAND_0100 AT EXIT-COMMAND\n",
        );
        let errs = run(ABAP, &dynp);
        assert!(messages(&errs)
            .iter()
            .any(|m| m.contains("Only one AT EXIT-COMMAND module is allowed")));
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

    /// Programm mit einem Modalfenster-Aufruf zusätzlich zum Hauptdynpro 0100.
    fn abap_with_modal_call() -> String {
        ABAP.replace(
            "  CALL SCREEN 0100.\n",
            "  CALL SCREEN 0100.\n  CALL SCREEN 0200 STARTING AT 10 5 ENDING AT 60 15.\n",
        )
    }

    #[test]
    fn modal_call_to_a_kind_modal_dynpro_is_accepted() {
        let dynp = format!(
            "{DYNP_OK}DYNPRO 0200 KIND MODAL\n  SIZE 9 49\n  TEXT 2 2 'Name'\n  FIELD ZNAME1 2 10 INPUT\nENDDYNPRO\n"
        );
        assert!(run(&abap_with_modal_call(), &dynp).is_empty());
    }

    #[test]
    fn modal_call_to_a_normal_dynpro_is_an_error() {
        let dynp = format!("{DYNP_OK}DYNPRO 0200\nENDDYNPRO\n");
        let errs = run(&abap_with_modal_call(), &dynp);
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(!errs[0].warning);
        assert_eq!(errs[0].line, 8);
        assert!(errs[0]
            .message
            .contains("CALL SCREEN ... STARTING AT needs a modal dynpro: DYNPRO 0200 in prog.dynp is not KIND MODAL"));
    }

    #[test]
    fn modal_call_counts_as_a_call_screen_for_the_program() {
        // Ohne normalen `CALL SCREEN` ist nur das Modalfenster da: keine Warnung „no CALL SCREEN“.
        let abap = ABAP.replace(
            "  CALL SCREEN 0100.\n",
            "  CALL SCREEN 0200 STARTING AT 10 5 ENDING AT 60 15.\n",
        );
        let dynp = format!("{DYNP_OK}DYNPRO 0200 KIND MODAL\nENDDYNPRO\n");
        let errs = run(&abap, &dynp);
        assert!(messages(&errs)
            .iter()
            .all(|m| !m.contains("no CALL SCREEN")));
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

    /// Programm mit der globalen Struktur ZKUNDE (Komponenten NAME1 c30 und NR n4) statt ZNAME1.
    fn abap_with_struct() -> String {
        ABAP.replace(
            "DATA zname1 TYPE c LENGTH 30.",
            "DATA: BEGIN OF zkunde,\n  name1 TYPE c LENGTH 30,\n  nr TYPE n LENGTH 4,\nEND OF zkunde.",
        )
    }

    #[test]
    fn struct_component_field_with_known_type_is_accepted() {
        let dynp = DYNP_OK.replace("FIELD ZNAME1 4 20 INPUT", "FIELD ZKUNDE-NAME1 4 20 INPUT");
        assert!(run(&abap_with_struct(), &dynp).is_empty());
    }

    #[test]
    fn struct_component_takes_length_and_type_of_component() {
        // NR ist n4: Breite 4 ab Spalte 20 (Spalten 20 bis 23), kein Konflikt mit anderen Elementen
        let dynp = DYNP_OK.replace("FIELD ZNAME1 4 20 INPUT", "FIELD ZKUNDE-NR 4 20 INPUT");
        assert!(run(&abap_with_struct(), &dynp).is_empty());
        let (_, _, ty) = component(&run_ir(&abap_with_struct()), "zkunde-nr").unwrap();
        assert_eq!(ty, FieldType::Numc(4));
    }

    /// Übersetzt das ABAP-Programm und gibt die IR zurück.
    fn run_ir(abap: &str) -> crate::lower::Ir {
        parse_source(abap).expect("ABAP program translates")
    }

    #[test]
    fn unknown_struct_component_is_an_error() {
        let dynp = DYNP_OK.replace("FIELD ZNAME1 4 20 INPUT", "FIELD ZKUNDE-XNAME 4 20 INPUT");
        let errs = run(&abap_with_struct(), &dynp);
        assert_eq!(
            messages(&errs),
            vec![
                "Field 'ZKUNDE-XNAME' is not a component of the global structure 'ZKUNDE'"
                    .to_string()
            ]
        );
        assert_eq!(errs[0].line, 7);
    }

    #[test]
    fn struct_component_of_plain_variable_is_an_error() {
        let dynp = DYNP_OK.replace("FIELD ZNAME1 4 20 INPUT", "FIELD ZKUNNR-NAME1 4 20 INPUT");
        let errs = run(&abap_with_struct(), &dynp);
        assert!(messages(&errs)
            .iter()
            .any(|m| m
                == "Field 'ZKUNNR-NAME1' is not a component of a global structure of the program"));
    }

    #[test]
    fn struct_component_with_unknown_type_is_not_supported_yet() {
        // LIKE liefert dem Dynpro keinen Typ: Übersetzungsfehler „not supported yet“
        let abap = abap_with_struct().replace("  nr TYPE n LENGTH 4,", "  nr LIKE zkunnr,");
        let dynp = DYNP_OK.replace("FIELD ZNAME1 4 20 INPUT", "FIELD ZKUNDE-NR 4 20 INPUT");
        let errs = run(&abap, &dynp);
        assert!(messages(&errs)
            .iter()
            .any(|m| m.contains("not supported yet") && m.contains("ZKUNDE-NR")));
    }

    #[test]
    fn struct_component_string_type_is_rejected_with_type_message() {
        let abap = abap_with_struct().replace("  nr TYPE n LENGTH 4,", "  nr TYPE string,");
        let dynp = DYNP_OK.replace("FIELD ZNAME1 4 20 INPUT", "FIELD ZKUNDE-NR 4 20 INPUT");
        let errs = run(&abap, &dynp);
        assert_eq!(
            messages(&errs),
            vec!["Type string is not allowed for dynpro field 'ZKUNDE-NR'".to_string()]
        );
    }

    #[test]
    fn set_cursor_field_must_name_a_field_of_a_dynpro() {
        let abap = ABAP.replace(
            "  LEAVE SCREEN.",
            "  SET CURSOR FIELD zfehlt.\n  LEAVE SCREEN.",
        );
        let errs = run(&abap, DYNP_OK);
        assert_eq!(
            messages(&errs),
            vec!["SET CURSOR FIELD 'ZFEHLT' is not a FIELD of any DYNPRO in prog.dynp".to_string()]
        );
        assert_eq!((errs[0].file.as_str(), errs[0].line), ("prog.abap", 12));
    }

    #[test]
    fn set_cursor_field_naming_a_dynpro_field_is_accepted() {
        let abap = ABAP.replace(
            "  LEAVE SCREEN.",
            "  SET CURSOR FIELD ZKUNNR.\n  LEAVE SCREEN.",
        );
        assert!(run(&abap, DYNP_OK).is_empty());
    }

    #[test]
    fn exclude_code_without_matching_fkey_is_a_warning() {
        let dynp = DYNP_OK.replace("ENDGUISTATUS", "  EXCLUDE PRINT\nENDGUISTATUS");
        let errs = run(ABAP, &dynp);
        assert_eq!(errs.len(), 1);
        assert!(errs[0].warning);
        assert_eq!(
            errs[0].message,
            "EXCLUDE 'PRINT' matches no FKEY code in GUISTATUS STATUS_0100"
        );
    }

    #[test]
    fn exclude_code_matching_an_fkey_gives_no_diagnostic() {
        let dynp = DYNP_OK.replace("ENDGUISTATUS", "  EXCLUDE BACK\nENDGUISTATUS");
        assert!(run(ABAP, &dynp).is_empty());
    }

    #[test]
    fn exclude_code_longer_than_70_characters_is_an_error() {
        let code = "X".repeat(71);
        let dynp = DYNP_OK.replace("ENDGUISTATUS", &format!("  EXCLUDE {code}\nENDGUISTATUS"));
        let errs = run(ABAP, &dynp);
        assert!(errs
            .iter()
            .any(|e| !e.warning && e.message.contains("longer than 70 characters")));
    }

    #[test]
    fn function_key_f24_is_accepted() {
        let dynp = DYNP_OK.replace(
            "FKEY F3 'Back' CODE BACK TYPE B",
            "FKEY F24 'Zurueck' CODE BACK",
        );
        assert!(run(ABAP, &dynp).is_empty());
    }

    /// Programm mit zusätzlichem INPUT-Modul CHECK_KUNNR für FIELD- und CHAIN-Module.
    fn abap_with_check() -> String {
        format!("{ABAP}MODULE check_kunnr INPUT.\nENDMODULE.\n")
    }

    /// `DYNP_OK` mit anderer PAI-Folge: die Zeilen unter `PROCESS AFTER INPUT` werden ersetzt.
    fn dynp_with_pai(pai: &str) -> String {
        DYNP_OK.replace(
            "    PROCESS AFTER INPUT\n      MODULE USER_COMMAND_0100\n",
            &format!("    PROCESS AFTER INPUT\n{pai}"),
        )
    }

    #[test]
    fn field_and_chain_flow_with_known_fields_and_modules_is_accepted() {
        let dynp = dynp_with_pai(
            "      FIELD ZKUNNR MODULE CHECK_KUNNR ON REQUEST\n      CHAIN\n        FIELD ZNAME1\n        MODULE CHECK_KUNNR ON CHAIN-INPUT\n      ENDCHAIN\n      MODULE USER_COMMAND_0100\n",
        );
        assert!(run(&abap_with_check(), &dynp).is_empty());
    }

    #[test]
    fn field_in_flow_that_is_not_a_field_of_the_dynpro_is_an_error() {
        let dynp = dynp_with_pai("      FIELD ZORT\n      MODULE USER_COMMAND_0100\n");
        let errs = run(&abap_with_check(), &dynp);
        assert_eq!(
            messages(&errs),
            vec!["FIELD 'ZORT' in the flow is not a FIELD of DYNPRO 0100".to_string()]
        );
        assert_eq!(errs[0].line, 13);
    }

    #[test]
    fn field_in_two_flow_statements_including_a_chain_is_an_error() {
        let dynp = dynp_with_pai(
            "      FIELD ZKUNNR\n      CHAIN\n        FIELD ZKUNNR\n      ENDCHAIN\n      MODULE USER_COMMAND_0100\n",
        );
        let errs = run(&abap_with_check(), &dynp);
        assert_eq!(
            messages(&errs),
            vec!["Field 'ZKUNNR' is in more than one FIELD statement of DYNPRO 0100".to_string()]
        );
        assert_eq!(errs[0].line, 15);
    }

    #[test]
    fn on_input_for_a_plain_module_is_an_error() {
        let dynp = dynp_with_pai("      MODULE USER_COMMAND_0100 ON INPUT\n");
        let errs = run(&abap_with_check(), &dynp);
        assert_eq!(
            messages(&errs),
            vec!["ON INPUT is only allowed for FIELD".to_string()]
        );
    }

    #[test]
    fn on_chain_input_outside_a_chain_is_an_error() {
        let dynp = dynp_with_pai(
            "      MODULE CHECK_KUNNR ON CHAIN-INPUT\n      MODULE USER_COMMAND_0100\n",
        );
        let errs = run(&abap_with_check(), &dynp);
        assert_eq!(
            messages(&errs),
            vec!["ON CHAIN-INPUT is only allowed for MODULE in a CHAIN".to_string()]
        );
        assert_eq!(errs[0].line, 13);
    }

    #[test]
    fn on_chain_request_on_a_field_is_an_error() {
        let dynp = dynp_with_pai(
            "      FIELD ZKUNNR MODULE CHECK_KUNNR ON CHAIN-REQUEST\n      MODULE USER_COMMAND_0100\n",
        );
        let errs = run(&abap_with_check(), &dynp);
        assert_eq!(
            messages(&errs),
            vec!["ON CHAIN-REQUEST is only allowed for MODULE in a CHAIN".to_string()]
        );
    }

    #[test]
    fn module_of_a_field_must_be_an_input_module() {
        let dynp = dynp_with_pai(
            "      FIELD ZKUNNR MODULE STATUS_0100 ON INPUT\n      MODULE USER_COMMAND_0100\n",
        );
        let errs = run(&abap_with_check(), &dynp);
        assert_eq!(
            messages(&errs),
            vec!["MODULE 'STATUS_0100' is not defined in the program as MODULE ... INPUT (needed by DYNPRO 0100)".to_string()]
        );
    }

    #[test]
    fn module_of_a_chain_must_be_an_input_module() {
        let dynp = dynp_with_pai(
            "      CHAIN\n        FIELD ZKUNNR\n        MODULE STATUS_0100 ON CHAIN-INPUT\n      ENDCHAIN\n      MODULE USER_COMMAND_0100\n",
        );
        let errs = run(&abap_with_check(), &dynp);
        assert!(messages(&errs).iter().any(|m| m
            == "MODULE 'STATUS_0100' is not defined in the program as MODULE ... INPUT (needed by DYNPRO 0100)"));
    }

    #[test]
    fn chain_module_does_not_count_as_a_second_exit_command_module() {
        let dynp = dynp_with_pai(
            "      CHAIN\n        FIELD ZKUNNR\n        MODULE CHECK_KUNNR ON CHAIN-INPUT\n      ENDCHAIN\n      MODULE USER_COMMAND_0100 AT EXIT-COMMAND\n",
        );
        assert!(run(&abap_with_check(), &dynp).is_empty());
    }

    #[test]
    fn chain_without_module_is_accepted() {
        let dynp = dynp_with_pai(
            "      CHAIN\n        FIELD ZKUNNR\n        FIELD ZNAME1\n      ENDCHAIN\n      MODULE USER_COMMAND_0100\n",
        );
        assert!(run(ABAP, &dynp).is_empty());
    }

    /// `DYNP_OK` mit einem Block `PROCESS ON VALUE-REQUEST` mit den Zeilen `pov` vor `ENDFLOW`.
    fn dynp_with_pov(pov: &str) -> String {
        DYNP_OK.replace(
            "  ENDFLOW\n",
            &format!("  PROCESS ON VALUE-REQUEST\n{pov}  ENDFLOW\n"),
        )
    }

    #[test]
    fn value_request_for_a_field_with_an_input_module_is_accepted() {
        let dynp = dynp_with_pov("    FIELD ZKUNNR MODULE CHECK_KUNNR\n");
        assert!(run(&abap_with_check(), &dynp).is_empty());
    }

    #[test]
    fn value_request_field_may_also_appear_in_a_pai_field_statement() {
        let dynp = dynp_with_pai("      FIELD ZKUNNR\n      MODULE USER_COMMAND_0100\n").replace(
            "  ENDFLOW\n",
            "  PROCESS ON VALUE-REQUEST\n    FIELD ZKUNNR MODULE CHECK_KUNNR\n  ENDFLOW\n",
        );
        assert!(run(&abap_with_check(), &dynp).is_empty());
    }

    #[test]
    fn value_request_field_that_is_not_a_dynpro_field_is_an_error() {
        let dynp = dynp_with_pov("    FIELD ZFEHLT MODULE CHECK_KUNNR\n");
        let errs = run(&abap_with_check(), &dynp);
        assert_eq!(
            messages(&errs),
            vec![
                "FIELD 'ZFEHLT' in PROCESS ON VALUE-REQUEST is not a FIELD of DYNPRO 0100"
                    .to_string()
            ]
        );
    }

    #[test]
    fn value_request_module_must_be_an_input_module() {
        let dynp = dynp_with_pov("    FIELD ZKUNNR MODULE STATUS_0100\n");
        let errs = run(&abap_with_check(), &dynp);
        assert_eq!(
            messages(&errs),
            vec!["MODULE 'STATUS_0100' is not defined in the program as MODULE ... INPUT (needed by DYNPRO 0100)".to_string()]
        );
    }
}
