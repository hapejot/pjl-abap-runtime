use abapgit_inspect::abap::graph::{build, EdgeKind, Graph, NodeKind};

fn graph(files: &[(&str, &str)]) -> Graph {
    let owned: Vec<(String, String)> = files
        .iter()
        .map(|(p, s)| (p.to_string(), s.to_string()))
        .collect();
    build(&owned)
}

fn edge<'a>(g: &'a Graph, from: &str, kind: EdgeKind, hint: &str) -> &'a abapgit_inspect::abap::graph::Edge {
    g.edges
        .iter()
        .find(|e| e.from == from && e.kind == kind && e.hint == hint)
        .unwrap_or_else(|| panic!("keine Kante {from} {kind:?} {hint}: {:#?}", g.edges))
}

const ZCL_A: &str = "CLASS zcl_a DEFINITION.\n  PUBLIC SECTION.\n    METHODS run.\n    METHODS helper.\nENDCLASS.\n\
CLASS zcl_a IMPLEMENTATION.\n  METHOD run.\n    DATA lo TYPE REF TO zcl_b.\n    lo->save( ).\n    zcl_b=>create( ).\n    me->helper( ).\n  ENDMETHOD.\n  METHOD helper.\n  ENDMETHOD.\nENDCLASS.";

const ZCL_B: &str = "CLASS zcl_b DEFINITION.\n  PUBLIC SECTION.\n    CLASS-METHODS create.\n    METHODS save.\nENDCLASS.\n\
CLASS zcl_b IMPLEMENTATION.\n  METHOD create.\n  ENDMETHOD.\n  METHOD save.\n  ENDMETHOD.\nENDCLASS.";

#[test]
fn method_calls_resolve_where_possible() {
    let g = graph(&[("zcl_a.clas.abap", ZCL_A), ("zcl_b.clas.abap", ZCL_B)]);

    let me = edge(&g, "zcl_a=>run", EdgeKind::MethodCall, "me->helper");
    assert_eq!(me.to.as_deref(), Some("zcl_a=>helper"));

    let stat = edge(&g, "zcl_a=>run", EdgeKind::StaticMethodCall, "zcl_b=>create");
    assert_eq!(stat.to.as_deref(), Some("zcl_b=>create"));

    // Typ von `lo` wird nicht verfolgt
    let dynamic = edge(&g, "zcl_a=>run", EdgeKind::MethodCall, "lo->save");
    assert_eq!(dynamic.to, None);
}

#[test]
fn method_nodes_point_to_implementation() {
    let g = graph(&[("zcl_a.clas.abap", ZCL_A), ("zcl_b.clas.abap", ZCL_B)]);
    let save = g.nodes.iter().find(|n| n.id == "zcl_b=>save").expect("Knoten fehlt");
    assert_eq!(save.kind, NodeKind::Method);
    assert_eq!(save.file, "zcl_b.clas.abap");
    assert_eq!(save.line, 9);
}

#[test]
fn type_reference_is_an_object_dependency() {
    let g = graph(&[("zcl_a.clas.abap", ZCL_A), ("zcl_b.clas.abap", ZCL_B)]);
    let ty = edge(&g, "zcl_a=>run", EdgeKind::TypeReference, "zcl_b");
    assert_eq!(ty.to.as_deref(), Some("zcl_b"));
    assert!(g.object_dependencies().contains(&("zcl_a".to_string(), "zcl_b".to_string())));
}

#[test]
fn inherited_method_is_resolved_via_superclass() {
    let base = "CLASS zcl_base DEFINITION.\n  PUBLIC SECTION.\n    METHODS inherited.\nENDCLASS.\n\
CLASS zcl_base IMPLEMENTATION.\n  METHOD inherited.\n  ENDMETHOD.\nENDCLASS.";
    let sub = "CLASS zcl_sub DEFINITION INHERITING FROM zcl_base.\n  PUBLIC SECTION.\n    METHODS go.\nENDCLASS.\n\
CLASS zcl_sub IMPLEMENTATION.\n  METHOD go.\n    me->inherited( ).\n  ENDMETHOD.\nENDCLASS.";
    let g = graph(&[("zcl_base.clas.abap", base), ("zcl_sub.clas.abap", sub)]);

    let inh = edge(&g, "zcl_sub", EdgeKind::Inheritance, "zcl_base");
    assert_eq!(inh.to.as_deref(), Some("zcl_base"));

    let call = edge(&g, "zcl_sub=>go", EdgeKind::MethodCall, "me->inherited");
    assert_eq!(call.to.as_deref(), Some("zcl_base=>inherited"));
}

#[test]
fn perform_and_function_calls() {
    let report = "REPORT zreport.\nPERFORM calc USING 1.\nPERFORM missing.\n\
START-OF-SELECTION.\n  CALL FUNCTION 'Z_FM_ONE'.\n  CALL FUNCTION 'Z_NOT_THERE'.\n\
FORM calc USING p TYPE i.\nENDFORM.";
    let fugr = "FUNCTION z_fm_one.\nENDFUNCTION.";
    let g = graph(&[("zreport.prog.abap", report), ("zfg.fugr.zfg_fm.abap", fugr)]);

    let perf = edge(&g, "zreport", EdgeKind::Perform, "calc");
    assert_eq!(perf.to.as_deref(), Some("zreport::form:calc"));
    assert_eq!(edge(&g, "zreport", EdgeKind::Perform, "missing").to, None);

    let ev = "zreport::event:start-of-selection";
    assert!(g.nodes.iter().any(|n| n.id == ev && n.kind == NodeKind::Event));
    assert_eq!(
        edge(&g, ev, EdgeKind::FunctionCall, "z_fm_one").to.as_deref(),
        Some("fm:z_fm_one")
    );
    assert_eq!(edge(&g, ev, EdgeKind::FunctionCall, "z_not_there").to, None);
}

#[test]
fn local_and_builtin_types_are_not_dependencies() {
    let src = "CLASS zcl_t DEFINITION.\n  PUBLIC SECTION.\n    TYPES: BEGIN OF ty_s,\n      a TYPE i,\n    END OF ty_s.\n\
    METHODS m IMPORTING is TYPE ty_s iv TYPE ztab-field RETURNING VALUE(r) TYPE string.\nENDCLASS.\n\
CLASS zcl_t IMPLEMENTATION.\n  METHOD m.\n  ENDMETHOD.\nENDCLASS.";
    let g = graph(&[("zcl_t.clas.abap", src)]);

    let hints: Vec<&str> = g
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::TypeReference)
        .map(|e| e.hint.as_str())
        .collect();
    assert_eq!(hints, vec!["ztab"]);
    let ztab = edge(&g, "zcl_t", EdgeKind::TypeReference, "ztab");
    assert_eq!(ztab.to, None);
}

#[test]
fn reachable_follows_resolved_edges_only() {
    let g = graph(&[("zcl_a.clas.abap", ZCL_A), ("zcl_b.clas.abap", ZCL_B)]);
    let reach = g.reachable_from("zcl_a=>run");
    assert!(reach.contains("zcl_b=>create"));
    assert!(reach.contains("zcl_a=>helper"));
    assert!(!reach.contains("zcl_b=>save"));
}
