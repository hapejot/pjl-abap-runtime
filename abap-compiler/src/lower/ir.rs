//! Typisierte Zwischendarstellung (IR) zwischen Parser und Codegen.

use super::*;
use crate::dynpro::{DynproFile, FieldType, Phase};
use abap_sql_ir::SqlRequest;

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
    /// Zur Übersetzungszeit unbekannt: untypisiertes oder `TYPE any` Feldsymbol, Typ erst zur Laufzeit
    Dynamic,
    /// Gepackte Dezimalzahl (DEC/P): Bytezahl im Speicher und Nachkommastellen
    Packed {
        bytes: usize,
        scale: usize,
    },
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Var(String),
    Slice {
        name: String,
        off: usize,
        len: usize,
    },
    Attr {
        obj: String,
        attr: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Data {
        name: String,
        ty: Ty,
        line: usize,
    },
    Assign {
        target: Target,
        value: Expr,
        line: usize,
    },
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
    Clear {
        target: Target,
        line: usize,
    },
    /// WRITE [/] [wert]. `newline`: `/` vor dem Wert beendet die aktuelle Zeile.
    Write {
        newline: bool,
        value: Option<Expr>,
        line: usize,
    },
    If {
        branches: Vec<(Cond, Vec<Stmt>)>,
        otherwise: Vec<Stmt>,
        line: usize,
    },
    Case {
        subject: Expr,
        whens: Vec<(Vec<Expr>, Vec<Stmt>)>,
        other: Vec<Stmt>,
        line: usize,
    },
    /// `DO n TIMES` (times = Some) oder endlose `DO` (times = None)
    Do {
        times: Option<Expr>,
        body: Vec<Stmt>,
        line: usize,
    },
    While {
        cond: Cond,
        body: Vec<Stmt>,
        line: usize,
    },
    /// ASSIGN quelle TO <fs>: Feldsymbol zeigt auf die Quelle, Typ-Tag wird übernommen
    AssignFs {
        fs: String,
        source: String,
        line: usize,
    },
    FieldSymbol {
        name: String,
        ty: Ty,
        line: usize,
    },
    Append {
        table: String,
        value: Expr,
        line: usize,
    },
    Loop {
        table: String,
        target: LoopTarget,
        filter: Option<LoopFilter>,
        body: Vec<Stmt>,
        line: usize,
    },
    Describe {
        table: String,
        target: String,
        line: usize,
    },
    Exit {
        line: usize,
    },
    Continue {
        line: usize,
    },
    Check {
        cond: Cond,
        line: usize,
    },
    Assert {
        cond: Cond,
        line: usize,
    },
    Perform {
        name: String,
        args: Vec<Arg>,
        line: usize,
    },
    /// RETURN: Ende der aktuellen FORM oder Methode
    Return {
        line: usize,
    },
    /// CREATE OBJECT name [EXPORTING ...]
    Create {
        name: String,
        args: Vec<NamedArg>,
        line: usize,
    },
    /// obj->methode( args ). Empfänger `me` ist erlaubt.
    /// Methodenaufruf; `receiving` ist das RECEIVING-Ziel bei CALL METHOD
    MethodCall {
        recv: String,
        method: String,
        args: Vec<NamedArg>,
        receiving: Option<Target>,
        line: usize,
    },
    ExternalFunctionCall {
        name: String,
        line: usize,
    },
    ExternalOperation {
        name: String,
        line: usize,
    },
    /// SQL-Anweisung ohne Cursor (`SELECT SINGLE`, `INTO TABLE`, `APPENDING TABLE`). `id` ist der Index in
    /// `Ir::sql_requests`, `params` die Hostvariablen in der Reihenfolge `SelectOp::params`, `target` das Ziel.
    Sql {
        id: u32,
        params: Vec<String>,
        target: String,
        line: usize,
    },
    /// `SELECT ... ENDSELECT`: Cursor, der Körper läuft je Zeile (siehe `Stmt::Sql` für die übrigen Felder)
    SqlLoop {
        id: u32,
        params: Vec<String>,
        target: String,
        body: Vec<Stmt>,
        line: usize,
    },
    /// Schreibende Open-SQL-Anweisung (`INSERT`, `UPDATE`, `DELETE`, `MODIFY`). `id` ist der Index in
    /// `Ir::sql_requests`, `params` die Hostvariablen der Anweisung in der Reihenfolge `params` der Anfrage.
    /// `target` ist der Arbeitsbereich bzw. die interne Tabelle der Zeilenquelle; leer heißt ohne Zeilenquelle.
    SqlWrite {
        id: u32,
        params: Vec<String>,
        target: String,
        line: usize,
    },
    /// `COMMIT WORK [AND WAIT]` (`rollback` false) bzw. `ROLLBACK WORK` (`rollback` true). `SY-SUBRC` wird gesetzt.
    SqlCommit {
        rollback: bool,
        line: usize,
    },
    /// Ausdrucksanweisung ohne Zuweisung, z.B. `a->m( )->n( x = 1 ).`
    ExprStmt {
        expr: Expr,
        line: usize,
    },
    Raise {
        exception: String,
        line: usize,
    },
    Try {
        body: Vec<Stmt>,
        catches: Vec<CatchClause>,
        cleanup: Vec<Stmt>,
        line: usize,
    },
    /// Dynpro-Anweisung (CALL/SET/LEAVE SCREEN, SET PF-STATUS, MESSAGE), siehe `ScreenOp`
    Screen {
        op: ScreenOp,
        line: usize,
    },
    NotSupported {
        tokens: Vec<Token>,
    },
}

/// Dynpro-Anweisung mit ihren Argumenten. Abbildung auf die Laufzeit: Spec 5.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScreenOp {
    /// CALL SCREEN n
    Call(u32),
    /// CALL SCREEN n STARTING AT x y [ENDING AT x2 y2] (Spec 12c). Spalte x, Zeile y der linken oberen
    /// Rahmenecke; `x2`, `y2` die rechte untere Ecke. Ohne ENDING AT sind `x2` und `y2` 0 (Größe aus SIZE).
    CallModal {
        no: u32,
        x: u32,
        y: u32,
        x2: u32,
        y2: u32,
    },
    /// SET SCREEN n (n > 0)
    Set(u32),
    /// LEAVE SCREEN, auch LEAVE TO SCREEN 0
    Leave,
    /// LEAVE TO SCREEN n (n > 0)
    LeaveTo(u32),
    /// LEAVE PROGRAM
    LeaveProgram,
    /// SET PF-STATUS 'name' (Großbuchstaben; gleicher Schlüssel wie der Name im GUISTATUS-Aufruf der Laufzeit)
    PfStatus(String),
    /// MESSAGE 'text' TYPE t; `kind` ist der Großbuchstabe des Typs
    Message { kind: char, text: String },
    /// SET CURSOR FIELD f (Großbuchstaben, wie an die Laufzeit übergeben)
    SetCursor(String),
    /// GET CURSOR FIELD f: Ziel ist eine Zeichenvariable, die der Codegen über ihre Adresse übergibt
    GetCursor(Target),
    /// SET TITLEBAR 'text' (nur Literal ohne WITH)
    SetTitlebar(String),
}

/// Ziel eines Dynpro-Verweises aus dem Programm; für die Prüfung gegen die `.dynp`-Datei.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScreenTarget {
    /// Dynpro-Nummer
    Dynpro(u32),
    /// Name des GUI-Status (Kleinschreibung)
    Status(String),
    /// Name eines Dynpro-Feldes in `SET CURSOR FIELD` (Kleinschreibung)
    Field(String),
}

/// Bezeichnung von `CALL SCREEN ... STARTING AT` in `ScreenRef::what` und Meldungen (Modalfenster, Spec 12c).
pub const MODAL_CALL: &str = "CALL SCREEN ... STARTING AT";

/// Verweis auf ein Dynpro oder einen GUI-Status aus einer Anweisung (`what`, z. B. "CALL SCREEN").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenRef {
    pub target: ScreenTarget,
    pub what: &'static str,
    pub line: usize,
}

/// Ablaufphase eines MODULE-Blocks: OUTPUT (PBO) oder INPUT (PAI).
/// `MODULE name OUTPUT ... ENDMODULE`: PBO; `MODULE name INPUT ... ENDMODULE`: PAI.
#[derive(Debug, Clone, PartialEq)]
pub struct ModuleDef {
    /// Name in Kleinschreibung
    pub name: String,
    pub phase: Phase,
    pub body: Vec<Stmt>,
    pub line: usize,
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

/// Ereignis, das im Ziel nie ausgeführt wird (z.B. AT SELECTION-SCREEN ohne Selektionsmaske).
/// Der Code wird übersetzt und geprüft, aber nicht in das Programm übernommen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InactiveEvent {
    pub name: String,
    pub line: usize,
}

#[derive(Debug, Clone)]
pub struct Ir {
    pub stmts: Vec<Stmt>,
    pub inactive_events: Vec<InactiveEvent>,
    pub forms: Vec<Form>,
    pub structs: Vec<StructDef>,
    pub classes: Vec<ClassDef>,
    pub constants: HashMap<String, Expr>,
    pub dependencies: Vec<String>,
    /// Feste Typen, die in dynamische Feldsymbole zugewiesen werden (für Prüfungen zur Laufzeit)
    pub dynamic_types: Vec<Ty>,
    /// MODULE-Blöcke (OUTPUT und INPUT) in Quellreihenfolge
    pub modules: Vec<ModuleDef>,
    /// Abbildung auf die Dynpro-Typen je Variablenname (Kleinschreibung), für `.dynp`-Prüfungen
    pub field_types: HashMap<String, FieldType>,
    /// Dynpro-Typen der Komponenten einer Struktur (Index in `structs`), in Komponentenreihenfolge.
    /// Fehlt eine Struktur, ist ihr Typ dem Dynpro nicht bekannt.
    pub struct_field_types: HashMap<usize, Vec<FieldType>>,
    /// Verweise auf Dynpros und GUI-Status aus den Anweisungen des Programms
    pub screen_refs: Vec<ScreenRef>,
    /// Inhalt der `.dynp`-Datei; `None`, wenn keine Datei gehört. Wird nach der Prüfung gesetzt.
    pub screens: Option<DynproFile>,
    /// SQL-Anweisungen des Programms (Phase 1). Der Index ist die `id` in `Stmt::Sql` und `Stmt::SqlLoop`.
    pub sql_requests: Vec<SqlRequest>,
}
