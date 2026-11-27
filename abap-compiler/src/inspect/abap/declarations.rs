//! Sammelt die Top-Level-Deklarationen eines geparsten ABAP-Programms.

use super::parser::{ClassDef, ClassKind, Program, Statement, TopItem};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declaration {
    /// Art der Deklaration, z.B. DATA, TYPES, CLASS, FORM, FUNCTION, METHODS
    pub kind: String,
    pub name: String,
    pub line: usize,
    /// Untergeordnete Deklarationen (z.B. Methoden und Attribute einer Klasse)
    pub children: Vec<Declaration>,
    /// Quelldatei (Dateiname), in der die Deklaration steht; wird beim Einlesen gesetzt
    pub file: String,
    /// Vollständiger Text des Deklarations-Statements, z.B. Methodensignatur
    pub detail: String,
    /// Zeile der Implementierung (`METHOD ...`), nur bei Methoden mit Implementierung
    pub impl_line: Option<usize>,
}

const DECL_KEYWORDS: &[&str] = &[
    "DATA",
    "CLASS-DATA",
    "CONSTANTS",
    "TYPES",
    "FIELD-SYMBOLS",
    "TABLES",
    "RANGES",
    "PARAMETERS",
    "SELECT-OPTIONS",
    "STATICS",
    "METHODS",
    "CLASS-METHODS",
    "EVENTS",
    "CLASS-EVENTS",
    "INTERFACES",
    "ALIASES",
    "INCLUDE",
];

fn name_at(s: &Statement, idx: usize) -> String {
    s.tokens
        .get(idx)
        .map(|t| t.text.clone())
        .unwrap_or_default()
}

/// Erzeugt aus einem Statement eine Deklaration. `depth` zählt BEGIN OF / END OF,
/// Komponenten innerhalb einer Struktur werden nicht ausgegeben.
fn decl_of(s: &Statement, depth: &mut usize) -> Option<Declaration> {
    let kw = s.keyword();
    if !DECL_KEYWORDS.contains(&kw.as_str()) {
        return None;
    }
    let t1 = name_at(s, 1).to_uppercase();
    if t1 == "END" {
        *depth = depth.saturating_sub(1);
        return None;
    }
    if t1 == "TYPE" || t1 == "LIKE" {
        return None; // INCLUDE TYPE innerhalb von Strukturen
    }
    let top = *depth == 0;
    let is_begin = t1 == "BEGIN";
    let name = if is_begin {
        name_at(s, 3)
    } else {
        name_at(s, 1)
    }
    .to_lowercase();
    if is_begin {
        *depth += 1;
    }
    if !top || name.is_empty() {
        return None;
    }
    Some(Declaration {
        kind: kw,
        name,
        line: s.line,
        children: Vec::new(),
        file: String::new(),
        detail: s.text(),
        impl_line: None,
    })
}

/// Methoden-Implementierungen je Klasse: (Klasse, Methode) -> Zeile des METHOD-Statements.
type ImplMap = std::collections::BTreeMap<(String, String), usize>;

fn class_decl(c: &ClassDef, impls: &ImplMap) -> Option<Declaration> {
    let kind = match c.kind {
        ClassKind::Definition => "CLASS",
        ClassKind::Interface => "INTERFACE",
        // Implementierungen enthalten keine neuen Deklarationen, nur Definitionen zählen.
        ClassKind::Implementation => return None,
    };
    let mut depth = 0usize;
    let class = c.name.to_lowercase();
    let mut children: Vec<Declaration> = c
        .members
        .iter()
        .filter_map(|m| decl_of(&m.statement, &mut depth))
        .collect();
    for child in children
        .iter_mut()
        .filter(|d| d.kind == "METHODS" || d.kind == "CLASS-METHODS")
    {
        child.impl_line = impls
            .get(&(class.clone(), child.name.to_lowercase()))
            .copied();
    }
    Some(Declaration {
        kind: kind.to_string(),
        name: class,
        line: c.head.line,
        children,
        file: String::new(),
        detail: c.head.text(),
        impl_line: None,
    })
}

/// Liefert alle Top-Level-Deklarationen eines Programms in Quelltextreihenfolge.
pub fn declarations(prog: &Program) -> Vec<Declaration> {
    let mut impls = ImplMap::new();
    for item in &prog.items {
        if let TopItem::Class(c) = item {
            if c.kind == ClassKind::Implementation {
                for m in &c.methods {
                    impls.insert((c.name.to_lowercase(), m.name.to_lowercase()), m.head.line);
                }
            }
        }
    }

    let mut out = Vec::new();
    let mut depth = 0usize;
    for item in &prog.items {
        match item {
            TopItem::Statement(s) => out.extend(decl_of(s, &mut depth)),
            TopItem::Block { .. } => {}
            TopItem::Class(c) => out.extend(class_decl(c, &impls)),
            TopItem::Routine {
                kind, name, head, ..
            } => {
                let kind = if matches!(kind.as_str(), "FORM" | "FUNCTION" | "MODULE") {
                    kind.clone()
                } else {
                    "EVENT".to_string()
                };
                out.push(Declaration {
                    kind,
                    name: name.to_lowercase(),
                    line: head.line,
                    children: Vec::new(),
                    file: String::new(),
                    detail: head.text(),
                    impl_line: None,
                });
            }
        }
    }
    out
}
