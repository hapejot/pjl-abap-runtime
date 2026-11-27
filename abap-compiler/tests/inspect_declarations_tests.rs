use abap_compiler::inspect::abap::{declarations, parse_l1};
use abap_compiler::inspect::objects::{collect, render};

fn decls(src: &str) -> Vec<(String, String)> {
    declarations(&parse_l1(src))
        .into_iter()
        .map(|d| (d.kind, d.name))
        .collect()
}

#[test]
fn collects_data_types_constants_in_order() {
    let d = decls("DATA a TYPE i.\nCONSTANTS c TYPE i VALUE 1.\nTYPES t TYPE i.");
    assert_eq!(
        d,
        vec![
            ("DATA".into(), "a".into()),
            ("CONSTANTS".into(), "c".into()),
            ("TYPES".into(), "t".into()),
        ]
    );
}

#[test]
fn chained_data_yields_one_declaration_per_name() {
    let d = decls("DATA: a TYPE i, b TYPE i.");
    assert_eq!(
        d,
        vec![("DATA".into(), "a".into()), ("DATA".into(), "b".into())]
    );
}

#[test]
fn structure_components_are_not_top_level() {
    let d = decls("TYPES: BEGIN OF ts,\n  a TYPE i,\n  b TYPE i,\nEND OF ts.");
    assert_eq!(d, vec![("TYPES".into(), "ts".into())]);
}

#[test]
fn begin_of_data_structure_reports_structure_name() {
    let d = decls("DATA: BEGIN OF gs,\n  x TYPE i,\nEND OF gs.\nDATA y TYPE i.");
    assert_eq!(
        d,
        vec![("DATA".into(), "gs".into()), ("DATA".into(), "y".into())]
    );
}

#[test]
fn form_function_and_module_are_declarations() {
    let d = decls("FORM run.\nENDFORM.\nFUNCTION zf.\nENDFUNCTION.\nMODULE m OUTPUT.\nENDMODULE.");
    assert_eq!(
        d,
        vec![
            ("FORM".into(), "run".into()),
            ("FUNCTION".into(), "zf".into()),
            ("MODULE".into(), "m".into()),
        ]
    );
}

#[test]
fn event_blocks_are_reported_as_event() {
    let d = decls("START-OF-SELECTION.\n  WRITE 'x'.");
    assert_eq!(d, vec![("EVENT".into(), "start-of-selection".into())]);
}

#[test]
fn statements_inside_blocks_are_not_top_level() {
    let d = decls("IF a = b.\n  DATA x TYPE i.\nENDIF.");
    assert!(d.is_empty());
}

#[test]
fn class_definition_has_members_as_children() {
    let src = "CLASS lcl DEFINITION.\n  PUBLIC SECTION.\n    DATA mv TYPE i.\n    METHODS run.\n    CLASS-METHODS make.\nENDCLASS.";
    let prog = parse_l1(src);
    let d = declarations(&prog);
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].kind, "CLASS");
    assert_eq!(d[0].name, "lcl");
    let kids: Vec<(String, String)> = d[0]
        .children
        .iter()
        .map(|c| (c.kind.clone(), c.name.clone()))
        .collect();
    assert_eq!(
        kids,
        vec![
            ("DATA".into(), "mv".into()),
            ("METHODS".into(), "run".into()),
            ("CLASS-METHODS".into(), "make".into()),
        ]
    );
}

#[test]
fn class_implementation_adds_no_top_level_entry() {
    let d = decls("CLASS lcl IMPLEMENTATION.\n  METHOD go.\n  ENDMETHOD.\nENDCLASS.");
    assert!(d.is_empty());
}

#[test]
fn interface_definition_is_reported() {
    let d = decls("INTERFACE zif.\n  METHODS m.\nENDINTERFACE.");
    assert_eq!(d, vec![("INTERFACE".into(), "zif".into())]);
}

#[test]
fn files_of_one_object_are_grouped() {
    let files = vec![
        (
            "src/zfoo.prog.abap".to_string(),
            "DATA a TYPE i.".to_string(),
        ),
        (
            "src/zfoo.prog.screen_0100.abap".to_string(),
            "PROCESS BEFORE OUTPUT.".to_string(),
        ),
        (
            "src/zbar.clas.abap".to_string(),
            "CLASS zbar DEFINITION.\nENDCLASS.".to_string(),
        ),
        ("src/.abapgit.xml".to_string(), "<xml/>".to_string()),
    ];
    let objs = collect(&files);
    assert_eq!(objs.len(), 2);
    let prog = objs.iter().find(|o| o.obj_type == "PROG").unwrap();
    assert_eq!(prog.name, "zfoo");
    assert_eq!(prog.decls.len(), 1);
    assert_eq!(prog.decls[0].0, "zfoo.prog.abap");
}

#[test]
fn namespace_hash_is_decoded_in_object_name() {
    let files = vec![(
        "#ns#cl_x.clas.abap".to_string(),
        "CLASS x DEFINITION.\nENDCLASS.".to_string(),
    )];
    let objs = collect(&files);
    assert_eq!(objs[0].name, "/ns/cl_x");
}

#[test]
fn render_lists_object_and_declarations() {
    let files = vec![(
        "zfoo.prog.abap".to_string(),
        "DATA a TYPE i.\nFORM run.\nENDFORM.".to_string(),
    )];
    let text = render(&collect(&files));
    assert!(text.starts_with("PROG zfoo\n"));
    assert!(text.contains("DATA a (L1)"));
    assert!(text.contains("FORM run (L2)"));
}

#[test]
fn include_is_expanded_in_place_and_hidden_as_object() {
    let files = vec![
        (
            "src/zmain.prog.abap".to_string(),
            "DATA a TYPE i.\nINCLUDE ztop.\nDATA z TYPE i.".to_string(),
        ),
        (
            "src/ztop.prog.abap".to_string(),
            "DATA t TYPE i.".to_string(),
        ),
    ];
    let objs = collect(&files);
    assert_eq!(objs.len(), 1, "ztop must not appear as its own object");
    assert_eq!(objs[0].name, "zmain");
    let text = render(&objs);
    let inc = text.find("INCLUDE ztop").unwrap();
    let inner = text.find("DATA t").unwrap();
    let after = text.find("DATA z").unwrap();
    assert!(
        inc < inner && inner < after,
        "Include-Inhalt muss zwischen die Nachbarn"
    );
}

#[test]
fn include_cycle_does_not_recurse_forever() {
    let files = vec![
        (
            "src/za.prog.abap".to_string(),
            "INCLUDE zb.\nDATA a TYPE i.".to_string(),
        ),
        (
            "src/zb.prog.abap".to_string(),
            "INCLUDE za.\nDATA b TYPE i.".to_string(),
        ),
    ];
    let objs = collect(&files);
    assert!(objs.len() <= 2);
    let text = render(&objs);
    assert!(text.contains("DATA a"));
}

#[test]
fn fugr_include_stays_within_function_group() {
    let files = vec![
        (
            "src/zfg.fugr.saplzfg.abap".to_string(),
            "INCLUDE lzfgtop.\nFUNCTION zf.\nENDFUNCTION.".to_string(),
        ),
        (
            "src/zfg.fugr.lzfgtop.abap".to_string(),
            "DATA g TYPE i.".to_string(),
        ),
    ];
    let objs = collect(&files);
    assert_eq!(objs.len(), 1);
    assert_eq!(objs[0].obj_type, "FUGR");
    let text = render(&objs);
    assert!(text.contains("DATA g (L1)"));
    assert!(text.contains("FUNCTION zf"));
}

#[test]
fn method_declaration_links_to_implementation_line() {
    let src = "CLASS lcl DEFINITION.\n  PUBLIC SECTION.\n    METHODS run IMPORTING iv_x TYPE i.\nENDCLASS.\n\nCLASS lcl IMPLEMENTATION.\n  METHOD run.\n  ENDMETHOD.\nENDCLASS.";
    let d = declarations(&parse_l1(src));
    assert_eq!(d.len(), 1);
    let m = &d[0].children[0];
    assert_eq!(m.name, "run");
    assert_eq!(m.impl_line, Some(7));
    assert!(m.detail.starts_with("METHODS run IMPORTING iv_x TYPE i"));
}

#[test]
fn entry_target_prefers_implementation() {
    let files = vec![(
        "zc.clas.abap".to_string(),
        "CLASS zc DEFINITION.\n  PUBLIC SECTION.\n    METHODS go.\nENDCLASS.\nCLASS zc IMPLEMENTATION.\n  METHOD go.\n  ENDMETHOD.\nENDCLASS.".to_string(),
    )];
    let objs = collect(&files);
    let entries = abap_compiler::inspect::objects::entries(&objs[0]);
    let go = entries.iter().find(|e| e.label == "METHODS go").unwrap();
    assert_eq!(go.line, 3);
    assert_eq!(go.target_line(), 6);
}
