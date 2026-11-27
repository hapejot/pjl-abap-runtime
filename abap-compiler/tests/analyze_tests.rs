use abap_compiler::analyze::{analyze, Report};

fn report(src: &str) -> Report {
    analyze(src, "zt")
}

fn class_of<'a>(r: &'a Report, name: &str) -> Option<&'a str> {
    r.names
        .iter()
        .find(|n| n.name == name)
        .map(|n| n.classification)
}

#[test]
fn parameter_declared_in_source_is_a_compiler_gap() {
    let src = "REPORT zt.\n\
CLASS lcl DEFINITION.\n  PUBLIC SECTION.\n    METHODS run IMPORTING VALUE(ri_x) TYPE REF TO zif_x.\nENDCLASS.\n\
CLASS lcl IMPLEMENTATION.\n  METHOD run.\n    ri_x->go( ).\n  ENDMETHOD.\nENDCLASS.\n";
    let r = report(src);
    assert_eq!(
        class_of(&r, "ri_x"),
        Some("im_quelltext_deklariert_compiler_kennt_es_nicht")
    );
    let name = r.names.iter().find(|n| n.name == "ri_x").unwrap();
    assert_eq!(name.inferred_kind, "objektreferenz");
    assert!(name.declared_at_line.is_some());
}

#[test]
fn unknown_name_gets_inferred_kind_from_usage() {
    let src = "REPORT zt.\nSTART-OF-SELECTION.\n  zcl_ext=>run( ).\n  lo_thing->go( ).\n";
    let r = report(src);
    let stat = r
        .by_classification
        .get("unbekannt_extern_oder_fehlend")
        .expect("unknown names expected");
    assert!(stat.names >= 1);
    let ext = r.names.iter().find(|n| n.name == "zcl_ext");
    let ext = ext.expect("zcl_ext wird als extern erkannt");
    assert_eq!(ext.classification, "extern_klasse_oder_interface");
    assert_eq!(ext.inferred_kind, "class");
}

#[test]
fn messages_without_names_are_counted_by_template() {
    let src = "REPORT zt.\nDATA t TYPE TABLE OF i.\nSTART-OF-SELECTION.\n  SORT t.\n";
    let r = report(src);
    assert!(r.total_messages >= 1);
    assert!(r
        .other_messages
        .keys()
        .any(|k| k.contains("Unsupported statement")));
}

#[test]
fn external_object_in_library_is_known_others_are_unknown() {
    use abap_compiler::analyze::analyze_with;
    use abap_compiler::library::{Library, Object, ObjectKind, Origin, Status};

    let mut lib = Library::new("test", Origin::Manual);
    lib.insert(Object::new(
        "cl_gui_alv_tree",
        ObjectKind::Class,
        Status::SignatureOnly,
    ));
    let src =
        "REPORT zt.\nSTART-OF-SELECTION.\n  cl_gui_alv_tree=>run( ).\n  zcl_unbekannt=>run( ).\n";
    let r = analyze_with(src, "zt", &[lib]);

    let known = r
        .names
        .iter()
        .find(|n| n.name == "cl_gui_alv_tree")
        .expect("name expected");
    assert_eq!(known.classification, "in_bibliothek");
    assert_eq!(known.library.as_deref(), Some("test"));
    assert!(r.unknown.iter().any(|o| o.name == "zcl_unbekannt"));
    assert!(!r.unknown.iter().any(|o| o.name == "cl_gui_alv_tree"));
}
