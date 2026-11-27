use abap_compiler::inspect::abap::requirements::{derive, Requirements, TypeKind};

fn reqs(files: &[(&str, &str)]) -> Requirements {
    let owned: Vec<(String, String)> = files
        .iter()
        .map(|(p, s)| (p.to_string(), s.to_string()))
        .collect();
    derive(&owned)
}

fn find<'a>(
    r: &'a Requirements,
    name: &str,
) -> &'a abap_compiler::inspect::abap::requirements::Requirement {
    r.items
        .iter()
        .find(|i| i.name == name)
        .unwrap_or_else(|| panic!("no requirement for {name}: {:#?}", r.items))
}

const APP: &str = "CLASS zcl_app DEFINITION.\n  PUBLIC SECTION.\n    METHODS run.\nENDCLASS.\n\
CLASS zcl_app IMPLEMENTATION.\n  METHOD run.\n    DATA lo TYPE REF TO zcl_ext.\n    DATA(lx) = NEW zcl_ext( ).\n\
    lo->zif_ext~do_it( ).\n    lo->get_name( ).\n    zcx_base=>raise_it( ).\n  ENDMETHOD.\nENDCLASS.";

#[test]
fn external_class_interface_and_methods_are_derived() {
    let r = reqs(&[("zcl_app.clas.abap", APP)]);

    let ext = find(&r, "zcl_ext");
    assert_eq!(ext.kind, TypeKind::Class);
    assert!(!ext.in_export);
    assert_eq!(ext.methods, vec!["get_name"]);
    assert_eq!(ext.implements, vec!["zif_ext"]);

    let intf = find(&r, "zif_ext");
    assert_eq!(intf.kind, TypeKind::Interface);
    assert_eq!(intf.methods, vec!["do_it"]);

    let base = find(&r, "zcx_base");
    assert_eq!(base.kind, TypeKind::Class);
    assert_eq!(base.methods, vec!["raise_it"]);
}

#[test]
fn types_in_export_are_not_external() {
    let ext = "CLASS zcl_ext DEFINITION.\n  PUBLIC SECTION.\n    METHODS get_name.\nENDCLASS.\n\
CLASS zcl_ext IMPLEMENTATION.\n  METHOD get_name.\n  ENDMETHOD.\nENDCLASS.";
    let r = reqs(&[("zcl_app.clas.abap", APP), ("zcl_ext.clas.abap", ext)]);
    assert!(find(&r, "zcl_ext").in_export);
    assert!(!r.external().any(|i| i.name == "zcl_ext"));
    assert!(r.external().any(|i| i.name == "zcx_base"));
}

#[test]
fn plain_ddic_types_are_not_listed() {
    let src = "REPORT zrep.\nDATA lv_x TYPE mara-matnr.\nDATA ls TYPE vbak.\n";
    let r = reqs(&[("zrep.prog.abap", src)]);
    assert!(r.items.is_empty(), "{:#?}", r.items);
}

#[test]
fn event_handler_sender_is_typed_by_event_class() {
    let src = "CLASS zcl_h DEFINITION.\n  PUBLIC SECTION.\n    METHODS on_drop\n      FOR EVENT on_drop OF cl_gui_alv_tree\n      IMPORTING sender.\nENDCLASS.\n\
CLASS zcl_h IMPLEMENTATION.\n  METHOD on_drop.\n    sender->get_outtab_line( ).\n  ENDMETHOD.\nENDCLASS.";
    let r = reqs(&[("zcl_h.clas.abap", src)]);
    let tree = find(&r, "cl_gui_alv_tree");
    assert_eq!(tree.kind, TypeKind::Class);
    assert_eq!(tree.methods, vec!["get_outtab_line"]);
}

#[test]
fn interface_use_in_class_definition_is_recorded() {
    let src = "CLASS zcl_impl DEFINITION.\n  PUBLIC SECTION.\n    INTERFACES zif_ext.\nENDCLASS.\n\
CLASS zcl_impl IMPLEMENTATION.\nENDCLASS.";
    let r = reqs(&[("zcl_impl.clas.abap", src)]);
    assert_eq!(find(&r, "zif_ext").kind, TypeKind::Interface);
    assert_eq!(find(&r, "zcl_impl").implements, vec!["zif_ext"]);
}
