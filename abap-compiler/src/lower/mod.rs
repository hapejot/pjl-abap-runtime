//! Übersetzt den Parser-AST (abapgit-inspect) in die typisierte IR für den Codegen.
//! Unbekannte Statements führen zu einem Übersetzungsfehler.

mod block;
mod decl;
mod ir;
mod stmt;
mod tokens;

pub use ir::*;
use tokens::*;

use std::collections::HashMap;

use abapgit_inspect::abap::{
    parse, ClassDef as AbapClass, ClassKind, Item, Kind, MemberKind, Program, Statement, Token, TopItem,
};

use crate::ast::{err, literal_content, parse_call_args, parse_cond, parse_expr, CompileError, Cond, Expr, NamedArg};

/// Parst und übersetzt ein ABAP-Programm in die IR.
pub fn lower_source(src: &str) -> Result<Ir, CompileError> {
    let prog = parse(src);
    if let Some(e) = prog.errors.first() {
        return err(e.line, format!("Syntaxfehler: {}", e.message));
    }
    lower_program(&prog)
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
    structs: Vec<StructDef>,
    /// Typname (klein) -> Typ; enthält Strukturen, Aliase und Klassen (als Ref)
    names: HashMap<String, Ty>,
    classes: Vec<ClassDef>,
    /// Methodensignaturen je Klasse (Index in `classes`), aus der Definition
    sigs: Vec<HashMap<String, MethodSig>>,
}

/// Signatur einer Methode aus der Klassendefinition.
#[derive(Clone)]
struct MethodSig {
    params: Vec<Param>,
    ret: Option<RetParam>,
}

fn lower_program(prog: &Program) -> Result<Ir, CompileError> {
    let mut lw = Lowerer { structs: Vec::new(), names: HashMap::new(), classes: Vec::new(), sigs: Vec::new() };
    let mut stmts = Vec::new();
    let mut form_items: Vec<&TopItem> = Vec::new();

    // Erst die Programmebene (Typen, Daten, START-OF-SELECTION), danach die FORMs.
    let mut top: Vec<Node> = Vec::new();
    for item in &prog.items {
        match item {
            TopItem::Statement(s) => top.push(Node::S(s)),
            TopItem::Block { head, body } => top.push(Node::B(head, body)),
            TopItem::Routine { kind, body, .. } if kind == "START-OF-SELECTION" => {
                top.extend(nodes(body));
            }
            TopItem::Routine { kind, .. } if kind == "FORM" => form_items.push(item),
            TopItem::Routine { kind, head, .. } => {
                return err(head.line, format!("Nicht unterstütztes Ereignis/Routine: {kind}"))
            }
            TopItem::Class(c) => top.push(Node::C(c)),
        }
    }
    lw.lower_nodes(&top, &mut stmts)?;
    lw.check_all_implemented()?;

    let mut forms = Vec::new();
    for item in form_items {
        let TopItem::Routine { head, body, .. } = item else { continue };
        let params = lw.parse_params(head.tokens.get(2..).unwrap_or(&[]), head.line)?;
        let mut fbody = Vec::new();
        lw.lower_nodes(&nodes(body), &mut fbody)?;
        forms.push(Form {
            name: head.tokens.get(1).map(word).unwrap_or_default(),
            params,
            body: fbody,
            line: head.line,
        });
    }

    Ok(Ir { stmts, forms, structs: lw.structs, classes: lw.classes })
}

impl Lowerer {
    /// Übersetzt eine Folge von Anweisungen. Strukturdefinitionen (BEGIN OF ... END OF) werden
    /// über mehrere Statements hinweg zusammengefasst, Typaliase registriert.
    pub(super) fn lower_nodes(&mut self, seq: &[Node], out: &mut Vec<Stmt>) -> Result<(), CompileError> {
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
                            Node::S(f) if f.keyword() == kw && f.tokens.get(1).is_some_and(|t| t.is_word("END")) => break,
                            Node::S(f) if f.keyword() == kw => fields.push(self.field(f)?),
                            Node::S(f) => return err(f.line, "Struktur unvollständig"),
                            Node::B(h, _) => return err(h.line, "Blöcke in Strukturen werden nicht unterstützt"),
                            Node::C(c) => return err(c.head.line, "Klassen in Strukturen werden nicht unterstützt"),
                        }
                        j += 1;
                    }
                    let idx = self.define_struct(&name, fields, line)?;
                    if kw == "TYPES" {
                        self.names.insert(name.to_lowercase(), Ty::Struct(idx));
                    } else {
                        out.push(Stmt::Data { name: name.to_lowercase(), ty: Ty::Struct(idx), line });
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
            match seq[i] {
                Node::S(s) => self.lower_stmt(s, out)?,
                Node::B(h, b) => self.lower_block(h, b, out)?,
                Node::C(c) => match c.kind {
                    ClassKind::Definition => self.define_class(c)?,
                    ClassKind::Implementation => self.implement_class(c)?,
                    ClassKind::Interface => return err(c.head.line, "Interfaces werden noch nicht unterstützt"),
                },
            }
            i += 1;
        }
        Ok(())
    }
}
