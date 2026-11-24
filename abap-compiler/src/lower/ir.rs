//! Typisierte Zwischendarstellung (IR) zwischen Parser und Codegen.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ty {
    Int,
    /// Zeichenfeld fester Länge (in Zeichen)
    Char(usize),
    /// Variabel lange Zeichenkette
    Str,
    /// Interne Tabelle mit Zeilen dieses Typs
    Table(Elem),
    /// Struktur, Index in `Ir::structs`
    Struct(usize),
    /// Objektreferenz (`TYPE REF TO`), Index in `Ir::classes`
    Ref(usize),
    /// Externer SAP-/DDIC-Typ, Index in `Ir::external_types`
    External(usize),
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoopFilter {
    pub component: String,
    pub value: Expr,
}

/// Typ der Zeilen einer internen Tabelle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Elem {
    Int,
    Char(usize),
    Str,
    Struct(usize),
    External(usize),
}

impl Elem {
    pub fn ty(self) -> Ty {
        match self {
            Elem::Int => Ty::Int,
            Elem::Char(n) => Ty::Char(n),
            Elem::Str => Ty::Str,
            Elem::Struct(i) => Ty::Struct(i),
            Elem::External(i) => Ty::External(i),
        }
    }
}

/// Komponenten einer Struktur in Deklarationsreihenfolge.
#[derive(Debug, Clone, PartialEq)]
pub struct StructDef {
    pub name: String,
    pub fields: Vec<(String, Ty)>,
}

/// Ziel einer LOOP-Schleife: Arbeitsbereich (INTO) oder Feldsymbol (ASSIGNING).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoopTarget {
    Into(String),
    Assigning(String),
}

/// Schreibziel: ganzes Feld, Teilfeld `name+off(len)` oder Attribut `obj->attr`.
/// Komponenten `s-feld` sind Teil des Namens.
#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    Var(String),
    Slice { name: String, off: usize, len: usize },
    Attr { obj: String, attr: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Data { name: String, ty: Ty, line: usize },
    Assign { target: Target, value: Expr, line: usize },
    AssignComponent {
        component: String,
        structure: String,
        target: String,
        line: usize,
    },
    ReadTable {
        table: String,
        target: String,
        index: Expr,
        line: usize,
    },
    Clear { target: Target, line: usize },
    /// WRITE [/] [wert]. `newline`: `/` vor dem Wert beendet die aktuelle Zeile.
    Write { newline: bool, value: Option<Expr>, line: usize },
    If { branches: Vec<(Cond, Vec<Stmt>)>, otherwise: Vec<Stmt>, line: usize },
    Case { subject: Expr, whens: Vec<(Vec<Expr>, Vec<Stmt>)>, other: Vec<Stmt>, line: usize },
    /// `DO n TIMES` (times = Some) oder endlose `DO` (times = None)
    Do { times: Option<Expr>, body: Vec<Stmt>, line: usize },
    While { cond: Cond, body: Vec<Stmt>, line: usize },
    FieldSymbol { name: String, ty: Ty, line: usize },
    Append { table: String, value: Expr, line: usize },
    Loop {
        table: String,
        target: LoopTarget,
        filter: Option<LoopFilter>,
        body: Vec<Stmt>,
        line: usize,
    },
    Describe { table: String, target: String, line: usize },
    Exit { line: usize },
    Continue { line: usize },
    Check { cond: Cond, line: usize },
    Assert { cond: Cond, line: usize },
    Perform { name: String, args: Vec<Arg>, line: usize },
    /// RETURN: Ende der aktuellen FORM oder Methode
    Return { line: usize },
    /// CREATE OBJECT name [EXPORTING ...]
    Create { name: String, args: Vec<NamedArg>, line: usize },
    /// obj->methode( args ). Empfänger `me` ist erlaubt.
    /// Methodenaufruf; `receiving` ist das RECEIVING-Ziel bei CALL METHOD
    MethodCall { recv: String, method: String, args: Vec<NamedArg>, receiving: Option<Target>, line: usize },
    ExternalFunctionCall { name: String, line: usize },
    ExternalOperation { name: String, line: usize },
    Raise { exception: String, line: usize },
    Try {
        body: Vec<Stmt>,
        catches: Vec<CatchClause>,
        cleanup: Vec<Stmt>,
        line: usize,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct CatchClause {
    pub class: String,
    pub target: Option<String>,
    pub body: Vec<Stmt>,
    pub line: usize,
}

/// Methode einer Klasse mit übersetzter Implementierung.
#[derive(Debug, Clone, PartialEq)]
pub struct RetParam {
    pub name: String,
    pub ty: Ty,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClassMethod {
    pub name: String,
    pub is_static: bool,
    pub params: Vec<Param>,
    /// RETURNING-Parameter, falls vorhanden
    pub ret: Option<RetParam>,
    pub body: Vec<Stmt>,
    pub line: usize,
}

/// Klasse: Attribute liegen in `Ir::structs[struct_idx]`.
#[derive(Debug, Clone, PartialEq)]
pub struct ClassDef {
    pub name: String,
    pub struct_idx: usize,
    pub parent: Option<ClassParent>,
    pub interfaces: Vec<InterfaceRef>,
    pub aliases: Vec<(String, String)>,
    pub is_abstract: bool,
    pub static_fields: Vec<(String, Ty)>,
    pub methods: Vec<ClassMethod>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InterfaceRef {
    Internal(String),
    External(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClassParent {
    Internal(usize),
    External(String),
}

/// Formalparameter einer FORM. Übergabe immer per Referenz.
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub ty: Ty,
    pub changing: bool,
}

/// Aktualparameter eines PERFORM.
#[derive(Debug, Clone, PartialEq)]
pub struct Arg {
    pub expr: Expr,
    pub changing: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Form {
    pub name: String,
    pub params: Vec<Param>,
    pub body: Vec<Stmt>,
    pub line: usize,
}

pub struct Ir {
    pub stmts: Vec<Stmt>,
    pub forms: Vec<Form>,
    pub structs: Vec<StructDef>,
    pub classes: Vec<ClassDef>,
    pub constants: HashMap<String, Expr>,
    pub external_types: Vec<String>,
}
