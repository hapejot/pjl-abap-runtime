//! Herleitung der minimal nötigen externen Anforderungen eines Exports.
//!
//! Aus der Verwendung im Quelltext wird geschlossen, was außerhalb des Exports existieren muss:
//! - `NEW zcl_x( )`, `CREATE OBJECT ... TYPE zcl_x`, `zcl_x=>m( )`, `CATCH zcx_x`: Klasse
//! - `TYPE REF TO zcl_x`, `zcl_x=>konst`: Klasse oder Interface
//! - `INTERFACES zif_x`, `o->zif_x~m( )`: Interface
//! - `o->m( )` mit `o` vom Typ T: T hat die Methode `m` (ggf. geerbt)
//! - `o->zif_x~m( )` mit `o` vom Typ T: T implementiert `zif_x`, und `zif_x` hat `m`
//!
//! Variablentypen stammen nur aus Deklarationen (`TYPE REF TO`, `CREATE OBJECT`) und
//! Zuweisungen `x = NEW zcl_x( )` bzw. `DATA(x) = NEW zcl_x( )`. Sie gelten flussunabhängig
//! für den ganzen Gültigkeitsbereich (Objekt, Klasse, Methode). Es wird nicht in externe
//! Typen hineinverfolgt; die Liste enthält daher nur, was direkt im Export verwendet wird.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use super::graph::{
    build as build_graph, group_programs, name_at, scopes_of, type_owner, Owner, Scope,
};
use super::lexer::{Kind, Token};
use super::parser::Statement;
use crate::inspect::model::is_builtin_type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TypeKind {
    Class,
    Interface,
    ClassOrInterface,
    /// Wird sowohl als Klasse als auch als Interface verwendet
    Conflict,
}

/// Belegstelle für eine Anforderung.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Evidence {
    pub role: &'static str,
    pub hint: String,
    pub file: String,
    pub line: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Requirement {
    pub name: String,
    pub kind: TypeKind,
    /// Liegt der Typ selbst im Export? Dann ist er nicht extern.
    pub in_export: bool,
    /// Benötigte Methoden (bei Interfaces die Methoden des Interfaces)
    pub methods: Vec<String>,
    /// Benötigte Interfaces, die die Klasse implementieren muss
    pub implements: Vec<String>,
    pub evidence: Vec<Evidence>,
}

#[derive(Debug, Default, Serialize)]
pub struct Requirements {
    pub items: Vec<Requirement>,
}

impl Requirements {
    /// Anforderungen, die außerhalb des Exports erfüllt werden müssen.
    pub fn external(&self) -> impl Iterator<Item = &Requirement> {
        self.items.iter().filter(|r| !r.in_export)
    }
}

#[derive(Default)]
struct Builder {
    class: bool,
    interface: bool,
    /// Verwendet als `TYPE REF TO` oder über `=>`: Klasse oder Interface
    either: bool,
    methods: BTreeSet<String>,
    implements: BTreeSet<String>,
    evidence: BTreeSet<Evidence>,
}

/// Variable -> mögliche Typen.
type Env = BTreeMap<String, BTreeSet<String>>;

struct Cx<'a> {
    from: &'a str,
    class: Option<&'a str>,
    file: &'a str,
    env: &'a Env,
}

fn entry<'m>(
    reqs: &'m mut BTreeMap<String, Builder>,
    cx: &Cx<'_>,
    line: usize,
    name: &str,
    role: &'static str,
    hint: &str,
) -> &'m mut Builder {
    let b = reqs.entry(name.to_string()).or_default();
    b.evidence.insert(Evidence {
        role,
        hint: hint.to_string(),
        file: cx.file.to_string(),
        line,
    });
    b
}

/// Variable links vor `TYPE` bzw. `=`, z.B. `lo` in `DATA lo TYPE REF TO zcl_x`.
fn var_before(toks: &[Token], idx: usize) -> Option<String> {
    let prev = toks.get(idx.checked_sub(1)?)?;
    if prev.kind != Kind::Word || prev.is_word("EXCEPTION") || prev.is_word("OBJECT") {
        return None;
    }
    Some(prev.text.to_lowercase())
}

/// Variable links vor `=`, auch in der Form `DATA(x) = ...`.
fn var_before_assign(toks: &[Token], idx: usize) -> Option<String> {
    let prev = toks.get(idx.checked_sub(1)?)?;
    if prev.kind == Kind::Word {
        return Some(prev.text.to_lowercase());
    }
    if prev.is_punct(")") {
        let inner = toks.get(idx.checked_sub(2)?)?;
        if inner.kind == Kind::Word {
            return Some(inner.text.to_lowercase());
        }
    }
    None
}

fn add_type(env: &mut Env, var: String, ty: String) {
    if !is_builtin_type(&ty) {
        env.entry(var).or_default().insert(ty);
    }
}

/// Sammelt Variablentypen aus einer Deklaration oder Zuweisung.
fn collect_env(s: &Statement, env: &mut Env) {
    let toks = &s.tokens;
    let kw = s.keyword();
    if matches!(kw.as_str(), "TYPES" | "TYPE-POOLS") {
        return;
    }
    for (i, t) in toks.iter().enumerate() {
        if t.is_word("TYPE") {
            let ref_to = toks.get(i + 1).is_some_and(|x| x.is_word("REF"));
            if ref_to || kw == "CREATE" {
                if let (Some(var), Some(ty)) = (var_before(toks, i), type_owner(toks, i)) {
                    add_type(env, var, ty);
                }
            }
        }
        // Event-Handler: `FOR EVENT ev OF zcl_x` -> der Parameter SENDER ist vom Typ zcl_x
        if t.is_word("OF") && i >= 2 && toks[i - 2].is_word("EVENT") {
            if let Some(cls) = name_at(toks, i + 1) {
                add_type(env, "sender".to_string(), cls);
            }
        }
        if t.is_punct("=") && toks.get(i + 1).is_some_and(|n| n.is_word("NEW")) {
            if let (Some(var), Some(ty)) = (
                var_before_assign(toks, i),
                toks.get(i + 2).filter(|x| x.kind == Kind::Word),
            ) {
                add_type(env, var, ty.text.to_lowercase());
            }
        }
    }
}

fn analyze(s: &Statement, cx: &Cx<'_>, reqs: &mut BTreeMap<String, Builder>) {
    let toks = &s.tokens;
    let kw = s.keyword();
    let line = s.line;

    for (i, t) in toks.iter().enumerate() {
        if t.is_word("NEW") && !(i > 0 && toks[i - 1].is_word("AT")) {
            if let Some(n) = toks.get(i + 1).filter(|x| x.kind == Kind::Word) {
                let n = n.text.to_lowercase();
                if !is_builtin_type(&n) {
                    entry(reqs, cx, line, &n, "new", &format!("NEW {n}")).class = true;
                }
            }
        }

        if t.is_word("TYPE") {
            if let Some(owner) = type_owner(toks, i).filter(|o| !is_builtin_type(o)) {
                if kw == "CREATE" {
                    entry(
                        reqs,
                        cx,
                        line,
                        &owner,
                        "create_object",
                        &format!("TYPE {owner}"),
                    )
                    .class = true;
                } else if kw == "RAISE" {
                    entry(reqs, cx, line, &owner, "raise", &format!("TYPE {owner}")).class = true;
                } else if toks.get(i + 1).is_some_and(|x| x.is_word("REF")) {
                    entry(
                        reqs,
                        cx,
                        line,
                        &owner,
                        "object_ref",
                        &format!("REF TO {owner}"),
                    )
                    .either = true;
                }
            }
        }

        // Statische Zugriffe: `zcl_x=>m( )` ist eine Klasse, `zif_x=>konst` Klasse oder Interface
        if t.is_punct("=>") && i > 0 && toks[i - 1].kind == Kind::Word && !toks[i - 1].is_word("me")
        {
            let x = toks[i - 1].text.to_lowercase();
            if !is_builtin_type(&x) {
                if let Some(m) = toks.get(i + 1).filter(|w| w.kind == Kind::Word) {
                    let m = m.text.to_lowercase();
                    let hint = format!("{x}=>{m}");
                    if toks.get(i + 2).is_some_and(|p| p.is_punct("(")) {
                        let b = entry(reqs, cx, line, &x, "static_call", &hint);
                        b.class = true;
                        b.methods.insert(m);
                    } else {
                        entry(reqs, cx, line, &x, "static_access", &hint).either = true;
                    }
                }
            }
        }

        // Methodenaufrufe auf typisierten Variablen
        if t.is_punct("->") && i > 0 {
            let Some(name_tok) = toks.get(i + 1).filter(|x| x.kind == Kind::Word) else {
                continue;
            };
            let (name, end) = match (toks.get(i + 2), toks.get(i + 3)) {
                (Some(tilde), Some(m)) if tilde.is_punct("~") && m.kind == Kind::Word => (
                    format!("{}~{}", name_tok.text.to_lowercase(), m.text.to_lowercase()),
                    i + 4,
                ),
                _ => (name_tok.text.to_lowercase(), i + 2),
            };
            let is_call = toks.get(end).is_some_and(|x| x.is_punct("("))
                || (kw == "CALL" && toks.get(1).is_some_and(|x| x.is_word("METHOD")));
            if !is_call || toks[i - 1].kind != Kind::Word {
                continue;
            }
            let recv = toks[i - 1].text.to_lowercase();
            let types: Vec<String> = if recv == "me" {
                cx.class.map(|c| vec![c.to_string()]).unwrap_or_default()
            } else {
                cx.env
                    .get(&recv)
                    .map(|set| set.iter().cloned().collect())
                    .unwrap_or_default()
            };
            let hint = format!("{recv}->{name}");
            for ty in types {
                if let Some((intf, m)) = name.split_once('~') {
                    entry(reqs, cx, line, &ty, "implements", &hint)
                        .implements
                        .insert(intf.to_string());
                    let ib = entry(reqs, cx, line, intf, "interface_method", &hint);
                    ib.interface = true;
                    ib.methods.insert(m.to_string());
                } else {
                    entry(reqs, cx, line, &ty, "method", &hint)
                        .methods
                        .insert(name.clone());
                }
            }
        }
    }

    // Event-Handler-Deklaration: die auslösende Klasse ist eine Anforderung
    if matches!(kw.as_str(), "METHODS" | "CLASS-METHODS") {
        if let Some(i) = toks.iter().position(|t| t.is_word("OF")) {
            if i >= 2 && toks[i - 2].is_word("EVENT") {
                if let Some(cls) = name_at(toks, i + 1) {
                    entry(
                        reqs,
                        cx,
                        line,
                        &cls,
                        "event",
                        &format!("FOR EVENT OF {cls}"),
                    )
                    .class = true;
                }
            }
        }
    }

    // Anweisungsbezogene Belege
    match kw.as_str() {
        "CLASS" | "INTERFACE" => {
            if let Some(i) = toks.iter().position(|t| t.is_word("FROM")) {
                if let Some(parent) = name_at(toks, i + 1) {
                    entry(reqs, cx, line, &parent, "inheritance", &parent).class = true;
                }
            }
        }
        "INTERFACES" => {
            if let Some(n) = name_at(toks, 1) {
                entry(reqs, cx, line, &n, "interface", &n).interface = true;
                let class = entry(reqs, cx, line, cx.from, "implements", &n);
                class.class = true;
                class.implements.insert(n.clone());
            }
        }
        "CATCH" if !toks.iter().any(|t| t.is_word("SYSTEM-EXCEPTIONS")) => {
            for t in toks.iter().skip(1).take_while(|t| !t.is_word("INTO")) {
                if t.kind == Kind::Word && !t.is_word("BEFORE") && !t.is_word("UNWIND") {
                    let n = t.text.to_lowercase();
                    entry(reqs, cx, line, &n, "catch", &n).class = true;
                }
            }
        }
        _ => {}
    }
}

fn merge(into: &mut Env, from: &Env) {
    for (var, types) in from {
        into.entry(var.clone())
            .or_default()
            .extend(types.iter().cloned());
    }
}

/// Leitet die externen Anforderungen aus `(Pfad, Quelltext)`-Paaren ab.
pub fn derive(files: &[(String, String)]) -> Requirements {
    let export: BTreeSet<String> = build_graph(files)
        .nodes
        .into_iter()
        .filter(|n| {
            matches!(
                n.kind,
                super::graph::NodeKind::Class | super::graph::NodeKind::Interface
            )
        })
        .map(|n| n.id)
        .collect();

    let mut reqs: BTreeMap<String, Builder> = BTreeMap::new();
    for ((obj_type, obj), progs) in &group_programs(files) {
        let owner = Owner {
            obj_type: obj_type.as_str(),
            obj: obj.as_str(),
        };
        let mut scopes: Vec<Scope<'_>> = Vec::new();
        for (file, prog) in progs {
            scopes.extend(scopes_of(prog, file, &owner));
        }

        // Umgebungen vorab: Objekt-Ebene, Klassenattribute, Methodenparameter
        let mut object_env = Env::new();
        let mut class_env: BTreeMap<String, Env> = BTreeMap::new();
        let mut param_env: BTreeMap<(String, String), Env> = BTreeMap::new();
        for scope in &scopes {
            match scope.node.kind {
                super::graph::NodeKind::Object => {
                    for s in &scope.stmts {
                        collect_env(s, &mut object_env);
                    }
                }
                super::graph::NodeKind::Class | super::graph::NodeKind::Interface => {
                    let cid = scope.node.id.clone();
                    for s in &scope.stmts {
                        if matches!(s.keyword().as_str(), "METHODS" | "CLASS-METHODS") {
                            let mname = name_at(&s.tokens, 1).unwrap_or_default();
                            collect_env(s, param_env.entry((cid.clone(), mname)).or_default());
                        } else {
                            collect_env(s, class_env.entry(cid.clone()).or_default());
                        }
                    }
                }
                _ => {}
            }
        }

        for scope in &scopes {
            let mut env = object_env.clone();
            if let Some(c) = &scope.class {
                if let Some(ce) = class_env.get(c) {
                    merge(&mut env, ce);
                }
                let mname = scope
                    .node
                    .id
                    .split_once("=>")
                    .map(|(_, m)| m.to_string())
                    .unwrap_or_default();
                if let Some(pe) = param_env.get(&(c.clone(), mname)) {
                    merge(&mut env, pe);
                }
            }
            for s in &scope.stmts {
                collect_env(s, &mut env);
            }
            let cx = Cx {
                from: scope.node.id.as_str(),
                class: scope.class.as_deref(),
                file: scope.file.as_str(),
                env: &env,
            };
            for s in &scope.stmts {
                analyze(s, &cx, &mut reqs);
            }
        }
    }

    let items = reqs
        .into_iter()
        .filter_map(|(name, b)| {
            let kind = match (b.class, b.interface) {
                (true, true) => TypeKind::Conflict,
                (true, false) => TypeKind::Class,
                (false, true) => TypeKind::Interface,
                (false, false) if b.either || !b.methods.is_empty() => TypeKind::ClassOrInterface,
                _ => return None,
            };
            Some(Requirement {
                in_export: export.contains(&name),
                name,
                kind,
                methods: b.methods.into_iter().collect(),
                implements: b.implements.into_iter().collect(),
                evidence: b.evidence.into_iter().collect(),
            })
        })
        .collect();

    Requirements { items }
}
