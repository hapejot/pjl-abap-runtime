use abap_compiler::inspect::abap::{parse_l1, ClassKind, Item, MemberKind, TopItem, Visibility};

fn program(src: &str) -> abap_compiler::inspect::abap::Program {
    parse_l1(src)
}

#[test]
fn chained_statement_is_split() {
    let prog = program("DATA: a TYPE i, b TYPE string.");
    assert!(prog.errors.is_empty());
    assert_eq!(prog.items.len(), 2);
    let texts: Vec<String> = prog
        .items
        .iter()
        .map(|i| match i {
            TopItem::Statement(s) => s.text(),
            _ => panic!("expected statement"),
        })
        .collect();
    assert_eq!(texts, vec!["DATA a TYPE i", "DATA b TYPE string"]);
}

#[test]
fn if_block_with_else_is_nested() {
    let prog = program("IF a = b.\n  x = 1.\nELSE.\n  x = 2.\nENDIF.");
    assert!(prog.errors.is_empty());
    match &prog.items[0] {
        TopItem::Block { head, body } => {
            assert_eq!(head.keyword(), "IF");
            // x = 1, ELSE, x = 2
            assert_eq!(body.len(), 3);
        }
        _ => panic!("expected block"),
    }
}

#[test]
fn select_single_is_not_a_block() {
    let prog = program("SELECT SINGLE a FROM t INTO @DATA(x).");
    assert!(prog.errors.is_empty());
    assert!(matches!(prog.items[0], TopItem::Statement(_)));
}

#[test]
fn aggregate_select_without_group_by_is_not_a_block() {
    let prog = program(
        "LOOP AT it_where INTO lv_where.\n\
         SELECT COUNT(*) FROM (iv_table_name) INTO lv_count WHERE (lv_where).\n\
         lv_total = lv_total + lv_count.\n\
         ENDLOOP.",
    );
    assert!(prog.errors.is_empty(), "{:?}", prog.errors);
    assert!(matches!(&prog.items[0], TopItem::Block { head, .. } if head.keyword() == "LOOP"));
}

#[test]
fn aggregate_select_with_group_by_is_a_block() {
    let prog = program(
        "SELECT carrier COUNT(*) FROM flights GROUP BY carrier INTO (carrier, count).\n\
         WRITE count.\n\
         ENDSELECT.",
    );
    assert!(prog.errors.is_empty(), "{:?}", prog.errors);
    assert!(matches!(&prog.items[0], TopItem::Block { head, .. } if head.keyword() == "SELECT"));
}

#[test]
fn select_loop_is_a_block() {
    let prog = program("SELECT a FROM t INTO wa.\n  WRITE wa.\nENDSELECT.");
    assert!(prog.errors.is_empty());
    assert!(matches!(prog.items[0], TopItem::Block { .. }));
}

#[test]
fn unmatched_end_is_reported() {
    let prog = program("ENDIF.");
    assert_eq!(prog.errors.len(), 1);
}

#[test]
fn missing_endif_is_reported() {
    let prog = program("IF a = b.\n  x = 1.\n");
    assert_eq!(prog.errors.len(), 1);
    assert!(prog.errors[0].message.contains("ENDIF"));
}

#[test]
fn module_call_without_definition_is_statement() {
    let prog = program("PROCESS BEFORE OUTPUT.\n  MODULE status_0100.\n");
    assert!(prog.errors.is_empty());
}

#[test]
fn module_definition_requires_endmodule() {
    let prog = program("MODULE status OUTPUT.\n  x = 1.\nENDMODULE.");
    assert!(prog.errors.is_empty());
    match &prog.items[0] {
        TopItem::Routine { kind, name, .. } => {
            assert_eq!(kind, "MODULE");
            assert_eq!(name, "status");
        }
        _ => panic!("expected routine"),
    }
}

#[test]
fn form_routine_is_parsed() {
    let prog = program("FORM run USING p TYPE i.\n  WRITE p.\nENDFORM.");
    assert!(prog.errors.is_empty());
    match &prog.items[0] {
        TopItem::Routine {
            kind, name, body, ..
        } => {
            assert_eq!(kind, "FORM");
            assert_eq!(name, "run");
            assert_eq!(body.len(), 1);
        }
        _ => panic!("expected routine"),
    }
}

#[test]
fn event_block_ends_at_next_event() {
    let prog = program("START-OF-SELECTION.\n  x = 1.\nEND-OF-SELECTION.\n  y = 2.");
    assert!(prog.errors.is_empty());
    assert_eq!(prog.items.len(), 2);
    match &prog.items[0] {
        TopItem::Routine { kind, body, .. } => {
            assert_eq!(kind, "START-OF-SELECTION");
            assert_eq!(body.len(), 1);
        }
        _ => panic!("expected event"),
    }
}

#[test]
fn initialization_block_ends_at_start_of_selection() {
    let prog = program("INITIALIZATION.\n  x = 1.\nSTART-OF-SELECTION.\n  x = 2.");
    assert!(prog.errors.is_empty(), "{:?}", prog.errors);
    assert_eq!(prog.items.len(), 2);
    assert!(matches!(&prog.items[0], TopItem::Routine { kind, body, .. }
        if kind == "INITIALIZATION" && body.len() == 1));
}

#[test]
fn class_definition_members_and_visibility() {
    let src = "CLASS lcl DEFINITION.\n  PUBLIC SECTION.\n    METHODS run IMPORTING iv_x TYPE i RETURNING VALUE(rv) TYPE string.\n  PRIVATE SECTION.\n    DATA mv TYPE i.\nENDCLASS.";
    let prog = program(src);
    assert!(prog.errors.is_empty(), "{:?}", prog.errors);
    let TopItem::Class(c) = &prog.items[0] else {
        panic!("expected class")
    };
    assert_eq!(c.kind, ClassKind::Definition);
    assert_eq!(c.name, "lcl");
    assert_eq!(c.members.len(), 2);

    assert_eq!(c.members[0].visibility, Visibility::Public);
    match &c.members[0].kind {
        MemberKind::Method(sig) => {
            assert_eq!(sig.name, "run");
            assert!(!sig.is_class);
            assert_eq!(sig.sections.len(), 2);
            assert_eq!(sig.sections[0].0, "IMPORTING");
            assert_eq!(sig.sections[0].1, "iv_x TYPE i");
            assert_eq!(sig.sections[1].0, "RETURNING");
        }
        other => panic!("expected method, got {other:?}"),
    }

    assert_eq!(c.members[1].visibility, Visibility::Private);
    assert!(matches!(c.members[1].kind, MemberKind::Data));
}

#[test]
fn class_static_method_is_flagged() {
    let prog =
        program("CLASS lcl DEFINITION.\n  PUBLIC SECTION.\n    CLASS-METHODS make.\nENDCLASS.");
    let TopItem::Class(c) = &prog.items[0] else {
        panic!("expected class")
    };
    match &c.members[0].kind {
        MemberKind::Method(sig) => assert!(sig.is_class),
        other => panic!("expected method, got {other:?}"),
    }
}

#[test]
fn interface_method_names_keep_their_qualifier() {
    let prog = program(
        "CLASS child DEFINITION.\n\
           PUBLIC SECTION.\n\
             METHODS zif_contract~create REDEFINITION.\n\
         ENDCLASS.\n\
         CLASS child IMPLEMENTATION.\n\
           METHOD zif_contract~create.\n\
           ENDMETHOD.\n\
         ENDCLASS.",
    );
    assert!(prog.errors.is_empty(), "{:?}", prog.errors);
    let TopItem::Class(definition) = &prog.items[0] else {
        panic!("expected class definition")
    };
    let MemberKind::Method(signature) = &definition.members[0].kind else {
        panic!("expected method declaration")
    };
    assert_eq!(signature.name, "zif_contract~create");
    let TopItem::Class(implementation) = &prog.items[1] else {
        panic!("expected class implementation")
    };
    assert_eq!(implementation.methods[0].name, "zif_contract~create");
}

#[test]
fn class_implementation_collects_methods() {
    let src = "CLASS lcl IMPLEMENTATION.\n  METHOD a.\n    x = 1.\n  ENDMETHOD.\n  METHOD b.\n    IF x = 1.\n    ENDIF.\n  ENDMETHOD.\nENDCLASS.";
    let prog = program(src);
    assert!(prog.errors.is_empty(), "{:?}", prog.errors);
    let TopItem::Class(c) = &prog.items[0] else {
        panic!("expected class")
    };
    assert_eq!(c.kind, ClassKind::Implementation);
    assert_eq!(c.methods.len(), 2);
    assert_eq!(c.methods[0].name, "a");
    assert_eq!(c.methods[1].body.len(), 1);
    assert!(matches!(c.methods[1].body[0], Item::Block { .. }));
}

#[test]
fn deferred_class_is_plain_statement() {
    let prog = program("CLASS lcl DEFINITION DEFERRED.");
    assert!(prog.errors.is_empty());
    assert!(matches!(prog.items[0], TopItem::Statement(_)));
}

#[test]
fn missing_endclass_is_reported() {
    let prog = program("CLASS lcl DEFINITION.\n  PUBLIC SECTION.\n");
    assert_eq!(prog.errors.len(), 1);
}

#[test]
fn method_refs_find_calls() {
    let prog = program("obj->Run( ).\nzcl_x=>make( ).");
    let refs: Vec<Vec<String>> = prog
        .items
        .iter()
        .map(|i| match i {
            TopItem::Statement(s) => s.method_refs(),
            _ => vec![],
        })
        .collect();
    assert_eq!(refs[0], vec!["run"]);
    assert_eq!(refs[1], vec!["make"]);
}

#[test]
fn outline_contains_class_and_method_names() {
    let src = "CLASS lcl IMPLEMENTATION.\n  METHOD go.\n  ENDMETHOD.\nENDCLASS.";
    let out = abap_compiler::inspect::abap::outline(&program(src));
    assert!(out.contains("CLASS IMPLEMENTATION lcl"));
    assert!(out.contains("METHOD go"));
}
