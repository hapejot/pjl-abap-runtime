//! Abhängigkeitsgraph auf Member-Ebene.
//!
//! Knoten sind Objekte, Klassen, Interfaces, Methoden, Formen, Funktionsbausteine,
//! Module und Ereignisse. Kanten sind Methodenaufrufe, Vererbung, Schnittstellen,
//! Konstruktion, Exceptions, Typverwendungen, Includes, Submits, PERFORMs und
//! Funktionsbausteinaufrufe.
//!
//! Ziele werden aufgelöst, soweit sie im Export liegen. Methodenaufrufe folgen dabei
//! der Vererbung. Liegt das Ziel nicht im Export (z.B. Standardklassen oder DDIC-Objekte),
//! bleibt `to` leer und `hint` nennt den Text aus dem Quelltext.
//!
//! Nicht aufgelöst werden Variablentypen: `lo->m( )` zielt nur dann auf eine Klasse,
//! wenn `lo` den Namen `me` hat. Dynamische Aufrufe bleiben ebenfalls unaufgelöst.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use super::lexer::{Kind, Token};
use super::parser::{
    parse_l1, ClassDef, ClassKind, Item, MemberKind, Program, Statement, TopItem,
};
use crate::inspect::model::is_builtin_type;
use crate::inspect::objects::object_key;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Object,
    Class,
    Interface,
    Method,
    Form,
    Function,
    Module,
    Event,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    Inheritance,
    Interface,
    MethodCall,
    StaticMethodCall,
    Perform,
    FunctionCall,
    Construction,
    Exception,
    TypeReference,
    Include,
    Submit,
}

#[derive(Debug, Clone, Serialize)]
pub struct Node {
    /// Kennung: `zcl_a`, `zcl_a=>run`, `zprog::form:calc`, `zprog::event:start-of-selection`, `fm:z_fm`
    pub id: String,
    pub kind: NodeKind,
    pub object_type: String,
    pub object: String,
    pub name: String,
    pub file: String,
    pub line: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Edge {
    /// Kennung des Quellknotens
    pub from: String,
    /// Kennung des Zielknotens; `None`, wenn nicht im Export oder nicht eindeutig
    pub to: Option<String>,
    /// Ziel wie im Quelltext, z.B. `lo->save`, `zcl_b=>create`, `ztab`
    pub hint: String,
    pub kind: EdgeKind,
    pub file: String,
    pub line: usize,
}

#[derive(Debug, Default, Serialize)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

impl Graph {
    /// Alle Knoten, die von `start` aus über aufgelöste Kanten erreichbar sind (inkl. `start`).
    pub fn reachable_from(&self, start: &str) -> BTreeSet<String> {
        let mut adj: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for edge in &self.edges {
            if let Some(to) = &edge.to {
                adj.entry(edge.from.as_str()).or_default().push(to.as_str());
            }
        }
        let mut seen = BTreeSet::new();
        let mut stack = vec![start];
        while let Some(id) = stack.pop() {
            if !seen.insert(id.to_string()) {
                continue;
            }
            if let Some(next) = adj.get(id) {
                stack.extend(next.iter().copied());
            }
        }
        seen
    }

    /// Objektebene: (Quellobjekt, Zielobjekt) für alle aufgelösten Kanten zwischen verschiedenen Objekten.
    pub fn object_dependencies(&self) -> BTreeSet<(String, String)> {
        let object_of: BTreeMap<&str, &str> = self
            .nodes
            .iter()
            .map(|n| (n.id.as_str(), n.object.as_str()))
            .collect();
        self.edges
            .iter()
            .filter_map(|edge| {
                let to = edge.to.as_deref()?;
                let from_obj = *object_of.get(edge.from.as_str())?;
                let to_obj = *object_of.get(to)?;
                (from_obj != to_obj).then(|| (from_obj.to_string(), to_obj.to_string()))
            })
            .collect()
    }
}

/// Auflösungsregel für eine Kante.
enum Lookup {
    /// Ziel ist eine bekannte Kennung; ohne Treffer bleibt die Kante unaufgelöst.
    Exact(String),
    /// Wie `Exact`, die Kante wird aber verworfen, wenn das Ziel nicht im Export liegt.
    Known(String),
    /// Methode `name` der Klasse `class`, inkl. geerbter Methoden.
    Method { class: String, name: String },
    /// Nicht auflösbar (dynamisch oder Typ unbekannt).
    None,
}

struct Pending {
    edge: Edge,
    lookup: Lookup,
}

pub(crate) struct Owner<'a> {
    pub(crate) obj_type: &'a str,
    pub(crate) obj: &'a str,
}

/// Ein Bereich mit eigenem Quellknoten (Methode, Form, Klassenkopf, Objekt-Ebene ...).
pub(crate) struct Scope<'a> {
    pub(crate) node: Node,
    /// Zusätzlich deklarierte Knoten (z.B. Methoden aus der Klassendefinition)
    pub(crate) extra: Vec<Node>,
    /// Klasse, in der die Anweisungen stehen; wird für `me->` benötigt
    pub(crate) class: Option<String>,
    pub(crate) file: String,
    pub(crate) stmts: Vec<&'a Statement>,
}

struct Ctx<'a> {
    from: &'a str,
    class: Option<&'a str>,
    obj: &'a str,
    file: &'a str,
    local_types: &'a BTreeSet<String>,
}

fn make_node(
    owner: &Owner<'_>,
    kind: NodeKind,
    id: String,
    name: &str,
    file: &str,
    line: usize,
) -> Node {
    Node {
        id,
        kind,
        object_type: owner.obj_type.to_string(),
        object: owner.obj.to_string(),
        name: name.to_string(),
        file: file.to_string(),
        line,
    }
}

fn flatten<'a>(items: &'a [Item], out: &mut Vec<&'a Statement>) {
    for item in items {
        match item {
            Item::Statement(s) => out.push(s),
            Item::Block { head, body } => {
                out.push(head);
                flatten(body, out);
            }
        }
    }
}

fn class_scopes<'a>(c: &'a ClassDef, file: &str, owner: &Owner<'_>, out: &mut Vec<Scope<'a>>) {
    let cls = c.name.to_lowercase();
    match c.kind {
        ClassKind::Definition | ClassKind::Interface => {
            let kind = if c.kind == ClassKind::Interface {
                NodeKind::Interface
            } else {
                NodeKind::Class
            };
            let mut stmts: Vec<&Statement> = vec![&c.head];
            let mut extra = Vec::new();
            for member in &c.members {
                stmts.push(&member.statement);
                if let MemberKind::Method(sig) = &member.kind {
                    let id = format!("{cls}=>{}", sig.name.to_lowercase());
                    extra.push(make_node(
                        owner,
                        NodeKind::Method,
                        id,
                        &sig.name,
                        file,
                        member.statement.line,
                    ));
                }
            }
            out.push(Scope {
                node: make_node(owner, kind, cls.clone(), &c.name, file, c.head.line),
                extra,
                class: None,
                file: file.to_string(),
                stmts,
            });
        }
        ClassKind::Implementation => {
            let mut other = Vec::new();
            flatten(&c.other, &mut other);
            out.push(Scope {
                node: make_node(owner, NodeKind::Class, cls.clone(), &c.name, file, c.head.line),
                extra: Vec::new(),
                class: None,
                file: file.to_string(),
                stmts: other,
            });
            for method in &c.methods {
                let mut stmts = Vec::new();
                flatten(&method.body, &mut stmts);
                let id = format!("{cls}=>{}", method.name.to_lowercase());
                out.push(Scope {
                    node: make_node(
                        owner,
                        NodeKind::Method,
                        id,
                        &method.name,
                        file,
                        method.head.line,
                    ),
                    extra: Vec::new(),
                    class: Some(cls.clone()),
                    file: file.to_string(),
                    stmts,
                });
            }
        }
    }
}

pub(crate) fn scopes_of<'a>(prog: &'a Program, file: &str, owner: &Owner<'_>) -> Vec<Scope<'a>> {
    let mut out = Vec::new();
    let mut top: Vec<&'a Statement> = Vec::new();
    for item in &prog.items {
        match item {
            TopItem::Statement(s) => top.push(s),
            TopItem::Block { head, body } => {
                top.push(head);
                flatten(body, &mut top);
            }
            TopItem::Class(c) => class_scopes(c, file, owner, &mut out),
            TopItem::Routine {
                kind,
                name,
                head,
                body,
            } => {
                let lname = name.to_lowercase();
                let (node_kind, id) = match kind.as_str() {
                    "FORM" => (NodeKind::Form, format!("{}::form:{lname}", owner.obj)),
                    "FUNCTION" => (NodeKind::Function, format!("fm:{lname}")),
                    "MODULE" => (NodeKind::Module, format!("{}::module:{lname}", owner.obj)),
                    _ => {
                        let label = if name == kind {
                            kind.clone()
                        } else {
                            format!("{kind} {name}")
                        };
                        (
                            NodeKind::Event,
                            format!("{}::event:{}", owner.obj, label.to_lowercase()),
                        )
                    }
                };
                let mut stmts: Vec<&Statement> = vec![head];
                flatten(body, &mut stmts);
                out.push(Scope {
                    node: make_node(owner, node_kind, id, name, file, head.line),
                    extra: Vec::new(),
                    class: None,
                    file: file.to_string(),
                    stmts,
                });
            }
        }
    }
    if !top.is_empty() {
        out.push(Scope {
            node: make_node(owner, NodeKind::Object, owner.obj.to_string(), owner.obj, file, 1),
            extra: Vec::new(),
            class: None,
            file: file.to_string(),
            stmts: top,
        });
    }
    out
}

/// Fügt einen Knoten ein. Ein bestehender Knoten bleibt erhalten, außer `replace` ist gesetzt
/// (Methodenimplementierung ersetzt die Deklaration) oder er ist nur ein Objekt-Platzhalter.
fn put(nodes: &mut BTreeMap<String, Node>, node: Node, replace: bool) {
    let keep = nodes
        .get(&node.id)
        .is_some_and(|old| !replace && old.kind != NodeKind::Object);
    if !keep {
        nodes.insert(node.id.clone(), node);
    }
}

/// Namen an Position `idx`, kleingeschrieben. Literale wie `'Z_FM'` werden ohne Anführungszeichen geliefert.
pub(crate) fn name_at(toks: &[Token], idx: usize) -> Option<String> {
    let t = toks
        .get(idx)
        .filter(|t| matches!(t.kind, Kind::Word | Kind::Literal))?;
    Some(match t.kind {
        Kind::Literal if t.text.len() >= 2 => t.text[1..t.text.len() - 1].to_lowercase(),
        _ => t.text.to_lowercase(),
    })
}

const TYPE_PREFIXES: [&str; 8] = [
    "REF", "TO", "LINE", "OF", "RANGE", "STANDARD", "SORTED", "TABLE",
];

/// Zieltyp nach `TYPE`/`LIKE` an Position `idx`: `REF TO zcl_x` -> `zcl_x`, `ztab-feld` -> `ztab`.
pub(crate) fn type_owner(toks: &[Token], idx: usize) -> Option<String> {
    let mut j = idx + 1;
    while toks
        .get(j)
        .is_some_and(|t| TYPE_PREFIXES.iter().any(|w| t.is_word(w)))
    {
        j += 1;
    }
    let t = toks
        .get(j)
        .filter(|t| t.kind == Kind::Word && !t.text.starts_with('<'))?;
    let owner = t.text.split('-').next().unwrap_or("").to_lowercase();
    if owner.is_empty() || matches!(owner.as_str(), "sy" | "syst") {
        return None;
    }
    Some(owner)
}

fn scan(s: &Statement, cx: &Ctx<'_>, out: &mut Vec<Pending>) {
    let toks = &s.tokens;
    let kw = s.keyword();
    let mut push = |kind: EdgeKind, hint: String, lookup: Lookup| {
        out.push(Pending {
            edge: Edge {
                from: cx.from.to_string(),
                to: None,
                hint,
                kind,
                file: cx.file.to_string(),
                line: s.line,
            },
            lookup,
        });
    };

    // Anweisungsbezogene Kanten
    match kw.as_str() {
        "CLASS" | "INTERFACE" => {
            if let Some(i) = toks.iter().position(|t| t.is_word("FROM")) {
                if let Some(parent) = name_at(toks, i + 1) {
                    push(EdgeKind::Inheritance, parent.clone(), Lookup::Exact(parent));
                }
            }
        }
        "INTERFACES" => {
            if let Some(n) = name_at(toks, 1) {
                push(EdgeKind::Interface, n.clone(), Lookup::Exact(n));
            }
        }
        "INCLUDE"
            if !toks
                .get(1)
                .is_some_and(|t| t.is_word("TYPE") || t.is_word("STRUCTURE") || t.text.starts_with('<')) =>
        {
            if let Some(n) = name_at(toks, 1) {
                push(EdgeKind::Include, n.clone(), Lookup::Exact(n));
            }
        }
        "SUBMIT" => {
            if let Some(n) = name_at(toks, 1) {
                push(EdgeKind::Submit, n.clone(), Lookup::Exact(n));
            }
        }
        "PERFORM" => {
            if let Some(n) = name_at(toks, 1) {
                let program = toks
                    .iter()
                    .position(|t| t.is_word("PROGRAM"))
                    .and_then(|i| name_at(toks, i + 1));
                match program {
                    Some(p) => push(
                        EdgeKind::Perform,
                        format!("{n} IN PROGRAM {p}"),
                        Lookup::Exact(format!("{p}::form:{n}")),
                    ),
                    None => push(
                        EdgeKind::Perform,
                        n.clone(),
                        Lookup::Exact(format!("{}::form:{n}", cx.obj)),
                    ),
                }
            }
        }
        "CALL" if toks.get(1).is_some_and(|t| t.is_word("FUNCTION")) => {
            if let Some(n) = name_at(toks, 2) {
                push(EdgeKind::FunctionCall, n.clone(), Lookup::Exact(format!("fm:{n}")));
            }
        }
        "CREATE" if toks.get(1).is_some_and(|t| t.is_word("OBJECT")) => {
            let class = toks
                .iter()
                .position(|t| t.is_word("TYPE"))
                .and_then(|i| name_at(toks, i + 1));
            if let Some(n) = class {
                push(EdgeKind::Construction, n.clone(), Lookup::Exact(n));
            }
        }
        "CATCH" if !toks.iter().any(|t| t.is_word("SYSTEM-EXCEPTIONS")) => {
            for t in toks.iter().skip(1).take_while(|t| !t.is_word("INTO")) {
                if t.kind == Kind::Word && !t.is_word("BEFORE") && !t.is_word("UNWIND") {
                    let n = t.text.to_lowercase();
                    push(EdgeKind::Exception, n.clone(), Lookup::Exact(n));
                }
            }
        }
        _ => {}
    }

    // Tokenbezogene Kanten
    for (i, t) in toks.iter().enumerate() {
        if t.is_word("NEW") && !(i > 0 && toks[i - 1].is_word("AT")) {
            if let Some(tok) = toks.get(i + 1).filter(|t| t.kind == Kind::Word) {
                let n = tok.text.to_lowercase();
                if !is_builtin_type(&n) {
                    push(EdgeKind::Construction, n.clone(), Lookup::Exact(n));
                }
            }
        }

        if (t.is_word("TYPE") && kw != "CREATE") || t.is_word("LIKE") {
            if let Some(owner) = type_owner(toks, i) {
                if !is_builtin_type(&owner) && !cx.local_types.contains(&owner) {
                    // LIKE verweist oft auf lokale Variablen: nur übernehmen, wenn es ein Knoten ist
                    let lookup = if t.is_word("LIKE") {
                        Lookup::Known(owner.clone())
                    } else {
                        Lookup::Exact(owner.clone())
                    };
                    push(EdgeKind::TypeReference, owner, lookup);
                }
            }
        }

        if !(t.is_punct("->") || t.is_punct("=>")) {
            continue;
        }
        let static_call = t.is_punct("=>");
        let Some(name_tok) = toks.get(i + 1).filter(|t| t.kind == Kind::Word) else {
            continue;
        };
        let (name, end) = match (toks.get(i + 2), toks.get(i + 3)) {
            (Some(tilde), Some(m)) if tilde.is_punct("~") && m.kind == Kind::Word => (
                format!("{}~{}", name_tok.text.to_lowercase(), m.text.to_lowercase()),
                i + 4,
            ),
            _ => (name_tok.text.to_lowercase(), i + 2),
        };
        // Aufruf nur mit Klammern oder in CALL METHOD; ohne beides ist es ein Attributzugriff
        let is_call = toks.get(end).is_some_and(|t| t.is_punct("("))
            || (kw == "CALL" && toks.get(1).is_some_and(|t| t.is_word("METHOD")));
        if !is_call {
            continue;
        }
        let recv = if i > 0 && toks[i - 1].kind == Kind::Word {
            Some(toks[i - 1].text.to_lowercase())
        } else {
            None
        };
        let op = if static_call { "=>" } else { "->" };
        let hint = format!("{}{op}{name}", recv.as_deref().unwrap_or(""));
        let has_tilde = name.contains('~');
        if static_call {
            let lookup = match recv {
                Some(class) if !has_tilde => Lookup::Method { class, name },
                _ => Lookup::None,
            };
            push(EdgeKind::StaticMethodCall, hint, lookup);
        } else {
            let lookup = match (recv.as_deref(), cx.class) {
                (Some("me"), Some(class)) if !has_tilde => Lookup::Method {
                    class: class.to_string(),
                    name,
                },
                _ => Lookup::None,
            };
            push(EdgeKind::MethodCall, hint, lookup);
        }
    }
}

/// Lokal deklarierte Typen eines Objekts (`TYPES ty_x`, `TYPES: BEGIN OF ty_x`). Auf sie wird nicht verwiesen.
fn local_types(scopes: &[Scope<'_>]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for scope in scopes {
        for s in &scope.stmts {
            if s.keyword() != "TYPES" {
                continue;
            }
            let name = match name_at(&s.tokens, 1) {
                Some(n) if n == "begin" => name_at(&s.tokens, 3),
                other => other,
            };
            if let Some(n) = name.filter(|n| n != "end") {
                out.insert(n);
            }
        }
    }
    out
}

/// Methode `name` der Klasse `class` oder einer ihrer Oberklassen.
fn resolve_method(
    nodes: &BTreeMap<String, Node>,
    parents: &BTreeMap<String, Vec<String>>,
    class: &str,
    name: &str,
) -> Option<String> {
    let mut todo = vec![class.to_string()];
    let mut seen = BTreeSet::new();
    while let Some(c) = todo.pop() {
        if !seen.insert(c.clone()) {
            continue;
        }
        let id = format!("{c}=>{name}");
        if nodes.contains_key(&id) {
            return Some(id);
        }
        if let Some(ps) = parents.get(&c) {
            todo.extend(ps.iter().cloned());
        }
    }
    None
}

/// Baut den Abhängigkeitsgraph aus `(Pfad, Quelltext)`-Paaren. Nicht-ABAP-Dateien werden ignoriert.
pub fn build(files: &[(String, String)]) -> Graph {
    let mut groups: BTreeMap<(String, String), Vec<(String, Program)>> = BTreeMap::new();
    for (path, src) in files {
        let Some(key) = object_key(path) else {
            continue;
        };
        let file = path.rsplit('/').next().unwrap_or(path).to_string();
        groups.entry(key).or_default().push((file, parse_l1(src)));
    }

    let mut nodes: BTreeMap<String, Node> = BTreeMap::new();
    let mut pending: Vec<Pending> = Vec::new();
    for ((obj_type, obj), progs) in &groups {
        let owner = Owner {
            obj_type: obj_type.as_str(),
            obj: obj.as_str(),
        };
        let mut scopes: Vec<Scope<'_>> = Vec::new();
        for (file, prog) in progs {
            scopes.extend(scopes_of(prog, file, &owner));
        }
        let local = local_types(&scopes);
        for scope in &scopes {
            let replace = scope.node.kind == NodeKind::Method && scope.class.is_some();
            put(&mut nodes, scope.node.clone(), replace);
            for node in &scope.extra {
                put(&mut nodes, node.clone(), false);
            }
            let cx = Ctx {
                from: scope.node.id.as_str(),
                class: scope.class.as_deref(),
                obj: obj.as_str(),
                file: scope.file.as_str(),
                local_types: &local,
            };
            for s in &scope.stmts {
                scan(s, &cx, &mut pending);
            }
        }
    }

    let mut parents: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for p in &pending {
        if p.edge.kind == EdgeKind::Inheritance {
            parents
                .entry(p.edge.from.clone())
                .or_default()
                .push(p.edge.hint.clone());
        }
    }

    let edges: Vec<Edge> = pending
        .into_iter()
        .filter_map(|p| {
            let to = match &p.lookup {
                Lookup::None => None,
                Lookup::Exact(id) | Lookup::Known(id) => nodes.contains_key(id).then(|| id.clone()),
                Lookup::Method { class, name } => resolve_method(&nodes, &parents, class, name),
            };
            if matches!(p.lookup, Lookup::Known(_)) && to.is_none() {
                return None;
            }
            Some(Edge { to, ..p.edge })
        })
        .collect();

    Graph {
        nodes: nodes.into_values().collect(),
        edges,
    }
}

/// Quelldateien je Entwicklungsobjekt, geparst. Nicht-ABAP-Dateien werden ignoriert.
pub(crate) fn group_programs(
    files: &[(String, String)],
) -> BTreeMap<(String, String), Vec<(String, Program)>> {
    let mut groups: BTreeMap<(String, String), Vec<(String, Program)>> = BTreeMap::new();
    for (path, src) in files {
        let Some(key) = object_key(path) else {
            continue;
        };
        let file = path.rsplit('/').next().unwrap_or(path).to_string();
        groups.entry(key).or_default().push((file, parse_l1(src)));
    }
    groups
}
