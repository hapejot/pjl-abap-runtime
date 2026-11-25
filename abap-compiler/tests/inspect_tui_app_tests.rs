use std::collections::BTreeMap;

use abap_compiler::inspect::objects::Entry;
use abap_compiler::inspect::tui::app::{App, Object, Sources};
use crossterm::event::{KeyCode, KeyModifiers};

fn entry(depth: usize, label: &str, file: &str, line: usize) -> Entry {
    Entry {
        depth,
        label: label.into(),
        file: file.into(),
        line,
        detail: String::new(),
        impl_line: None,
    }
}

fn sample() -> App {
    let objects = vec![
        Object {
            key: ("CLAS".into(), "zcl_a".into()),
            entries: vec![
                entry(0, "CLASS zcl_a", "zcl_a.clas.abap", 1),
                entry(1, "METHODS run", "zcl_a.clas.abap", 5),
            ],
        },
        Object { key: ("PROG".into(), "zfoo".into()), entries: vec![entry(0, "FORM fill", "zfoo.prog.abap", 3)] },
        Object { key: ("PROG".into(), "zbar".into()), entries: vec![] },
    ];
    let mut sources: Sources = BTreeMap::new();
    sources.insert(
        ("CLAS".into(), "zcl_a".into()),
        vec![("zcl_a.clas.abap".into(), "CLASS zcl_a DEFINITION.\n\tENDCLASS.\n\nMETHOD run.\nENDMETHOD.".into())],
    );
    sources.insert(("PROG".into(), "zfoo".into()), vec![("zfoo.prog.abap".into(), "DATA a.\nFORM fill.\nENDFORM.".into())]);
    App::new(objects, sources)
}

fn key(app: &mut App, c: KeyCode) {
    app.handle_key(c, KeyModifiers::NONE, 10);
}

#[test]
fn starts_with_all_objects_and_first_selected() {
    let app = sample();
    assert_eq!(app.rows.len(), 6);
    assert_eq!(app.current().unwrap().1, "zcl_a");
}

#[test]
fn down_moves_through_entries_then_to_next_object() {
    let mut app = sample();
    key(&mut app, KeyCode::Down);
    assert_eq!(app.current().unwrap().1, "zcl_a", "Eintrag gehoert zum Objekt");
    assert_eq!(app.current_entry().unwrap().label, "CLASS zcl_a");
    key(&mut app, KeyCode::Down);
    key(&mut app, KeyCode::Down);
    assert_eq!(app.current().unwrap().1, "zfoo");
    assert!(app.current_entry().is_none());
    assert!(app.lines.iter().any(|l| l == "DATA a."));
}

#[test]
fn entry_selection_scrolls_source_to_its_line() {
    let mut app = sample();
    // Zeilen: 0 zcl_a, 1 CLASS, 2 METHODS run (Zeile 5 in der Datei)
    key(&mut app, KeyCode::Down);
    key(&mut app, KeyCode::Down);
    assert_eq!(app.current_entry().unwrap().label, "METHODS run");
    let header = app.lines.iter().position(|l| l.starts_with("---- zcl_a")).unwrap();
    assert_eq!(app.src_off, header + 5 - 2);
    assert!(!app.focus_src);
}

#[test]
fn enter_on_entry_moves_focus_to_source() {
    let mut app = sample();
    key(&mut app, KeyCode::Enter);
    assert!(!app.focus_src, "Enter auf Objekt wechselt nicht");
    key(&mut app, KeyCode::Down);
    key(&mut app, KeyCode::Enter);
    assert!(app.focus_src);
}

#[test]
fn selection_is_clamped_at_ends() {
    let mut app = sample();
    key(&mut app, KeyCode::Up);
    assert_eq!(app.sel, 0);
    key(&mut app, KeyCode::End);
    assert_eq!(app.sel, 5);
    key(&mut app, KeyCode::Down);
    assert_eq!(app.sel, 5);
}

#[test]
fn filter_matches_name_and_type_case_insensitive() {
    let mut app = sample();
    key(&mut app, KeyCode::Char('/'));
    for c in "ZBA".chars() {
        key(&mut app, KeyCode::Char(c));
    }
    assert_eq!(app.rows.len(), 1);
    assert_eq!(app.current().unwrap().1, "zbar");

    key(&mut app, KeyCode::Enter);
    assert!(!app.filter_mode);
    key(&mut app, KeyCode::Esc);
    assert_eq!(app.rows.len(), 6);
}

#[test]
fn filter_by_type() {
    let mut app = sample();
    key(&mut app, KeyCode::Char('/'));
    for c in "prog".chars() {
        key(&mut app, KeyCode::Char(c));
    }
    assert_eq!(app.rows.len(), 3);
}

#[test]
fn backspace_widens_filter_again() {
    let mut app = sample();
    key(&mut app, KeyCode::Char('/'));
    key(&mut app, KeyCode::Char('x'));
    assert_eq!(app.rows.len(), 0);
    key(&mut app, KeyCode::Backspace);
    assert_eq!(app.rows.len(), 6);
}

#[test]
fn tab_switches_focus_and_arrows_scroll_source() {
    let mut app = sample();
    key(&mut app, KeyCode::Tab);
    assert!(app.focus_src);
    let before = app.src_off;
    key(&mut app, KeyCode::Down);
    assert_eq!(app.src_off, before + 1);
    assert_eq!(app.sel, 0, "Auswahl bleibt, wenn der Quelltext Fokus hat");
}

#[test]
fn source_scroll_is_clamped() {
    let mut app = sample();
    app.scroll_src(1000);
    assert_eq!(app.src_off, app.lines.len() - 1);
    app.scroll_src(-1000);
    assert_eq!(app.src_off, 0);
}

#[test]
fn tabs_in_source_are_expanded() {
    let app = sample();
    assert!(app.lines.iter().all(|l| !l.contains('\t')));
}

#[test]
fn q_and_ctrl_c_quit() {
    let mut app = sample();
    key(&mut app, KeyCode::Char('q'));
    assert!(app.quit);

    let mut app = sample();
    app.handle_key(KeyCode::Char('c'), KeyModifiers::CONTROL, 10);
    assert!(app.quit);
}

#[test]
fn empty_filter_result_has_no_selection() {
    let mut app = sample();
    key(&mut app, KeyCode::Char('/'));
    key(&mut app, KeyCode::Char('z'));
    key(&mut app, KeyCode::Char('z'));
    assert!(app.current().is_none());
    assert!(app.lines.is_empty());
}
