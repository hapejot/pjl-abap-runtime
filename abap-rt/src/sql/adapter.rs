//! Adaptervertrag (`doc/sql-spezifikation.md`, Abschnitt 6.1) und die Typen, die Adapter und Kern austauschen.
//!
//! Der Kern kennt keine Datenbank. Er ruft nur die Methoden des Traits auf und übersetzt die Ergebnisse
//! in den ABAP-Zielbereich. Jeder Adapter wird je Verbindung einmal angelegt und wiederverwendet.

use abap_sql_ir::{Expr, ResultMode, SelectOp, SqlOp, SqlRequest};

/// Kennung eines offenen Cursors, vom Adapter vergeben.
pub type CursorId = u64;

/// Hostvariable, wie sie an den Adapter übergeben wird. Gepackte Zahlen kommen als Dezimaltext.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParamValue {
    Int(i64),
    Text(String),
}

/// Rohwert aus der Datenbank. Die Umwandlung in den ABAP-Typ macht der Kern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawValue {
    Null,
    Int(i64),
    Text(String),
}

/// Ergebnis einer Ausführung. Abfragen liefern `rows` in der Reihenfolge von `SelectOp::columns`.
/// Schreibanweisungen haben keine Zeilen; `dbcnt` ist dann die Anzahl betroffener Zeilen (SY-DBCNT).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Outcome {
    pub rows: Vec<Vec<RawValue>>,
    pub dbcnt: i64,
}

/// Fähigkeiten eines Adapters (Abschnitt 8). Der Kern prüft jede Anweisung dagegen (`missing_capabilities`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Capabilities {
    pub select: bool,
    pub single: bool,
    pub where_clause: bool,
    pub between: bool,
    pub in_list: bool,
    pub like: bool,
    pub and_or_not: bool,
    pub order_by: bool,
    pub limit: bool,
    pub insert: bool,
    /// `UPDATE tab FROM wa` (Einzelsatz nach Schlüssel)
    pub update_by_key: bool,
    /// `UPDATE tab SET ... WHERE ...` (Mengenänderung)
    pub update_by_set: bool,
    /// `DELETE tab FROM wa` und `DELETE FROM TABLE itab` (nach Schlüssel)
    pub delete: bool,
    /// `DELETE FROM tab WHERE ...` (Mengenänderung)
    pub delete_by_set: bool,
    pub modify: bool,
    pub commit: bool,
    pub rollback: bool,
    pub concurrent_cursors: bool,
}

impl Capabilities {
    /// Fähigkeiten des SQLite-Adapters (Abschnitt 7.1). Alle außer `concurrent_cursors`.
    pub fn sqlite() -> Capabilities {
        Capabilities {
            select: true,
            single: true,
            where_clause: true,
            between: true,
            in_list: true,
            like: true,
            and_or_not: true,
            order_by: true,
            limit: true,
            insert: true,
            update_by_key: true,
            update_by_set: true,
            delete: true,
            delete_by_set: true,
            modify: true,
            commit: true,
            rollback: true,
            concurrent_cursors: false,
        }
    }

    /// Liest die Fähigkeiten aus einem JSON-Objekt. Fehlende oder nicht-boolesche Schlüssel gelten als `false`.
    pub fn from_json(value: &serde_json::Value) -> Capabilities {
        let flag = |name: &str| value.get(name).and_then(|v| v.as_bool()).unwrap_or(false);
        Capabilities {
            select: flag("select"),
            single: flag("single"),
            where_clause: flag("where"),
            between: flag("between"),
            in_list: flag("in"),
            like: flag("like"),
            and_or_not: flag("and_or_not"),
            order_by: flag("order_by"),
            limit: flag("limit"),
            insert: flag("insert"),
            update_by_key: flag("update_by_key"),
            update_by_set: flag("update_by_set"),
            delete: flag("delete"),
            delete_by_set: flag("delete_by_set"),
            modify: flag("modify"),
            commit: flag("commit"),
            rollback: flag("rollback"),
            concurrent_cursors: flag("concurrent_cursors"),
        }
    }

    /// Ob die benannte Fähigkeit (Namen wie in `missing_capabilities`) vorhanden ist.
    fn has(&self, name: &str) -> bool {
        match name {
            "select" => self.select,
            "single" => self.single,
            "where" => self.where_clause,
            "between" => self.between,
            "in" => self.in_list,
            "like" => self.like,
            "and_or_not" => self.and_or_not,
            "order_by" => self.order_by,
            "limit" => self.limit,
            "insert" => self.insert,
            "update_by_key" => self.update_by_key,
            "update_by_set" => self.update_by_set,
            "delete" => self.delete,
            "delete_by_set" => self.delete_by_set,
            "modify" => self.modify,
            "commit" => self.commit,
            "rollback" => self.rollback,
            _ => false,
        }
    }
}

/// DDIC-Name der Tabelle der Anweisung; `None` für `COMMIT WORK` und `ROLLBACK WORK`.
pub fn table_of(req: &SqlRequest) -> Option<&str> {
    match &req.op {
        SqlOp::Select(op) => Some(&op.table),
        SqlOp::Insert(op) => Some(&op.table),
        SqlOp::Update(op) => Some(&op.table),
        SqlOp::Delete(op) => Some(&op.table),
        SqlOp::Modify(op) => Some(&op.table),
        SqlOp::Commit | SqlOp::Rollback => None,
    }
}

/// Name der Operation einer Anweisung, wie in den Fähigkeiten (Abschnitt 8) und im Protokoll.
pub fn operation_name(req: &SqlRequest) -> &'static str {
    match &req.op {
        SqlOp::Select(_) => "select",
        SqlOp::Insert(_) => "insert",
        SqlOp::Update(op) if op.rows.is_some() => "update_by_key",
        SqlOp::Update(_) => "update_by_set",
        SqlOp::Delete(op) if op.rows.is_some() => "delete",
        SqlOp::Delete(_) => "delete_by_set",
        SqlOp::Modify(_) => "modify",
        SqlOp::Commit => "commit",
        SqlOp::Rollback => "rollback",
    }
}

/// Die Fähigkeiten, die der Adapter für diese Anweisung braucht und die ihm fehlen.
///
/// Die Liste ist deterministisch: Operation, `single`, `where`, dann die Ausdrucksformen in der Reihenfolge
/// ihres Auftretens (`between`, `in`, `like`, `and_or_not`), dann `order_by` und `limit`. Jede Fähigkeit steht
/// höchstens einmal in der Liste. Leer heißt: der Adapter kann die Anweisung ausführen.
///
/// Die Prüfung ist frei von Laufzeitabhängigkeiten, damit der Compiler sie zur Übersetzungszeit nutzen kann.
pub fn missing_capabilities(caps: &Capabilities, req: &SqlRequest) -> Vec<&'static str> {
    let mut needed: Vec<&'static str> = vec![operation_name(req)];
    let mut require = |name: &'static str| {
        if !needed.contains(&name) {
            needed.push(name);
        }
    };
    let mut filter: Option<&Expr> = None;
    let mut select_only: Option<&SelectOp> = None;
    match &req.op {
        SqlOp::Select(select) => {
            if select.result == ResultMode::Single {
                require("single");
            }
            filter = select.filter.as_ref();
            select_only = Some(select);
        }
        SqlOp::Update(op) => filter = op.filter.as_ref(),
        SqlOp::Delete(op) => filter = op.filter.as_ref(),
        SqlOp::Insert(_) | SqlOp::Modify(_) | SqlOp::Commit | SqlOp::Rollback => {}
    }
    if let Some(expr) = filter {
        require("where");
        expr_features(expr, &mut require);
    }
    if let Some(select) = select_only {
        if !select.order_by.is_empty() {
            require("order_by");
        }
        if select.up_to.is_some() {
            require("limit");
        }
    }
    needed.retain(|name| !caps.has(name));
    needed
}

/// Ausdrucksformen eines Filters, in der Reihenfolge des Auftretens.
fn expr_features(expr: &Expr, require: &mut impl FnMut(&'static str)) {
    match expr {
        Expr::Column { .. } | Expr::Param { .. } | Expr::Literal { .. } => {}
        Expr::Compare { left, right, .. } => {
            expr_features(left, require);
            expr_features(right, require);
        }
        Expr::Between { value, low, high } => {
            require("between");
            expr_features(value, require);
            expr_features(low, require);
            expr_features(high, require);
        }
        Expr::In { value, list } => {
            require("in");
            expr_features(value, require);
            for item in list {
                expr_features(item, require);
            }
        }
        Expr::Like { value, pattern } => {
            require("like");
            expr_features(value, require);
            expr_features(pattern, require);
        }
        Expr::And { left, right } | Expr::Or { left, right } => {
            require("and_or_not");
            expr_features(left, require);
            expr_features(right, require);
        }
        Expr::Not { operand } => {
            require("and_or_not");
            expr_features(operand, require);
        }
    }
}

/// Art eines Adapterfehlers. Der Kern stellt die Fehlermeldung mit Zeile und Verbindung zusammen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlErrorKind {
    /// Datenbankfehler (Verbindung, Constraint, Syntax, Datei nicht gefunden).
    Db,
    /// Verletzung eines eindeutigen Schlüssels (Dublette). Der Kern entscheidet, ob das ein Fehler ist
    /// (`INSERT FROM TABLE` ohne `ACCEPTING DUPLICATE KEYS`) oder ein Ergebnis mit `SY-SUBRC = 4`.
    Duplicate,
    /// Nicht unterstützte Fähigkeit (in `check`).
    Capability,
    /// Antwort passt nicht zum Protokoll.
    Protocol,
    /// Anfrage (IR) ist ungültig, z. B. unzulässiger Bezeichner.
    Invalid,
    /// Der Adapterprozess ist beendet oder hat das Zeitlimit überschritten. `message` ist vollständig
    /// bis auf die Zeilenangabe, `detail` (z. B. gesammeltes stderr) folgt nach einem Doppelpunkt.
    Process,
}

/// Fehler eines Adapters. `message` ist englisch und enthält keine Zugangsdaten.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SqlError {
    pub kind: SqlErrorKind,
    pub message: String,
    pub detail: Option<String>,
}

impl SqlError {
    pub fn new(kind: SqlErrorKind, message: impl Into<String>) -> SqlError {
        SqlError {
            kind,
            message: message.into(),
            detail: None,
        }
    }

    pub fn db(message: impl Into<String>) -> SqlError {
        SqlError::new(SqlErrorKind::Db, message)
    }

    /// Dublette: ein eindeutiger Schlüssel wäre doppelt.
    pub fn duplicate(message: impl Into<String>) -> SqlError {
        SqlError::new(SqlErrorKind::Duplicate, message)
    }

    pub fn capability(message: impl Into<String>) -> SqlError {
        SqlError::new(SqlErrorKind::Capability, message)
    }

    pub fn protocol(message: impl Into<String>) -> SqlError {
        SqlError::new(SqlErrorKind::Protocol, message)
    }

    pub fn invalid(message: impl Into<String>) -> SqlError {
        SqlError::new(SqlErrorKind::Invalid, message)
    }

    pub fn process(message: impl Into<String>) -> SqlError {
        SqlError::new(SqlErrorKind::Process, message)
    }
}

/// Vertrag eines Adapters (Abschnitt 6.1). Cursor und Transaktionen sind für SQLite und den externen Adapter
/// implementiert; der Codegen-Pfad ruft `check`, `execute` (Abfrage und Schreibanweisungen) und die Cursor-Methoden auf.
pub trait SqlAdapter: Send {
    /// Fähigkeiten, einmal beim Anlegen ermittelt.
    fn capabilities(&self) -> &Capabilities;
    /// Prüft die Anfrage vor der Ausführung (Bezeichner, Fähigkeiten).
    fn check(&self, req: &SqlRequest) -> Result<(), SqlError>;
    /// Führt eine Anweisung aus. Abfragen liefern `rows` (bei `up_to` höchstens so viele), Schreibanweisungen
    /// liefern `dbcnt` (Anzahl betroffener Zeilen). Eine Dublette ist `SqlErrorKind::Duplicate`.
    fn execute(&mut self, req: &SqlRequest, params: &[ParamValue]) -> Result<Outcome, SqlError>;
    /// Text der Anweisung für die Protokollierung (`ABAPC_TRACE_SQL`). Nur Adapter mit SQL-Text liefern ihn;
    /// der Text enthält nur Platzhalter, nie Werte.
    fn statement_text(&self, _req: &SqlRequest) -> Option<String> {
        None
    }
    /// Öffnet einen Cursor für `SELECT ... ENDSELECT`.
    fn open_cursor(
        &mut self,
        req: &SqlRequest,
        params: &[ParamValue],
    ) -> Result<CursorId, SqlError>;
    /// Liefert die nächste Zeile oder `None` am Ende.
    fn fetch(&mut self, cur: CursorId) -> Result<Option<Vec<RawValue>>, SqlError>;
    /// Schließt den Cursor. Fehler werden nicht gemeldet.
    fn close_cursor(&mut self, cur: CursorId);
    /// Beginnt eine LUW.
    fn begin(&mut self) -> Result<(), SqlError>;
    /// Schreibt die LUW fest.
    fn commit(&mut self) -> Result<(), SqlError>;
    /// Verwirft die LUW.
    fn rollback(&mut self) -> Result<(), SqlError>;
}

#[cfg(test)]
mod tests {
    use abap_sql_ir::{
        AbapType, Assignment, CharType, CmpOp, DeleteOp, InsertOp, ModifyOp, OrderItem, Param,
        ResultColumn, RowField, RowMode, RowSource, SelectOp, UpdateOp, Value as IrValue,
        IR_VERSION,
    };

    use super::*;

    fn request(op: SqlOp) -> SqlRequest {
        SqlRequest {
            ir_version: IR_VERSION,
            line: 4,
            connection: "DEFAULT".into(),
            op,
        }
    }

    fn col(name: &str) -> Expr {
        Expr::Column { name: name.into() }
    }

    fn text(value: &str) -> Expr {
        Expr::Literal {
            value: IrValue::Char {
                value: value.into(),
            },
        }
    }

    fn compare(left: Expr, right: Expr) -> Expr {
        Expr::Compare {
            op: CmpOp::Eq,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    fn select(result: ResultMode, filter: Option<Expr>, order: bool, up_to: Option<u64>) -> SqlOp {
        SqlOp::Select(SelectOp {
            table: "ZFLUG".into(),
            columns: vec![ResultColumn {
                name: "carrid".into(),
                source: AbapType::Int,
                target: AbapType::Int,
                offset: 0,
            }],
            filter,
            order_by: if order {
                vec![OrderItem {
                    column: "carrid".into(),
                    descending: false,
                }]
            } else {
                vec![]
            },
            up_to,
            result,
            target_size: 4,
            target_fields: vec![],
            params: vec![],
        })
    }

    fn row_source(mode: RowMode) -> RowSource {
        RowSource {
            mode,
            fields: vec![RowField {
                column: "carrid".into(),
                ty: AbapType::Char {
                    length: 2,
                    dtype: CharType::Char,
                },
                offset: 0,
            }],
            size: 4,
        }
    }

    fn update(rows: Option<RowSource>, filter: Option<Expr>) -> SqlOp {
        SqlOp::Update(UpdateOp {
            table: "ZFLUG".into(),
            assignments: vec![Assignment {
                column: "seatsmax".into(),
                value: Expr::Param { index: 0 },
            }],
            filter,
            rows,
            params: vec![Param {
                name: "lv".into(),
                ty: AbapType::Int,
            }],
        })
    }

    #[test]
    fn join_free_select_lists_every_missing_capability_once_in_order() {
        let filter = Expr::And {
            left: Box::new(Expr::Like {
                value: Box::new(col("carrid")),
                pattern: Box::new(text("L%")),
            }),
            right: Box::new(Expr::In {
                value: Box::new(col("connid")),
                list: vec![text("0400")],
            }),
        };
        let req = request(select(ResultMode::Single, Some(filter), true, Some(10)));
        assert_eq!(
            missing_capabilities(&Capabilities::default(), &req),
            vec![
                "select",
                "single",
                "where",
                "and_or_not",
                "like",
                "in",
                "order_by",
                "limit"
            ]
        );
    }

    #[test]
    fn select_with_all_features_needs_nothing_from_sqlite() {
        let req = request(select(ResultMode::Single, None, true, Some(10)));
        assert!(missing_capabilities(&Capabilities::sqlite(), &req).is_empty());
    }

    #[test]
    fn missing_feature_is_named_and_present_ones_are_not() {
        let mut caps = Capabilities::sqlite();
        caps.order_by = false;
        let req = request(select(ResultMode::Table, None, true, None));
        assert_eq!(missing_capabilities(&caps, &req), vec!["order_by"]);
    }

    #[test]
    fn between_is_reported_for_a_range_filter() {
        let filter = Expr::Between {
            value: Box::new(col("seats")),
            low: Box::new(text("1")),
            high: Box::new(text("9")),
        };
        let req = request(select(ResultMode::Table, Some(filter), false, None));
        let caps = Capabilities {
            select: true,
            where_clause: true,
            ..Capabilities::default()
        };
        assert_eq!(missing_capabilities(&caps, &req), vec!["between"]);
    }

    #[test]
    fn insert_needs_only_the_insert_capability() {
        let req = request(SqlOp::Insert(InsertOp {
            table: "ZFLUG".into(),
            rows: row_source(RowMode::Single),
            accepting_duplicate_keys: false,
        }));
        assert_eq!(
            missing_capabilities(&Capabilities::default(), &req),
            vec!["insert"]
        );
        assert!(missing_capabilities(&Capabilities::sqlite(), &req).is_empty());
    }

    #[test]
    fn update_by_key_and_update_by_set_are_told_apart() {
        let by_key = request(update(Some(row_source(RowMode::Single)), None));
        assert_eq!(
            missing_capabilities(&Capabilities::default(), &by_key),
            vec!["update_by_key"]
        );
        let by_set = request(update(
            None,
            Some(compare(col("carrid"), Expr::Param { index: 0 })),
        ));
        assert_eq!(
            missing_capabilities(&Capabilities::default(), &by_set),
            vec!["update_by_set", "where"]
        );
    }

    #[test]
    fn delete_by_key_and_delete_by_set_are_told_apart() {
        let by_key = request(SqlOp::Delete(DeleteOp {
            table: "ZFLUG".into(),
            filter: None,
            rows: Some(row_source(RowMode::Table)),
            params: vec![],
        }));
        assert_eq!(
            missing_capabilities(&Capabilities::default(), &by_key),
            vec!["delete"]
        );
        let all_rows = request(SqlOp::Delete(DeleteOp {
            table: "ZFLUG".into(),
            filter: None,
            rows: None,
            params: vec![],
        }));
        assert_eq!(
            missing_capabilities(&Capabilities::default(), &all_rows),
            vec!["delete_by_set"]
        );
    }

    #[test]
    fn modify_commit_and_rollback_name_their_capability() {
        let modify = request(SqlOp::Modify(ModifyOp {
            table: "ZFLUG".into(),
            rows: row_source(RowMode::Single),
        }));
        assert_eq!(
            missing_capabilities(&Capabilities::default(), &modify),
            vec!["modify"]
        );
        assert_eq!(
            missing_capabilities(&Capabilities::default(), &request(SqlOp::Commit)),
            vec!["commit"]
        );
        assert_eq!(
            missing_capabilities(&Capabilities::default(), &request(SqlOp::Rollback)),
            vec!["rollback"]
        );
    }

    #[test]
    fn capabilities_json_reads_the_write_flags() {
        let value = serde_json::json!({"insert": true, "delete_by_set": true, "modify": "yes"});
        let caps = Capabilities::from_json(&value);
        assert!(caps.insert && caps.delete_by_set);
        assert!(!caps.modify, "non-boolean flags count as missing");
        assert!(!caps.update_by_key);
    }

    #[test]
    fn duplicate_error_has_its_own_kind() {
        let err = SqlError::duplicate("UNIQUE constraint failed");
        assert_eq!(err.kind, SqlErrorKind::Duplicate);
        assert_eq!(err.message, "UNIQUE constraint failed");
    }
}
