//! Übersetzt den Parser-AST (abapgit-inspect) in die typisierte IR für den Codegen.
//! Unbekannte Statements führen zu einem Übersetzungsfehler.

mod block;
mod decl;
mod ir;
mod stmt;
mod tokens;

pub use ir::*;
use tokens::*;
use tracing::error;

use std::collections::{BTreeSet, HashMap};

use abapgit_inspect::abap::{
    parse_l1, ClassDef as AbapClass, ClassKind, Item, Kind, MemberKind, Program, Statement, Token,
    TopItem,
};

use crate::ast::{
    err, literal_content, parse_call_args, parse_cond, parse_expr, CompileError, Cond, Expr,
    NamedArg,
};
use crate::symbols::SymbolCatalog;

/// Parst und übersetzt ein ABAP-Programm in die IR.
/// Alle Fehler der Übersetzung; leer heißt Erfolg. Fehler je Anweisung werden gesammelt.
pub fn parse_source(src: &str) -> Result<Ir, Vec<CompileError>> {
    let prog = parse_l1(src);
    for e in prog.errors.iter() {
        error!("L1 Error: {} {}", e.line, e.message);
    }
    parse_l2_program(&prog)
}

/// Eine Anweisung im Ablauf: einfaches Statement oder Block mit Körper.
enum Node<'a> {
    S(&'a Statement),
    B(&'a Statement, &'a [Item]),
    C(&'a AbapClass),
}

fn nodes(items: &[Item]) -> Vec<Node<'_>> {
    items
        .iter()
        .map(|i| match i {
            Item::Statement(s) => Node::S(s),
            Item::Block { head, body } => Node::B(head, body),
        })
        .collect()
}

/// Erkennt `TYPES|DATA BEGIN OF name` und liefert den Namen.
fn struct_begin(s: &Statement) -> Option<(String, String)> {
    let kw = s.keyword();
    if (kw == "TYPES" || kw == "DATA")
        && s.tokens.len() >= 4
        && s.tokens[1].is_word("BEGIN")
        && s.tokens[2].is_word("OF")
    {
        Some((kw, word(&s.tokens[3])))
    } else {
        None
    }
}

/// Zustand während der Übersetzung: bekannte Typnamen und Strukturdefinitionen.
struct Lowerer {
    /// Gesammelte Fehler; die Übersetzung läuft nach einem Fehler weiter.
    errors: Vec<CompileError>,
    structs: Vec<StructDef>,
    /// Typname (klein) -> Typ; enthält Strukturen, Aliase und Klassen (als Ref)
    names: HashMap<String, Ty>,
    classes: Vec<ClassDef>,
    /// Methodensignaturen je Klasse (Index in `classes`), aus der Definition
    sigs: Vec<HashMap<String, MethodSig>>,
    interfaces: HashMap<String, HashMap<String, MethodSig>>,
    interface_fields: HashMap<String, HashMap<String, Ty>>,
    constants: HashMap<String, Expr>,
    external_types: Vec<String>,
    external_type_indices: HashMap<String, usize>,
    data_types: HashMap<String, Ty>,
    scope: Option<String>,
}

/// Signatur einer Methode aus der Klassendefinition.
#[derive(Clone)]
struct MethodSig {
    params: Vec<Param>,
    ret: Option<RetParam>,
    is_static: bool,
}

fn parse_l2_program(prog: &Program) -> Result<Ir, Vec<CompileError>> {
    let _symbols = SymbolCatalog::from_program(prog).map_err(|e| vec![e])?;
    let mut lw = Lowerer {
        errors: Vec::new(),
        structs: Vec::new(),
        names: HashMap::new(),
        classes: Vec::new(),
        sigs: Vec::new(),
        interfaces: HashMap::new(),
        interface_fields: HashMap::new(),
        constants: HashMap::from([
            ("abap_true".to_string(), Expr::Lit("X".into())),
            ("abap_false".to_string(), Expr::Lit(String::new())),
        ]),
        external_types: Vec::new(),
        external_type_indices: HashMap::new(),
        data_types: HashMap::new(),
        scope: None,
    };
    let mut stmts = Vec::new();
    let mut form_items: Vec<&TopItem> = Vec::new();

    // Erst die Programmebene (Typen, Daten, START-OF-SELECTION), danach die FORMs.
    let mut top: Vec<Node> = Vec::new();
    for item in &prog.items {
        match item {
            TopItem::Statement(s) => top.push(Node::S(s)),
            TopItem::Block { head, body } => top.push(Node::B(head, body)),
            TopItem::Routine { kind, body, .. }
                if matches!(kind.as_str(), "INITIALIZATION" | "START-OF-SELECTION") =>
            {
                top.extend(nodes(body));
            }
            TopItem::Routine { kind, .. } if kind == "FORM" => form_items.push(item),
            TopItem::Routine { kind, head, .. }
                if kind == "AT" && head.tokens.iter().any(|t| t.is_word("SELECTION-SCREEN")) =>
            {
                // Selection-screen callbacks are SAP GUI kernel events and do not run in
                // a native report executable.
            }
            TopItem::Routine { kind, head, .. } => lw.errors.push(CompileError {
                line: head.line,
                message: format!("Nicht unterstütztes Ereignis/Routine: {kind}"),
            }),
            TopItem::Class(c) => top.push(Node::C(c)),
        }
    }
    if let Err(e) = lw.lower_nodes(&top, &mut stmts) {
        lw.errors.push(e);
    }
    if let Err(e) = lw.check_all_implemented() {
        lw.errors.push(e);
    }

    let mut forms = Vec::new();
    for item in form_items {
        let TopItem::Routine { head, body, .. } = item else {
            continue;
        };
        let params = match lw.parse_params(head.tokens.get(2..).unwrap_or(&[]), head.line) {
            Ok(params) => params,
            Err(e) => {
                lw.errors.push(e);
                continue;
            }
        };
        let mut fbody = Vec::new();
        if let Err(e) = lw.lower_nodes(&nodes(body), &mut fbody) {
            lw.errors.push(e);
        }
        forms.push(Form {
            name: head.tokens.get(1).map(word).unwrap_or_default(),
            params,
            body: fbody,
            line: head.line,
        });
    }

    if !lw.errors.is_empty() {
        return Err(lw.errors);
    }
    Ok(Ir {
        stmts,
        forms,
        structs: lw.structs,
        classes: lw.classes,
        constants: lw.constants,
        dependencies: lw.external_types,
    })
}

impl Lowerer {
    /// Übersetzt eine Folge von Anweisungen. Strukturdefinitionen (BEGIN OF ... END OF) werden
    /// über mehrere Statements hinweg zusammengefasst, Typaliase registriert.
    pub(super) fn lower_nodes(
        &mut self,
        seq: &[Node],
        out: &mut Vec<Stmt>,
    ) -> Result<(), CompileError> {
        let mut i = 0;
        while i < seq.len() {
            if let Node::S(s) = seq[i] {
                if let Some((kw, name)) = struct_begin(s) {
                    let line = s.line;
                    let mut fields = Vec::new();
                    let mut j = i + 1;
                    loop {
                        let Some(node) = seq.get(j) else {
                            return err(line, format!("END OF {name} fehlt"));
                        };
                        match node {
                            Node::S(f)
                                if f.keyword() == kw
                                    && f.tokens.get(1).is_some_and(|t| t.is_word("END")) =>
                            {
                                break
                            }
                            Node::S(f)
                                if f.keyword() == kw
                                    && f.tokens
                                        .get(1)
                                        .is_some_and(|token| token.is_word("BEGIN")) =>
                            {
                                let mut depth = 1usize;
                                j += 1;
                                while j < seq.len() && depth > 0 {
                                    if let Node::S(nested) = &seq[j] {
                                        if nested.keyword() == kw
                                            && nested
                                                .tokens
                                                .get(1)
                                                .is_some_and(|token| token.is_word("BEGIN"))
                                        {
                                            depth += 1;
                                        } else if nested.keyword() == kw
                                            && nested
                                                .tokens
                                                .get(1)
                                                .is_some_and(|token| token.is_word("END"))
                                        {
                                            depth -= 1;
                                        }
                                    }
                                    j += 1;
                                }
                                continue;
                            }
                            Node::S(f)
                                if f.keyword() == kw
                                    && f.tokens
                                        .get(1)
                                        .is_some_and(|token| token.is_word("INCLUDE")) => {}
                            Node::S(f) if f.tokens.iter().any(|token| token.is_word("INCLUDE")) => {
                            }
                            Node::S(f) if f.keyword() == kw => fields.push(self.field(f)?),
                            Node::S(f) => return err(f.line, "Struktur unvollständig"),
                            Node::B(h, _) => {
                                return err(h.line, "Blöcke in Strukturen werden nicht unterstützt")
                            }
                            Node::C(c) => {
                                return err(
                                    c.head.line,
                                    "Klassen in Strukturen werden nicht unterstützt",
                                )
                            }
                        }
                        j += 1;
                    }
                    let idx = self.define_struct(&name, fields, line)?;
                    if kw == "TYPES" {
                        self.names.insert(name.to_lowercase(), Ty::Struct(idx));
                    } else {
                        out.push(Stmt::Data {
                            name: name.to_lowercase(),
                            ty: Ty::Struct(idx),
                            line,
                        });
                    }
                    i = j + 1;
                    continue;
                }
                if s.keyword() == "TYPES" && s.tokens.len() >= 4 && s.tokens[2].is_word("TYPE") {
                    // Alias: TYPES name TYPE <typ>.
                    let ty = self.ty_of(&s.tokens[3..], s.line)?;
                    self.names.insert(word(&s.tokens[1]), ty);
                    i += 1;
                    continue;
                }
            }

            // Ein fehlerhaftes Statement wird gesammelt; die Folge läuft weiter.
            let result = match seq[i] {
                Node::S(s) => self.lower_stmt(s, out),
                Node::B(h, b) => self.lower_block(h, b, out),
                Node::C(c) => match c.kind {
                    ClassKind::Definition => self.define_class(c),
                    ClassKind::Implementation => self.implement_class(c),
                    ClassKind::Interface => self.define_interface(c),
                },
            };
            if let Err(e) = result {
                self.errors.push(e);
            }
            i += 1;
        }
        Ok(())
    }

    fn qualified_type(&self, name: &str) -> String {
        self.scope
            .as_ref()
            .map(|scope| format!("{scope}=>{}", name.to_lowercase()))
            .unwrap_or_else(|| name.to_lowercase())
    }

    fn external_type(&mut self, name: &str) -> Ty {
        Ty::External(self.external_type_index(name))
    }

    fn external_type_index(&mut self, name: &str) -> usize {
        let name = name.to_lowercase();
        *self
            .external_type_indices
            .entry(name.clone())
            .or_insert_with(|| {
                let index = self.external_types.len();
                self.external_types.push(name);
                index
            })
    }
}

#[cfg(test)]
mod tests {
    use super::parse_source;
    use crate::lower::{ClassParent, InterfaceRef};

    #[test]
    fn lowers_initialization_before_start_of_selection() {
        let ir = parse_source(
            "INITIALIZATION.\n\
             DATA value TYPE i.\n\
             value = 1.\n\
             START-OF-SELECTION.\n\
             WRITE value.",
        )
        .unwrap();

        assert_eq!(ir.stmts.len(), 3);
    }

    #[test]
    fn ignores_selection_screen_kernel_events() {
        let ir = parse_source(
            "AT SELECTION-SCREEN OUTPUT.\n\
             WRITE 'screen'.\n\
             START-OF-SELECTION.\n\
             WRITE 'run'.",
        )
        .unwrap();

        assert_eq!(ir.stmts.len(), 1);
    }

    #[test]
    fn inherits_internal_parent_layout_and_metadata() {
        let ir = parse_source(
            "CLASS base DEFINITION.\n\
               PUBLIC SECTION.\n\
                 DATA id TYPE i.\n\
             ENDCLASS.\n\
             CLASS base IMPLEMENTATION.\n\
             ENDCLASS.\n\
             CLASS child DEFINITION INHERITING FROM base.\n\
               PUBLIC SECTION.\n\
                 DATA name TYPE string.\n\
             ENDCLASS.\n\
             CLASS child IMPLEMENTATION.\n\
             ENDCLASS.",
        )
        .unwrap();

        assert_eq!(ir.classes[1].parent, Some(ClassParent::Internal(0)));
        assert_eq!(ir.structs[ir.classes[1].struct_idx].fields.len(), 2);
        assert_eq!(ir.structs[ir.classes[1].struct_idx].fields[0].0, "id");
    }

    #[test]
    fn records_external_parent_as_kernel_symbol() {
        let ir = parse_source(
            "CLASS child DEFINITION INHERITING FROM cx_static_check.\n\
             ENDCLASS.\n\
             CLASS child IMPLEMENTATION.\n\
             ENDCLASS.",
        )
        .unwrap();

        assert_eq!(
            ir.classes[0].parent,
            Some(ClassParent::External("cx_static_check".into()))
        );
    }

    #[test]
    fn registers_internal_and_external_interfaces() {
        let ir = parse_source(
            "INTERFACE if_contract.\n\
               METHODS load IMPORTING value TYPE i.\n\
             ENDINTERFACE.\n\
             CLASS consumer DEFINITION.\n\
               PUBLIC SECTION.\n\
                 INTERFACES if_contract.\n\
                 INTERFACES if_kernel.\n\
             ENDCLASS.\n\
             CLASS consumer IMPLEMENTATION.\n\
               METHOD if_contract~load.\n\
                 WRITE value.\n\
               ENDMETHOD.\n\
             ENDCLASS.",
        )
        .unwrap();

        assert_eq!(
            ir.classes[0].interfaces,
            vec![
                InterfaceRef::Internal("if_contract".into()),
                InterfaceRef::External("if_kernel".into()),
            ]
        );
        assert_eq!(ir.classes[0].methods[0].name, "if_contract~load");
    }

    #[test]
    fn resolves_class_and_interface_types() {
        let ir = parse_source(
            "INTERFACE if_types.\n\
               TYPES ty_id TYPE i.\n\
               TYPES: BEGIN OF ty_pair,\n\
                 left TYPE i,\n\
                 right TYPE i,\n\
               END OF ty_pair.\n\
             ENDINTERFACE.\n\
             CLASS consumer DEFINITION.\n\
               PUBLIC SECTION.\n\
                 TYPES ty_alias TYPE if_types=>ty_id.\n\
                 DATA number TYPE if_types=>ty_id.\n\
                 METHODS get IMPORTING value TYPE ty_alias.\n\
             ENDCLASS.\n\
             CLASS consumer IMPLEMENTATION.\n\
               METHOD get.\n\
                 number = value.\n\
               ENDMETHOD.\n\
             ENDCLASS.",
        )
        .unwrap();

        let class_struct = ir.classes[0].struct_idx;
        assert_eq!(ir.structs[class_struct].fields.len(), 1);
        assert_eq!(ir.structs[class_struct].fields[0].0, "number");
        assert!(ir
            .structs
            .iter()
            .any(|structure| structure.name == "if_types=>ty_pair"));
        assert_eq!(ir.classes[0].methods[0].params[0].name, "value");
    }

    #[test]
    fn resolves_like_class_attribute_in_local_structure() {
        let ir = parse_source(
            "CLASS holder DEFINITION.\n\
               PUBLIC SECTION.\n\
                 DATA text TYPE string.\n\
               PRIVATE SECTION.\n\
                 TYPES: BEGIN OF state,\n\
                   copy LIKE text,\n\
                 END OF state.\n\
             ENDCLASS.\n\
             CLASS holder IMPLEMENTATION.\n\
             ENDCLASS.",
        )
        .unwrap();

        assert!(ir.structs.iter().any(|structure| {
            structure.name == "holder=>state"
                && structure
                    .fields
                    .iter()
                    .any(|(name, ty)| name == "copy" && matches!(ty, crate::lower::Ty::Str))
        }));
    }

    #[test]
    fn registers_scalar_and_structured_constants() {
        let ir = parse_source(
            "CLASS settings DEFINITION.\n\
               PUBLIC SECTION.\n\
                 CONSTANTS limit TYPE i VALUE 4.\n\
                 CONSTANTS: BEGIN OF message,\n\
                   id TYPE string VALUE '42',\n\
                   text TYPE string VALUE 'answer',\n\
                 END OF message.\n\
             ENDCLASS.\n\
             CLASS settings IMPLEMENTATION.\n\
             ENDCLASS.\n\
             WRITE settings=>limit.\n\
             WRITE settings=>message-text.",
        )
        .unwrap();

        assert_eq!(ir.constants["settings=>limit"], crate::ast::Expr::Int(4));
        assert_eq!(
            ir.constants["settings=>message-text"],
            crate::ast::Expr::Lit("answer".into())
        );
    }

    #[test]
    fn ignores_escaped_parameter_names() {
        let ir = parse_source(
            "CLASS worker DEFINITION.\n\
               PUBLIC SECTION.\n\
                 METHODS run IMPORTING !VALUE TYPE i.\n\
             ENDCLASS.\n\
             CLASS worker IMPLEMENTATION.\n\
               METHOD run.\n\
                 WRITE value.\n\
               ENDMETHOD.\n\
             ENDCLASS.",
        )
        .unwrap();

        assert_eq!(ir.classes[0].methods[0].params[0].name, "value");
    }

    #[test]
    fn parses_is_initial_and_is_not_initial() {
        parse_source(
            "DATA value TYPE i.\n\
             IF value IS INITIAL.\n\
               value = 1.\n\
             ENDIF.\n\
             IF value IS NOT INITIAL.\n\
               value = 2.\n\
             ENDIF.",
        )
        .unwrap();
    }

    #[test]
    fn accepts_like_parameter_as_external_type() {
        let ir = parse_source(
            "CLASS worker DEFINITION.\n\
               PUBLIC SECTION.\n\
                 METHODS run IMPORTING !TEXTID LIKE if_t100_message=>t100key OPTIONAL.\n\
             ENDCLASS.\n\
             CLASS worker IMPLEMENTATION.\n\
               METHOD run.\n\
               ENDMETHOD.\n\
             ENDCLASS.",
        )
        .unwrap();

        assert!(matches!(
            ir.classes[0].methods[0].params[0].ty,
            crate::lower::Ty::External(_)
        ));
        assert_eq!(
            ir.dependencies,
            vec!["if_t100_message=>t100key".to_string()]
        );
    }

    #[test]
    fn accepts_like_inherited_attribute_as_external_type() {
        let ir = parse_source(
            "CLASS child DEFINITION INHERITING FROM cx_root.\n\
               PUBLIC SECTION.\n\
                 METHODS constructor IMPORTING previous LIKE previous OPTIONAL.\n\
             ENDCLASS.\n\
             CLASS child IMPLEMENTATION.\n\
               METHOD constructor.\n\
               ENDMETHOD.\n\
             ENDCLASS.",
        )
        .unwrap();

        assert_eq!(ir.dependencies, vec!["child=>previous".to_string()]);
    }

    #[test]
    fn lowers_static_class_data_and_method() {
        let ir = parse_source(
            "CLASS counter DEFINITION.\n\
               PUBLIC SECTION.\n\
                 CLASS-DATA value TYPE i.\n\
                 CLASS-METHODS add IMPORTING amount TYPE i.\n\
             ENDCLASS.\n\
             CLASS counter IMPLEMENTATION.\n\
               METHOD add.\n\
                 counter=>value = counter=>value + amount.\n\
               ENDMETHOD.\n\
             ENDCLASS.",
        )
        .unwrap();

        assert_eq!(ir.classes[0].static_fields[0].0, "value");
        assert!(ir.classes[0].methods[0].is_static);
    }

    #[test]
    fn includes_interface_attributes_in_class_layout() {
        let ir = parse_source(
            "INTERFACE if_contract.\n\
               DATA value TYPE i.\n\
             ENDINTERFACE.\n\
             CLASS worker DEFINITION.\n\
               PUBLIC SECTION.\n\
                 INTERFACES if_contract.\n\
                 METHODS set_value.\n\
             ENDCLASS.\n\
             CLASS worker IMPLEMENTATION.\n\
               METHOD set_value.\n\
                 if_contract~value = 3.\n\
               ENDMETHOD.\n\
             ENDCLASS.",
        )
        .unwrap();

        assert!(ir.structs[ir.classes[0].struct_idx]
            .fields
            .iter()
            .any(|(name, _)| name == "if_contract~value"));
    }

    #[test]
    fn lowers_constructor_arguments() {
        let ir = parse_source(
            "CLASS item DEFINITION.\n\
               PUBLIC SECTION.\n\
                 METHODS constructor IMPORTING value TYPE i.\n\
             ENDCLASS.\n\
             CLASS item IMPLEMENTATION.\n\
               METHOD constructor.\n\
               ENDMETHOD.\n\
             ENDCLASS.\n\
             DATA object TYPE REF TO item.\n\
             CREATE OBJECT object EXPORTING value = 4.",
        )
        .unwrap();

        assert!(ir.stmts.iter().any(|statement| matches!(
            statement,
            crate::lower::Stmt::Create { name, args, .. }
                if name == "object" && args.len() == 1
        )));
    }

    #[test]
    fn lowers_try_catch_raise() {
        let ir = parse_source(
            "CLASS failure DEFINITION INHERITING FROM cx_static_check.\n\
             ENDCLASS.\n\
             CLASS failure IMPLEMENTATION.\n\
             ENDCLASS.\n\
             DATA exception TYPE REF TO failure.\n\
             TRY.\n\
               CREATE OBJECT exception.\n\
               RAISE EXCEPTION exception.\n\
             CATCH cx_static_check INTO exception.\n\
               WRITE 'caught'.\n\
             CLEANUP.\n\
               WRITE 'cleanup'.\n\
             ENDTRY.",
        )
        .unwrap();

        assert!(ir.stmts.iter().any(|statement| matches!(
            statement,
            crate::lower::Stmt::Try { catches, cleanup, .. }
                if catches.len() == 1 && cleanup.len() == 1
        )));
    }

    #[test]
    fn lowers_assign_component_of_known_structure() {
        let ir = parse_source(
            "TYPES: BEGIN OF state,\n\
               value TYPE i,\n\
             END OF state.\n\
             DATA record TYPE state.\n\
             FIELD-SYMBOLS <value> TYPE i.\n\
             ASSIGN COMPONENT 'VALUE' OF STRUCTURE record TO <value>.",
        )
        .unwrap();

        assert!(ir.stmts.iter().any(|statement| matches!(
            statement,
            crate::lower::Stmt::AssignComponent { component, structure, .. }
                if component == "value" && structure == "record"
        )));
    }

    #[test]
    fn parses_is_assigned() {
        parse_source(
            "TYPES: BEGIN OF state, value TYPE i, END OF state.\n\
             DATA record TYPE state.\n\
             FIELD-SYMBOLS <value> TYPE i.\n\
             ASSIGN COMPONENT 'VALUE' OF STRUCTURE record TO <value>.\n\
             IF <value> IS ASSIGNED.\n\
               <value> = 1.\n\
             ENDIF.",
        )
        .unwrap();
    }

    #[test]
    fn parses_string_templates() {
        let ir = parse_source(
            "DATA count TYPE i.\n\
             count = 42.\n\
             DATA text TYPE string.\n\
             text = |There are { count } items.|.",
        )
        .unwrap();

        assert!(ir.stmts.iter().any(|statement| matches!(
            statement,
            crate::lower::Stmt::Assign {
                value: crate::ast::Expr::Template(parts),
                ..
            } if parts.len() == 3
        )));
    }

    #[test]
    fn resolves_like_qualified_class_attribute_and_external_ddic() {
        let ir = parse_source(
            "CLASS source DEFINITION.\n\
               PUBLIC SECTION.\n\
                 DATA program TYPE progname.\n\
             ENDCLASS.\n\
             CLASS source IMPLEMENTATION.\n\
             ENDCLASS.\n\
             CLASS consumer DEFINITION.\n\
               PUBLIC SECTION.\n\
                 DATA copy LIKE source=>program.\n\
             ENDCLASS.\n\
             CLASS consumer IMPLEMENTATION.\n\
             ENDCLASS.",
        )
        .unwrap();

        assert!(ir.dependencies.contains(&"source=>progname".to_string()));
        assert!(
            ir.dependencies.contains(&"source=>program".to_string())
                || ir
                    .structs
                    .iter()
                    .any(|structure| structure.name == "class consumer"
                        && structure.fields.iter().any(|(name, _)| name == "copy"))
        );
    }

    #[test]
    fn records_external_interface_reference() {
        let ir = parse_source("DATA log TYPE REF TO zif_external_log.").unwrap();

        assert_eq!(ir.dependencies, vec!["ref=>zif_external_log"]);
    }

    #[test]
    fn allows_structured_class_attributes() {
        let ir = parse_source(
            "TYPES: BEGIN OF state, value TYPE i, END OF state.\n\
             CLASS holder DEFINITION.\n\
               PUBLIC SECTION.\n\
                 DATA current TYPE state.\n\
             ENDCLASS.\n\
             CLASS holder IMPLEMENTATION.\n\
             ENDCLASS.",
        )
        .unwrap();

        assert!(matches!(
            ir.structs[ir.classes[0].struct_idx].fields[0].1,
            crate::lower::Ty::Struct(_)
        ));
    }

    #[test]
    fn ignores_redefinition_modifier_in_method_signature() {
        let ir = parse_source(
            "CLASS base DEFINITION.\n\
               PUBLIC SECTION.\n\
                 METHODS run.\n\
             ENDCLASS.\n\
             CLASS base IMPLEMENTATION.\n\
               METHOD run.\n\
               ENDMETHOD.\n\
             ENDCLASS.\n\
             CLASS child DEFINITION INHERITING FROM base.\n\
               PUBLIC SECTION.\n\
                 METHODS run REDEFINITION.\n\
             ENDCLASS.\n\
             CLASS child IMPLEMENTATION.\n\
               METHOD run.\n\
               ENDMETHOD.\n\
             ENDCLASS.",
        )
        .unwrap();

        assert_eq!(ir.classes[1].methods[0].name, "run");
    }

    #[test]
    fn lowers_unqualified_instance_method_call() {
        let ir = parse_source(
            "CLASS worker DEFINITION.\n\
               PUBLIC SECTION.\n\
                 METHODS run.\n\
                 METHODS helper.\n\
             ENDCLASS.\n\
             CLASS worker IMPLEMENTATION.\n\
               METHOD run.\n\
                 helper( ).\n\
               ENDMETHOD.\n\
               METHOD helper.\n\
               ENDMETHOD.\n\
             ENDCLASS.",
        )
        .unwrap();

        assert!(matches!(
            ir.classes[0].methods[0].body[0],
            crate::lower::Stmt::MethodCall { ref recv, ref method, .. }
                if recv == "me" && method == "helper"
        ));
    }

    #[test]
    fn lowers_field_symbol_like_line_of_table() {
        let ir = parse_source(
            "TYPES: BEGIN OF row, value TYPE i, END OF row.\n\
             DATA rows TYPE TABLE OF row.\n\
             FIELD-SYMBOLS <row> LIKE LINE OF rows.",
        )
        .unwrap();

        assert!(ir.stmts.iter().any(|statement| matches!(
            statement,
            crate::lower::Stmt::FieldSymbol {
                ty: crate::lower::Ty::Struct(_),
                ..
            }
        )));
    }

    #[test]
    fn lowers_read_table_assigning_index() {
        let ir = parse_source(
            "TYPES: BEGIN OF row, value TYPE i, END OF row.\n\
             DATA rows TYPE TABLE OF row.\n\
             FIELD-SYMBOLS <row> LIKE LINE OF rows.\n\
             READ TABLE rows ASSIGNING <row> INDEX 1.",
        )
        .unwrap();

        assert!(matches!(
            ir.stmts.last(),
            Some(crate::lower::Stmt::ReadTable { .. })
        ));
    }

    #[test]
    fn lowers_external_function_call_as_kernel_dependency() {
        let ir = parse_source("CALL FUNCTION 'DOCU_GET' EXPORTING id = 'NA'.").unwrap();
        assert!(matches!(
            ir.stmts[0],
            crate::lower::Stmt::ExternalFunctionCall { ref name, .. } if name == "docu_get"
        ));
    }
}
