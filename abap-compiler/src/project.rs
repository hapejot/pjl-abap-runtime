//! Programme aus einem abapGit-ZIP laden und ihre INCLUDEs auflösen.
//!
//! Ein `INCLUDE name` wird durch den Inhalt von `name.prog.abap` aus demselben Export ersetzt.
//! Die Ersetzung passiert auf der Ebene des geparsten Programms, daher bleiben die Zeilennummern
//! der Include-Datei erhalten.
//!
//! Routinen (`FORM`, `MODULE`, Ereignisse) und Klassen sind deklarativ. Steht ein Include innerhalb
//! eines Blocks, werden sie auf die oberste Ebene gehoben, die ausführbaren Anweisungen bleiben an
//! ihrer Stelle. Nicht auflösbar bleiben `INCLUDE TYPE`/`STRUCTURE`, Feldsymbole und Includes ohne
//! Datei im Export.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use zip::ZipArchive;

use crate::ddic::{self, Ddic};
use crate::inspect::abap::{parse_l1, Item, Program, Statement, TopItem};
use crate::inspect::cli::read_abap_files;
use crate::inspect::objects::object_key;

/// Alle Programme (PROG-Objekte) eines ZIP-Exports: Name (klein) -> geparstes Programm.
pub fn load_programs(zip_path: &Path) -> Result<BTreeMap<String, Program>, String> {
    let file = File::open(zip_path).map_err(|e| format!("{}: {e}", zip_path.display()))?;
    let mut archive = ZipArchive::new(file).map_err(|e| format!("Invalid ZIP: {e}"))?;
    let mut programs = BTreeMap::new();
    for (path, src) in read_abap_files(&mut archive) {
        if let Some((obj_type, name)) = object_key(&path) {
            if obj_type == "PROG" {
                programs.insert(name, parse_l1(&src));
            }
        }
    }
    Ok(programs)
}

fn has_report(prog: &Program) -> bool {
    prog.items
        .iter()
        .any(|i| matches!(i, TopItem::Statement(s) if s.keyword() == "REPORT"))
}

/// Name des Hauptprogramms: der angegebene, sonst das einzige Programm mit REPORT.
pub fn main_program(
    programs: &BTreeMap<String, Program>,
    requested: Option<&str>,
) -> Result<String, String> {
    if let Some(name) = requested {
        let key = name.to_lowercase();
        return if programs.contains_key(&key) {
            Ok(key)
        } else {
            Err(format!("Program '{name}' not found in the export"))
        };
    }
    let reports: Vec<&String> = programs
        .iter()
        .filter(|(_, p)| has_report(p))
        .map(|(n, _)| n)
        .collect();
    match reports.as_slice() {
        [one] => Ok((*one).clone()),
        [] => Err("no program with REPORT in the export; specify --program".into()),
        many => Err(format!(
            "several programs with REPORT ({}); specify --program",
            many.iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// Name des Includes, falls die Anweisung ein auflösbarer `INCLUDE name` ist.
fn include_target(s: &Statement) -> Option<String> {
    if s.keyword() != "INCLUDE" {
        return None;
    }
    let t = s.tokens.get(1)?;
    if t.is_word("TYPE") || t.is_word("STRUCTURE") || t.text.starts_with('<') {
        return None;
    }
    Some(t.text.to_lowercase())
}

/// Löst alle INCLUDEs des Programms rekursiv auf. Zyklen werden nicht erneut eingesetzt.
pub fn expand_includes(main: &mut Program, programs: &BTreeMap<String, Program>) {
    let mut stack = Vec::new();
    expand_top(&mut main.items, programs, &mut stack);
}

/// Oberste Ebene: ein Include wird an seiner Stelle durch seine Elemente ersetzt.
fn expand_top(
    items: &mut Vec<TopItem>,
    programs: &BTreeMap<String, Program>,
    stack: &mut Vec<String>,
) {
    let mut out = Vec::with_capacity(items.len());
    for item in std::mem::take(items) {
        let target = match &item {
            TopItem::Statement(s) => include_target(s),
            _ => None,
        };
        if let Some(name) = target {
            if let Some(inc) = programs.get(&name).filter(|_| !stack.contains(&name)) {
                stack.push(name);
                let mut inner = inc.items.clone();
                expand_top(&mut inner, programs, stack);
                stack.pop();
                out.extend(inner);
                continue;
            }
        }
        let mut hoisted = Vec::new();
        out.push(expand_in_body(item, programs, stack, &mut hoisted));
        out.extend(hoisted);
    }
    *items = out;
}

/// Includes innerhalb von Routinen, Blöcken und Klassen-Implementierungen.
/// Herausgehobene Routinen und Klassen landen in `hoisted` und werden auf oberster Ebene eingefügt.
fn expand_in_body(
    item: TopItem,
    programs: &BTreeMap<String, Program>,
    stack: &mut Vec<String>,
    hoisted: &mut Vec<TopItem>,
) -> TopItem {
    match item {
        TopItem::Block { head, mut body } => {
            expand_items(&mut body, programs, stack, hoisted);
            TopItem::Block { head, body }
        }
        TopItem::Routine {
            kind,
            name,
            head,
            mut body,
        } => {
            expand_items(&mut body, programs, stack, hoisted);
            TopItem::Routine {
                kind,
                name,
                head,
                body,
            }
        }
        TopItem::Class(mut class) => {
            for method in class.methods.iter_mut() {
                expand_items(&mut method.body, programs, stack, hoisted);
            }
            expand_items(&mut class.other, programs, stack, hoisted);
            TopItem::Class(class)
        }
        other => other,
    }
}

/// Ersetzt INCLUDEs in einer Anweisungsfolge. Routinen und Klassen aus dem Include werden
/// nach `hoisted` verschoben, Anweisungen und Blöcke bleiben an der Stelle.
fn expand_items(
    items: &mut Vec<Item>,
    programs: &BTreeMap<String, Program>,
    stack: &mut Vec<String>,
    hoisted: &mut Vec<TopItem>,
) {
    let mut out = Vec::with_capacity(items.len());
    for item in std::mem::take(items) {
        let target = match &item {
            Item::Statement(s) => include_target(s),
            Item::Block { .. } => None,
        };
        if let Some(name) = target {
            if let Some(inc) = programs.get(&name).filter(|_| !stack.contains(&name)) {
                stack.push(name);
                let mut inner = inc.items.clone();
                expand_top(&mut inner, programs, stack);
                stack.pop();
                for top in inner {
                    match top {
                        TopItem::Statement(s) => out.push(Item::Statement(s)),
                        TopItem::Block { head, body } => out.push(Item::Block { head, body }),
                        other => hoisted.push(other),
                    }
                }
                continue;
            }
        }
        let item = match item {
            Item::Block { head, mut body } => {
                expand_items(&mut body, programs, stack, hoisted);
                Item::Block { head, body }
            }
            other => other,
        };
        out.push(item);
    }
    *items = out;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prog(src: &str) -> Program {
        parse_l1(src)
    }

    fn lib(files: &[(&str, &str)]) -> BTreeMap<String, Program> {
        files
            .iter()
            .map(|(n, s)| (n.to_string(), prog(s)))
            .collect()
    }

    #[test]
    fn top_level_include_is_replaced_by_its_items() {
        let libs = lib(&[("inc", "WRITE 'a'.\n")]);
        let mut main = prog("REPORT zt.\nINCLUDE inc.\nWRITE 'b'.\n");
        expand_includes(&mut main, &libs);
        let kinds: Vec<String> = main
            .items
            .iter()
            .map(|i| match i {
                TopItem::Statement(s) => s.text(),
                _ => "?".into(),
            })
            .collect();
        assert_eq!(kinds, vec!["REPORT zt", "WRITE 'a'", "WRITE 'b'"]);
    }

    #[test]
    fn nested_include_keeps_statements_and_hoists_routines() {
        let libs = lib(&[(
            "dyn",
            "MODULE status OUTPUT.\n  WRITE 'x'.\nENDMODULE.\nWRITE 'y'.\n",
        )]);
        let mut main = prog("REPORT zt.\nSTART-OF-SELECTION.\n  INCLUDE dyn.\n  WRITE 'z'.\n");
        expand_includes(&mut main, &libs);
        // MODULE wandert auf die oberste Ebene, WRITE bleibt im Block
        assert!(main
            .items
            .iter()
            .any(|i| matches!(i, TopItem::Routine { kind, .. } if kind == "MODULE")));
        let block = main
            .items
            .iter()
            .find_map(|i| match i {
                TopItem::Routine { body, .. } => Some(body),
                _ => None,
            })
            .expect("START-OF-SELECTION");
        let texts: Vec<String> = block
            .iter()
            .map(|i| match i {
                Item::Statement(s) => s.text(),
                _ => "?".into(),
            })
            .collect();
        assert_eq!(texts, vec!["WRITE 'y'", "WRITE 'z'"]);
    }

    #[test]
    fn cyclic_include_is_not_expanded_twice() {
        let libs = lib(&[("a", "INCLUDE a.\nWRITE 'a'.\n")]);
        let mut main = prog("REPORT zt.\nINCLUDE a.\n");
        expand_includes(&mut main, &libs);
        let writes = main
            .items
            .iter()
            .filter(|i| matches!(i, TopItem::Statement(s) if s.text() == "WRITE 'a'"))
            .count();
        assert_eq!(writes, 1);
        // der zyklische Include bleibt als unaufgelöste Anweisung stehen
        assert!(main
            .items
            .iter()
            .any(|i| matches!(i, TopItem::Statement(s) if s.keyword() == "INCLUDE")));
    }

    #[test]
    fn missing_include_stays_unresolved() {
        let libs = lib(&[]);
        let mut main = prog("REPORT zt.\nINCLUDE nichtda.\n");
        expand_includes(&mut main, &libs);
        assert!(main
            .items
            .iter()
            .any(|i| matches!(i, TopItem::Statement(s) if s.keyword() == "INCLUDE")));
    }
}

/// DDIC-Definitionen (TABL, DTEL, DOMA) aus einem Verzeichnis mit abapGit-XML-Dateien (`--ddic`).
/// Nur die Dateien unmittelbar in diesem Verzeichnis, sortiert nach Dateiname.
pub fn load_ddic_dir(dir: &Path) -> Result<Ddic, String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut paths: Vec<std::path::PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            let name = p.to_string_lossy();
            name.ends_with(".tabl.xml")
                || name.ends_with(".dtel.xml")
                || name.ends_with(".doma.xml")
        })
        .collect();
    paths.sort();
    let mut files = Vec::new();
    for path in paths {
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        files.push((path.to_string_lossy().to_string(), text));
    }
    Ok(ddic::from_files(&files))
}

/// DDIC-Definitionen (TABL, DTEL, DOMA) eines ZIP-Exports.
pub fn load_ddic(zip_path: &Path) -> Result<Ddic, String> {
    let file = File::open(zip_path).map_err(|e| format!("{}: {e}", zip_path.display()))?;
    let mut archive = ZipArchive::new(file).map_err(|e| format!("Invalid ZIP: {e}"))?;
    let mut files = Vec::new();
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("ZIP entry {i}: {e}"))?;
        let name = entry.name().to_string();
        if name.ends_with(".tabl.xml") || name.ends_with(".dtel.xml") || name.ends_with(".doma.xml")
        {
            let mut text = String::new();
            entry
                .read_to_string(&mut text)
                .map_err(|e| format!("{name}: {e}"))?;
            files.push((name, text));
        }
    }
    Ok(ddic::from_files(&files))
}
