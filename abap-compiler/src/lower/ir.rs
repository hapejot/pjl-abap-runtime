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
}

/// Typ der Zeilen einer internen Tabelle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Elem {
    Int,
    Char(usize),
    Str,
    Struct(usize),
}

impl Elem {
    pub fn ty(self) -> Ty {
        match self {
            Elem::Int => Ty::Int,
            Elem::Char(n) => Ty::Char(n),
            Elem::Str => Ty::Str,
            Elem::Struct(i) => Ty::Struct(i),
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
    Loop { table: String, target: LoopTarget, body: Vec<Stmt>, line: usize },
    Describe { table: String, target: String, line: usize },
    Exit { line: usize },
    Check { cond: Cond, line: usize },
    Perform { name: String, args: Vec<Arg>, line: usize },
    /// RETURN: Ende der aktuellen FORM oder Methode
    Return { line: usize },
    /// CREATE OBJECT name
    Create { name: String, line: usize },
    /// obj->methode( args ). Empfänger `me` ist erlaubt.
    /// Methodenaufruf; `receiving` ist das RECEIVING-Ziel bei CALL METHOD
    MethodCall { recv: String, method: String, args: Vec<NamedArg>, receiving: Option<Target>, line: usize },
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
    pub methods: Vec<ClassMethod>,
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
}
