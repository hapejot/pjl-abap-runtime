//! SQLite-Adapter (`doc/sql-spezifikation.md`, Abschnitt 7.1).
//!
//! Die Datei wird mit Lese- und Schreibzugriff geöffnet, aber nicht angelegt (kein `SQLITE_OPEN_CREATE`);
//! eine fehlende Datei ist ein Fehler. Der SQL-Text entsteht aus der IR. Bezeichner stammen aus der
//! Konfiguration und werden geprüft und in doppelte Anführungszeichen gesetzt. Werte gehen als gebundene
//! Parameter (`?1`, `?2`, ...) in die Anfrage, nie als Text.
//!
//! Entscheidungen (Phase 2, im Bericht gekennzeichnet):
//! - `LIKE` ist case-sensitiv wie in ABAP. Die Anfrage nutzt `GLOB`, das Muster wird beim Binden umgesetzt:
//!   `%` wird `*`, `_` wird `?`, und `*`, `?`, `[` stehen in Klammern (`[*]`, `[?]`, `[[]`), damit sie wörtlich
//!   gelten. Die Umsetzung steht in `like_to_glob`.
//! - Schlüssel für `UPDATE ... FROM wa`, `DELETE ... FROM wa` und `MODIFY` ist `key` der Tabellenkonfiguration.
//!   Fehlt er, ist das ein Fehler.
//! - Eine Dublette ist nur die SQLite-Verletzung von `PRIMARY KEY` oder `UNIQUE` (`SqlError::duplicate`).
//!   Andere Constraint-Verletzungen (NOT NULL, CHECK) bleiben Datenbankfehler.
//! - `SY-DBCNT` einer Schreibanweisung ist die Zahl, die SQLite für die Anweisung zurückgibt
//!   (`changes()`). Eine Anweisung ohne Zeilenänderung ergibt 0.
//!
//! Annahme (Phase 1): Ein Cursor lädt alle Zeilen beim Öffnen in einen Puffer, `fetch` liefert aus dem Puffer.

use std::collections::{HashMap, VecDeque};
use std::path::Path;

use abap_sql_ir::{
    AbapType, CmpOp, DeleteOp, Expr, InsertOp, ModifyOp, ResultColumn, RowSource, SelectOp, SqlOp,
    SqlRequest, UpdateOp, Value as IrValue,
};
use rusqlite::config::DbConfig;
use rusqlite::ffi;
use rusqlite::types::{Value as SqlValue, ValueRef};
use rusqlite::{params_from_iter, Connection, OpenFlags};

use super::adapter::{
    table_of, Capabilities, CursorId, Outcome, ParamValue, RawValue, SqlAdapter, SqlError,
};
use super::config::{is_identifier, TableConfig};

/// Wert, der beim Ausführen an einen Platzhalter gebunden wird.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bind {
    /// Hostvariable mit Index in den Hostparametern der Anweisung (Zeilenwerte zählen dazu, siehe IR)
    Param(u32),
    /// Hostvariable als `LIKE`-Muster; wird beim Binden nach `GLOB` umgesetzt
    LikeParam(u32),
    /// Ganzzahl aus dem Quelltext
    Int(i64),
    /// Zeichenkette aus dem Quelltext, nachgestellte Leerzeichen bereits entfernt
    Text(String),
}

/// SQL-Text mit Platzhaltern `?1`, `?2`, ... und den Werten in Reihenfolge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltSql {
    pub sql: String,
    pub binds: Vec<Bind>,
}

/// Übersetzt ein ABAP-`LIKE`-Muster in ein `GLOB`-Muster. Case-sensitiv wie ABAP.
///
/// `%` wird `*` (beliebige Folge, auch leer), `_` wird `?` (genau ein Zeichen). Die Metazeichen von `GLOB`,
/// die im ABAP-Muster Daten sind, stehen in Klammern: `*` als `[*]`, `?` als `[?]`, `[` als `[[]`.
pub fn like_to_glob(pattern: &str) -> String {
    let mut out = String::with_capacity(pattern.len());
    for c in pattern.chars() {
        match c {
            '%' => out.push('*'),
            '_' => out.push('?'),
            '*' | '?' | '[' => {
                out.push('[');
                out.push(c);
                out.push(']');
            }
            other => out.push(other),
        }
    }
    out
}

/// SQLite-Verbindung mit den Tabellen, die über sie laufen.
pub struct SqliteAdapter {
    conn: Connection,
    /// DDIC-Name (Großbuchstaben) -> Tabellenkonfiguration
    tables: HashMap<String, TableConfig>,
    capabilities: Capabilities,
    cursors: HashMap<CursorId, VecDeque<Vec<RawValue>>>,
    next_cursor: CursorId,
}

impl SqliteAdapter {
    /// Öffnet die Datei mit Lese- und Schreibzugriff. Eine fehlende Datei ist ein Fehler, sie wird nicht angelegt.
    pub fn open_file(path: &Path, tables: Vec<TableConfig>) -> Result<SqliteAdapter, SqlError> {
        if !path.is_file() {
            return Err(SqlError::db(format!(
                "database file not found: {}",
                path.display()
            )));
        }
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(db_error)?;
        SqliteAdapter::new(conn, tables)
    }

    /// Adapter über eine vorhandene Verbindung (für In-Memory-Datenbanken in Tests).
    ///
    /// Schaltet die Behandlung doppelt gequoteter Namen als Zeichenkette ab (DQS). Sonst würde ein
    /// falscher Spaltenname wie `"carrier"` still als Text `carrier` gelesen, statt einen Fehler zu liefern.
    pub fn new(conn: Connection, tables: Vec<TableConfig>) -> Result<SqliteAdapter, SqlError> {
        conn.set_db_config(DbConfig::SQLITE_DBCONFIG_DQS_DML, false)
            .map_err(db_error)?;
        conn.set_db_config(DbConfig::SQLITE_DBCONFIG_DQS_DDL, false)
            .map_err(db_error)?;
        Ok(SqliteAdapter {
            conn,
            tables: tables.into_iter().map(|t| (t.ddic.clone(), t)).collect(),
            capabilities: Capabilities::sqlite(),
            cursors: HashMap::new(),
            next_cursor: 1,
        })
    }

    fn table(&self, ddic: &str) -> Result<&TableConfig, SqlError> {
        self.tables.get(&ddic.to_uppercase()).ok_or_else(|| {
            SqlError::invalid(format!("table {ddic} is not configured for this adapter"))
        })
    }

    /// Baut die Anfrage. Prüft dabei alle Bezeichner.
    fn build(&self, req: &SqlRequest) -> Result<BuiltSql, SqlError> {
        let name = table_of(req)
            .ok_or_else(|| SqlError::invalid("statement without table has no SQL text"))?;
        let table = self.table(name)?;
        build_statement(req, table)
    }

    fn query(
        &self,
        built: &BuiltSql,
        params: &[ParamValue],
        columns: &[ResultColumn],
    ) -> Result<Vec<Vec<RawValue>>, SqlError> {
        let values = bind_all(built, params)?;
        let mut statement = self.conn.prepare(&built.sql).map_err(db_error)?;
        let mut rows = statement
            .query(params_from_iter(values))
            .map_err(db_error)?;
        let mut result = Vec::new();
        while let Some(row) = rows.next().map_err(db_error)? {
            let mut out = Vec::with_capacity(columns.len());
            for (index, column) in columns.iter().enumerate() {
                let value = row.get_ref(index).map_err(db_error)?;
                out.push(raw_value(value, &column.source)?);
            }
            result.push(out);
        }
        Ok(result)
    }

    /// Führt eine Schreibanweisung aus und liefert die Anzahl der geänderten Zeilen.
    fn write(&self, built: &BuiltSql, params: &[ParamValue]) -> Result<i64, SqlError> {
        let values = bind_all(built, params)?;
        let changed = self
            .conn
            .execute(&built.sql, params_from_iter(values))
            .map_err(write_error)?;
        Ok(changed as i64)
    }
}

impl SqlAdapter for SqliteAdapter {
    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    fn check(&self, req: &SqlRequest) -> Result<(), SqlError> {
        self.build(req).map(|_| ())
    }

    fn execute(&mut self, req: &SqlRequest, params: &[ParamValue]) -> Result<Outcome, SqlError> {
        let built = self.build(req)?;
        match &req.op {
            SqlOp::Select(select) => {
                let rows = self.query(&built, params, &select.columns)?;
                Ok(Outcome { rows, dbcnt: 0 })
            }
            _ => {
                let dbcnt = self.write(&built, params)?;
                Ok(Outcome {
                    rows: Vec::new(),
                    dbcnt,
                })
            }
        }
    }

    fn statement_text(&self, req: &SqlRequest) -> Option<String> {
        self.build(req).ok().map(|built| built.sql)
    }

    fn open_cursor(
        &mut self,
        req: &SqlRequest,
        params: &[ParamValue],
    ) -> Result<CursorId, SqlError> {
        let outcome = self.execute(req, params)?;
        let id = self.next_cursor;
        self.next_cursor += 1;
        self.cursors.insert(id, outcome.rows.into());
        Ok(id)
    }

    fn fetch(&mut self, cur: CursorId) -> Result<Option<Vec<RawValue>>, SqlError> {
        let rows = self
            .cursors
            .get_mut(&cur)
            .ok_or_else(|| SqlError::protocol(format!("unknown cursor {cur}")))?;
        Ok(rows.pop_front())
    }

    fn close_cursor(&mut self, cur: CursorId) {
        self.cursors.remove(&cur);
    }

    fn begin(&mut self) -> Result<(), SqlError> {
        self.conn.execute_batch("BEGIN").map_err(db_error)
    }

    fn commit(&mut self) -> Result<(), SqlError> {
        self.conn.execute_batch("COMMIT").map_err(db_error)
    }

    fn rollback(&mut self) -> Result<(), SqlError> {
        self.conn.execute_batch("ROLLBACK").map_err(db_error)
    }
}

fn db_error(error: rusqlite::Error) -> SqlError {
    SqlError::db(error.to_string())
}

/// Fehler einer Schreibanweisung. Nur die Verletzung eines eindeutigen Schlüssels ist eine Dublette.
fn write_error(error: rusqlite::Error) -> SqlError {
    match &error {
        rusqlite::Error::SqliteFailure(failure, _)
            if matches!(
                failure.extended_code,
                ffi::SQLITE_CONSTRAINT_PRIMARYKEY | ffi::SQLITE_CONSTRAINT_UNIQUE
            ) =>
        {
            SqlError::duplicate(error.to_string())
        }
        _ => db_error(error),
    }
}

/// Werte für die Platzhalter in Reihenfolge der Bindung.
fn bind_all(built: &BuiltSql, params: &[ParamValue]) -> Result<Vec<SqlValue>, SqlError> {
    built
        .binds
        .iter()
        .map(|bind| bind_value(bind, params))
        .collect()
}

/// Baut den Text einer Anweisung aus der IR (Abfrage oder Schreibanweisung). Bezeichner werden geprüft und
/// quotiert, Werte nur als Platzhalter. `COMMIT` und `ROLLBACK` haben keinen SQL-Text in diesem Adapter.
pub fn build_statement(req: &SqlRequest, table: &TableConfig) -> Result<BuiltSql, SqlError> {
    match &req.op {
        SqlOp::Select(select) => build_select(select, table),
        SqlOp::Insert(op) => build_insert(op, table),
        SqlOp::Update(op) => build_update(op, table),
        SqlOp::Delete(op) => build_delete(op, table),
        SqlOp::Modify(op) => build_modify(op, table),
        SqlOp::Commit | SqlOp::Rollback => Err(SqlError::invalid(
            "COMMIT and ROLLBACK are not SQL statements",
        )),
    }
}

fn check_target(table: &TableConfig) -> Result<(), SqlError> {
    if !is_identifier(&table.target) {
        return Err(SqlError::invalid(format!(
            "invalid target table name {}",
            table.target
        )));
    }
    Ok(())
}

/// Schlüsselspalten der Tabelle aus der Konfiguration. Ohne Schlüssel ist keine Zuordnung nach Schlüssel möglich.
fn key_columns(table: &TableConfig) -> Result<&[String], SqlError> {
    if table.key.is_empty() {
        return Err(SqlError::invalid(format!(
            "table {} has no key in configuration",
            table.ddic
        )));
    }
    Ok(&table.key)
}

fn is_key(table: &TableConfig, column: &str) -> bool {
    table.key.iter().any(|key| key.eq_ignore_ascii_case(column))
}

/// Position des Schlüsselfelds in der Zeilenquelle. Fehlt es, ist die Anweisung nicht ausführbar.
fn key_position(source: &RowSource, key: &str) -> Result<usize, SqlError> {
    source
        .fields
        .iter()
        .position(|field| field.column.eq_ignore_ascii_case(key))
        .ok_or_else(|| SqlError::invalid(format!("key column {key} is missing in the row source")))
}

/// `INSERT INTO t (spalten) VALUES (...)`. Die Werte sind die Zeilenfelder, Feld `k` ist Parameter `k`
/// (ohne Hostparameter, siehe IR).
fn build_insert(op: &InsertOp, table: &TableConfig) -> Result<BuiltSql, SqlError> {
    check_target(table)?;
    if op.rows.fields.is_empty() {
        return Err(SqlError::invalid("INSERT without columns"));
    }
    let mut builder = Builder::new(table);
    let mut columns = Vec::with_capacity(op.rows.fields.len());
    let mut values = Vec::with_capacity(op.rows.fields.len());
    for (index, field) in op.rows.fields.iter().enumerate() {
        columns.push(builder.column(&field.column)?);
        values.push(builder.bind(Bind::Param(index as u32)));
    }
    Ok(BuiltSql {
        sql: format!(
            "INSERT INTO \"{}\" ({}) VALUES ({})",
            table.target,
            columns.join(", "),
            values.join(", ")
        ),
        binds: builder.binds,
    })
}

/// `UPDATE t SET ... [WHERE ...]` (Form `SET`) oder `UPDATE t SET <Nicht-Schlüssel> WHERE <Schlüssel>` (Form
/// `FROM wa`). In der Form `FROM` stehen die Zeilenfelder nach den Hostparametern der Anweisung.
fn build_update(op: &UpdateOp, table: &TableConfig) -> Result<BuiltSql, SqlError> {
    check_target(table)?;
    let mut builder = Builder::new(table);
    let base = op.params.len() as u32;
    let mut sets = Vec::new();
    match &op.rows {
        None => {
            if op.assignments.is_empty() {
                return Err(SqlError::invalid("UPDATE without SET"));
            }
            for assignment in &op.assignments {
                let column = builder.column(&assignment.column)?;
                let value = builder.value(&assignment.value)?;
                sets.push(format!("{column} = {value}"));
            }
        }
        Some(source) => {
            for (index, field) in source.fields.iter().enumerate() {
                if !is_key(table, &field.column) {
                    let column = builder.column(&field.column)?;
                    let value = builder.bind(Bind::Param(base + index as u32));
                    sets.push(format!("{column} = {value}"));
                }
            }
            if sets.is_empty() {
                return Err(SqlError::invalid("UPDATE without columns to change"));
            }
        }
    }
    let mut sql = format!("UPDATE \"{}\" SET {}", table.target, sets.join(", "));
    match &op.rows {
        None => {
            if let Some(filter) = &op.filter {
                let condition = builder.expr(filter)?;
                sql.push_str(" WHERE ");
                sql.push_str(&condition);
            }
        }
        Some(source) => {
            let conditions = key_conditions(&mut builder, table, source, base)?;
            sql.push_str(" WHERE ");
            sql.push_str(&conditions.join(" AND "));
        }
    }
    Ok(BuiltSql {
        sql,
        binds: builder.binds,
    })
}

/// `DELETE FROM t [WHERE ...]` (Menge) oder `DELETE FROM t WHERE <Schlüssel>` (Form `FROM wa`).
fn build_delete(op: &DeleteOp, table: &TableConfig) -> Result<BuiltSql, SqlError> {
    check_target(table)?;
    let mut builder = Builder::new(table);
    let mut sql = format!("DELETE FROM \"{}\"", table.target);
    match &op.rows {
        None => {
            if let Some(filter) = &op.filter {
                let condition = builder.expr(filter)?;
                sql.push_str(" WHERE ");
                sql.push_str(&condition);
            }
        }
        Some(source) => {
            let base = op.params.len() as u32;
            let conditions = key_conditions(&mut builder, table, source, base)?;
            sql.push_str(" WHERE ");
            sql.push_str(&conditions.join(" AND "));
        }
    }
    Ok(BuiltSql {
        sql,
        binds: builder.binds,
    })
}

/// Schlüsselbedingungen `spalte = ?` aus den Schlüsselfeldern der Zeilenquelle.
fn key_conditions(
    builder: &mut Builder<'_>,
    table: &TableConfig,
    source: &RowSource,
    base: u32,
) -> Result<Vec<String>, SqlError> {
    let mut conditions = Vec::new();
    for key in key_columns(table)? {
        let index = key_position(source, key)?;
        let column = builder.column(&source.fields[index].column)?;
        let value = builder.bind(Bind::Param(base + index as u32));
        conditions.push(format!("{column} = {value}"));
    }
    Ok(conditions)
}

/// `MODIFY`: `INSERT ... ON CONFLICT (schlüssel) DO UPDATE SET <Nicht-Schlüssel>`. Die Änderung zählt als eine
/// Zeile, auch wenn sich der Wert nicht ändert. Ohne Nicht-Schlüsselfelder setzt die Anweisung den Schlüssel
/// auf sich selbst, damit die Zeile dennoch als verarbeitet zählt.
fn build_modify(op: &ModifyOp, table: &TableConfig) -> Result<BuiltSql, SqlError> {
    check_target(table)?;
    let keys = key_columns(table)?;
    if op.rows.fields.is_empty() {
        return Err(SqlError::invalid("MODIFY without columns"));
    }
    for key in keys {
        key_position(&op.rows, key)?;
    }
    let mut builder = Builder::new(table);
    let mut columns = Vec::with_capacity(op.rows.fields.len());
    let mut values = Vec::with_capacity(op.rows.fields.len());
    let mut updates = Vec::new();
    for (index, field) in op.rows.fields.iter().enumerate() {
        let column = builder.column(&field.column)?;
        values.push(builder.bind(Bind::Param(index as u32)));
        if !is_key(table, &field.column) {
            updates.push(format!("{column} = excluded.{column}"));
        }
        columns.push(column);
    }
    let mut target = Vec::with_capacity(keys.len());
    for key in keys {
        target.push(builder.column(key)?);
    }
    if updates.is_empty() {
        let first = &target[0];
        updates.push(format!("{first} = excluded.{first}"));
    }
    Ok(BuiltSql {
        sql: format!(
            "INSERT INTO \"{}\" ({}) VALUES ({}) ON CONFLICT ({}) DO UPDATE SET {}",
            table.target,
            columns.join(", "),
            values.join(", "),
            target.join(", "),
            updates.join(", ")
        ),
        binds: builder.binds,
    })
}

/// Baut den SELECT-Text. Bezeichner werden geprüft und quotiert, Werte nur als Platzhalter.
pub fn build_select(select: &SelectOp, table: &TableConfig) -> Result<BuiltSql, SqlError> {
    if select.columns.is_empty() {
        return Err(SqlError::invalid("SELECT without result columns"));
    }
    check_target(table)?;
    let mut builder = Builder::new(table);
    let mut projection = Vec::with_capacity(select.columns.len());
    for column in &select.columns {
        projection.push(builder.column(&column.name)?);
    }
    let mut sql = format!("SELECT {} FROM \"{}\"", projection.join(", "), table.target);
    if let Some(filter) = &select.filter {
        let condition = builder.expr(filter)?;
        sql.push_str(" WHERE ");
        sql.push_str(&condition);
    }
    if !select.order_by.is_empty() {
        let mut items = Vec::with_capacity(select.order_by.len());
        for item in &select.order_by {
            let direction = if item.descending { "DESC" } else { "ASC" };
            items.push(format!("{} {direction}", builder.column(&item.column)?));
        }
        sql.push_str(" ORDER BY ");
        sql.push_str(&items.join(", "));
    }
    if let Some(limit) = select.up_to {
        // Eine ganze Zahl aus der IR, kein Text aus dem Programm.
        sql.push_str(&format!(" LIMIT {limit}"));
    }
    Ok(BuiltSql {
        sql,
        binds: builder.binds,
    })
}

struct Builder<'a> {
    table: &'a TableConfig,
    binds: Vec<Bind>,
}

impl<'a> Builder<'a> {
    fn new(table: &'a TableConfig) -> Builder<'a> {
        Builder {
            table,
            binds: Vec::new(),
        }
    }

    /// Spaltenname im Ziel, geprüft und quotiert.
    fn column(&self, ddic_column: &str) -> Result<String, SqlError> {
        let physical = self.table.column(ddic_column);
        if !is_identifier(&physical) {
            return Err(SqlError::invalid(format!(
                "invalid column name {ddic_column}"
            )));
        }
        Ok(format!("\"{physical}\""))
    }

    fn bind(&mut self, bind: Bind) -> String {
        self.binds.push(bind);
        format!("?{}", self.binds.len())
    }

    /// Literal aus dem Quelltext als Platzhalter.
    fn literal(&mut self, value: &IrValue) -> String {
        match value {
            IrValue::Int { value } => self.bind(Bind::Int(*value)),
            IrValue::Char { value } => {
                self.bind(Bind::Text(value.trim_end_matches(' ').to_string()))
            }
        }
    }

    /// Wert einer Zuweisung: Hostvariable oder Literal.
    fn value(&mut self, expr: &Expr) -> Result<String, SqlError> {
        match expr {
            Expr::Param { index } => Ok(self.bind(Bind::Param(*index))),
            Expr::Literal { value } => Ok(self.literal(value)),
            _ => Err(SqlError::invalid(
                "SET value must be a host variable or a literal",
            )),
        }
    }

    /// Muster von `LIKE`: Hostvariable oder Zeichenliteral, nach `GLOB` umgesetzt beim Binden.
    fn pattern(&mut self, expr: &Expr) -> Result<String, SqlError> {
        match expr {
            Expr::Param { index } => Ok(self.bind(Bind::LikeParam(*index))),
            Expr::Literal {
                value: IrValue::Char { value },
            } => Ok(self.bind(Bind::Text(like_to_glob(value.trim_end_matches(' '))))),
            _ => Err(SqlError::invalid(
                "LIKE pattern must be a host variable or a character literal",
            )),
        }
    }

    fn expr(&mut self, expr: &Expr) -> Result<String, SqlError> {
        Ok(match expr {
            Expr::Column { name } => self.column(name)?,
            Expr::Param { index } => self.bind(Bind::Param(*index)),
            Expr::Literal { value } => self.literal(value),
            Expr::Compare { op, left, right } => {
                let l = self.expr(left)?;
                let r = self.expr(right)?;
                format!("({l} {} {r})", cmp_sql(*op))
            }
            Expr::Between { value, low, high } => {
                let v = self.expr(value)?;
                let lo = self.expr(low)?;
                let hi = self.expr(high)?;
                format!("({v} BETWEEN {lo} AND {hi})")
            }
            Expr::In { value, list } => {
                if list.is_empty() {
                    return Err(SqlError::invalid("IN list is empty"));
                }
                let v = self.expr(value)?;
                let mut items = Vec::with_capacity(list.len());
                for item in list {
                    items.push(self.expr(item)?);
                }
                format!("({v} IN ({}))", items.join(", "))
            }
            Expr::Like { value, pattern } => {
                let v = self.expr(value)?;
                let p = self.pattern(pattern)?;
                format!("({v} GLOB {p})")
            }
            Expr::And { left, right } => {
                let l = self.expr(left)?;
                let r = self.expr(right)?;
                format!("({l} AND {r})")
            }
            Expr::Or { left, right } => {
                let l = self.expr(left)?;
                let r = self.expr(right)?;
                format!("({l} OR {r})")
            }
            Expr::Not { operand } => format!("(NOT {})", self.expr(operand)?),
        })
    }
}

fn cmp_sql(op: CmpOp) -> &'static str {
    match op {
        CmpOp::Eq => "=",
        CmpOp::Ne => "<>",
        CmpOp::Lt => "<",
        CmpOp::Le => "<=",
        CmpOp::Gt => ">",
        CmpOp::Ge => ">=",
    }
}

fn host_at(index: u32, params: &[ParamValue]) -> Result<&ParamValue, SqlError> {
    params
        .get(index as usize)
        .ok_or_else(|| SqlError::invalid(format!("host variable index {index} is out of range")))
}

fn bind_value(bind: &Bind, params: &[ParamValue]) -> Result<SqlValue, SqlError> {
    match bind {
        Bind::Int(value) => Ok(SqlValue::Integer(*value)),
        Bind::Text(text) => Ok(SqlValue::Text(text.clone())),
        Bind::Param(index) => Ok(match host_at(*index, params)? {
            ParamValue::Int(value) => SqlValue::Integer(*value),
            ParamValue::Text(text) => SqlValue::Text(text.trim_end_matches(' ').to_string()),
        }),
        Bind::LikeParam(index) => Ok(match host_at(*index, params)? {
            ParamValue::Int(value) => SqlValue::Text(like_to_glob(&value.to_string())),
            ParamValue::Text(text) => SqlValue::Text(like_to_glob(text.trim_end_matches(' '))),
        }),
    }
}

/// Wandelt einen Wert der Datenbank in einen Rohwert. Der Typ der Spalte aus der IR entscheidet,
/// ob ein Integer als Zahl oder als Text geliefert wird.
fn raw_value(value: ValueRef<'_>, source: &AbapType) -> Result<RawValue, SqlError> {
    let is_int = matches!(source, AbapType::Int);
    match value {
        ValueRef::Null => Ok(RawValue::Null),
        ValueRef::Integer(number) => Ok(if is_int {
            RawValue::Int(number)
        } else {
            RawValue::Text(number.to_string())
        }),
        ValueRef::Text(bytes) => {
            let text = std::str::from_utf8(bytes)
                .map_err(|_| SqlError::db("text value is not valid UTF-8"))?;
            if is_int {
                let number = text
                    .trim()
                    .parse::<i64>()
                    .map_err(|_| SqlError::db(format!("value '{text}' is not an integer")))?;
                Ok(RawValue::Int(number))
            } else {
                Ok(RawValue::Text(text.to_string()))
            }
        }
        ValueRef::Real(_) => Err(SqlError::db("REAL values are not supported")),
        ValueRef::Blob(_) => Err(SqlError::db("BLOB values are not supported")),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use abap_sql_ir::{
        CharType, OrderItem, Param, ResultMode, RowField, RowMode, RowSource, IR_VERSION,
    };

    use super::*;

    fn table_config(columns: &[(&str, &str)]) -> TableConfig {
        TableConfig {
            ddic: "ZFLUG".into(),
            connection: "DEFAULT".into(),
            target: "flug".into(),
            columns: columns
                .iter()
                .map(|(d, t)| (d.to_uppercase(), t.to_string()))
                .collect::<HashMap<_, _>>(),
            key: vec![],
        }
    }

    /// Tabelle `flug` mit fünf Zeilen. Eine Zeile enthält ein Anführungszeichen im Wert.
    fn database() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE flug (carrid TEXT, connid TEXT, seats INTEGER, fldate TEXT);
             INSERT INTO flug VALUES ('LH', '0400', 180, '20260101');
             INSERT INTO flug VALUES ('LH', '0401', 200, '20260102');
             INSERT INTO flug VALUES ('AF', '0010', 220, '20260103');
             INSERT INTO flug VALUES ('x'' OR ''1''=''1', '0001', 1, '20260104');",
        )
        .unwrap();
        conn
    }

    fn adapter() -> SqliteAdapter {
        SqliteAdapter::new(database(), vec![table_config(&[])]).unwrap()
    }

    fn char(length: u32, dtype: CharType) -> AbapType {
        AbapType::Char { length, dtype }
    }

    fn column(name: &str, source: AbapType) -> ResultColumn {
        ResultColumn {
            name: name.into(),
            target: source.clone(),
            source,
            offset: 0,
        }
    }

    fn standard_columns() -> Vec<ResultColumn> {
        vec![
            column("carrid", char(2, CharType::Char)),
            column("connid", char(4, CharType::Numc)),
            column("seats", AbapType::Int),
            column("fldate", char(8, CharType::Dats)),
        ]
    }

    fn request(
        filter: Option<Expr>,
        order_by: Vec<OrderItem>,
        up_to: Option<u64>,
        params: Vec<Param>,
    ) -> SqlRequest {
        SqlRequest {
            ir_version: IR_VERSION,
            line: 3,
            connection: "DEFAULT".into(),
            op: SqlOp::Select(SelectOp {
                table: "ZFLUG".into(),
                columns: standard_columns(),
                filter,
                order_by,
                up_to,
                result: ResultMode::Table,
                target_size: 0,
                target_fields: vec![],
                params,
            }),
        }
    }

    fn all() -> SqlRequest {
        request(None, vec![], None, vec![])
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

    fn int(value: i64) -> Expr {
        Expr::Literal {
            value: IrValue::Int { value },
        }
    }

    fn cmp(op: CmpOp, left: Expr, right: Expr) -> Expr {
        Expr::Compare {
            op,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    /// Führt die Anfrage aus und liefert die Zeilen als Text, Integer werden als `#n` dargestellt.
    fn run(adapter: &mut SqliteAdapter, req: &SqlRequest, params: &[ParamValue]) -> Vec<String> {
        adapter.check(req).expect("check");
        adapter
            .execute(req, params)
            .expect("execute")
            .rows
            .iter()
            .map(|row| {
                row.iter()
                    .map(|v| match v {
                        RawValue::Null => "NULL".to_string(),
                        RawValue::Int(i) => format!("#{i}"),
                        RawValue::Text(t) => t.clone(),
                    })
                    .collect::<Vec<_>>()
                    .join("|")
            })
            .collect()
    }

    fn carriers(
        adapter: &mut SqliteAdapter,
        req: &SqlRequest,
        params: &[ParamValue],
    ) -> Vec<String> {
        run(adapter, req, params)
            .into_iter()
            .map(|row| row.split('|').next().unwrap().to_string())
            .collect()
    }

    #[test]
    fn select_without_filter_returns_all_rows_in_projection_order() {
        let mut a = adapter();
        let rows = run(&mut a, &all(), &[]);
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0], "LH|0400|#180|20260101");
    }

    #[test]
    fn host_value_with_quote_is_data_and_does_not_change_the_query() {
        let mut a = adapter();
        let req = request(
            Some(cmp(CmpOp::Eq, col("connid"), Expr::Param { index: 0 })),
            vec![],
            None,
            vec![Param {
                name: "lv_connid".into(),
                ty: char(4, CharType::Char),
            }],
        );
        // Der Wert `' OR '1'='1` darf nicht als SQL wirken: ohne Treffer bleibt die Ergebnismenge leer.
        let rows = run(&mut a, &req, &[ParamValue::Text("' OR '1'='1".into())]);
        assert!(rows.is_empty(), "{rows:?}");
        // Die Anfrage enthält nur Platzhalter, nie den Wert.
        let built = a.build(&req).unwrap();
        assert!(!built.sql.contains("OR '1'"), "{}", built.sql);
        assert!(built.sql.contains("?1"), "{}", built.sql);
    }

    #[test]
    fn stored_value_with_quote_is_found_only_as_exact_data() {
        let mut a = adapter();
        let req = request(
            Some(cmp(CmpOp::Eq, col("carrid"), Expr::Param { index: 0 })),
            vec![],
            None,
            vec![Param {
                name: "lv_carrid".into(),
                ty: char(2, CharType::Char),
            }],
        );
        // Der gespeicherte Wert enthält Anführungszeichen. Der Vergleich ist exakt, nicht injiziert.
        let carriers_found = carriers(&mut a, &req, &[ParamValue::Text("x' OR '1'='1".into())]);
        assert_eq!(carriers_found, vec!["x' OR '1'='1".to_string()]);
    }

    #[test]
    fn character_parameter_trailing_blanks_are_removed_before_binding() {
        let mut a = adapter();
        let req = request(
            Some(cmp(CmpOp::Eq, col("carrid"), Expr::Param { index: 0 })),
            vec![],
            None,
            vec![Param {
                name: "lv_carrid".into(),
                ty: char(4, CharType::Char),
            }],
        );
        let carriers_found = carriers(&mut a, &req, &[ParamValue::Text("LH  ".into())]);
        assert_eq!(carriers_found.len(), 2);
    }

    #[test]
    fn unknown_table_is_rejected_by_check() {
        let a = adapter();
        let mut req = all();
        let SqlOp::Select(s) = &mut req.op else {
            unreachable!("select request")
        };
        s.table = "ZNOPE".into();
        let err = a.check(&req).unwrap_err();
        assert_eq!(err.kind, super::super::adapter::SqlErrorKind::Invalid);
        assert!(err.message.contains("ZNOPE"), "{}", err.message);
    }

    #[test]
    fn column_name_with_quote_is_rejected() {
        let a = adapter();
        let mut req = all();
        let SqlOp::Select(s) = &mut req.op else {
            unreachable!("select request")
        };
        s.columns[0].name = "carrid\"x".into();
        let err = a.check(&req).unwrap_err();
        assert_eq!(err.kind, super::super::adapter::SqlErrorKind::Invalid);
        assert!(
            err.message.contains("invalid column name"),
            "{}",
            err.message
        );
    }

    #[test]
    fn column_name_with_space_in_filter_is_rejected() {
        let a = adapter();
        let req = request(
            Some(cmp(CmpOp::Eq, col("car rid"), text("LH"))),
            vec![],
            None,
            vec![],
        );
        assert!(a.check(&req).is_err());
    }

    #[test]
    fn target_table_name_from_configuration_with_quote_is_rejected_by_builder() {
        let mut config = table_config(&[]);
        config.target = "flug\"; DROP".into();
        let req = all();
        let SqlOp::Select(select) = &req.op else {
            unreachable!("select request")
        };
        assert!(build_select(select, &config).is_err());
    }

    #[test]
    fn configured_column_names_are_used_in_the_query() {
        let mut a =
            SqliteAdapter::new(database(), vec![table_config(&[("carrid", "carrier")])]).unwrap();
        let built = a.build(&all()).unwrap();
        assert!(built.sql.contains("\"carrier\""), "{}", built.sql);
        assert!(
            built.sql.starts_with("SELECT \"carrier\", \"connid\""),
            "{}",
            built.sql
        );
        // Die Tabelle hat keine Spalte `carrier`; die Abfrage muss daher scheitern, nicht still leer laufen.
        let err = a.execute(&all(), &[]).unwrap_err();
        assert_eq!(err.kind, super::super::adapter::SqlErrorKind::Db);
        assert!(err.message.contains("carrier"), "{}", err.message);
    }

    #[test]
    fn double_quoted_unknown_name_is_an_error_on_the_adapter_connection() {
        // Ohne Abschaltung von DQS würde SQLite den Namen als Textliteral lesen und den Fehler verschleiern.
        let a = SqliteAdapter::new(database(), vec![]).unwrap();
        assert!(a.conn.prepare("SELECT \"nosuch\" FROM flug").is_err());
        assert!(a.conn.prepare("SELECT \"carrid\" FROM flug").is_ok());
    }

    #[test]
    fn limit_restricts_the_number_of_rows() {
        let mut a = adapter();
        let req = request(None, vec![], Some(2), vec![]);
        assert_eq!(run(&mut a, &req, &[]).len(), 2);
        assert!(a.build(&req).unwrap().sql.ends_with("LIMIT 2"));
    }

    #[test]
    fn order_by_descending_sorts_by_the_physical_column() {
        let mut a = adapter();
        let req = request(
            None,
            vec![OrderItem {
                column: "connid".into(),
                descending: true,
            }],
            None,
            vec![],
        );
        let rows = run(&mut a, &req, &[]);
        assert_eq!(rows[0], "LH|0401|#200|20260102");
        assert_eq!(rows[3], "x' OR '1'='1|0001|#1|20260104");
    }

    #[test]
    fn between_selects_inclusive_range() {
        let mut a = adapter();
        let req = request(
            Some(Expr::Between {
                value: Box::new(col("seats")),
                low: Box::new(int(180)),
                high: Box::new(int(200)),
            }),
            vec![],
            None,
            vec![],
        );
        assert_eq!(run(&mut a, &req, &[]).len(), 2);
    }

    #[test]
    fn in_list_matches_any_listed_value() {
        let mut a = adapter();
        let req = request(
            Some(Expr::In {
                value: Box::new(col("carrid")),
                list: vec![text("AF"), text("XX")],
            }),
            vec![],
            None,
            vec![],
        );
        assert_eq!(carriers(&mut a, &req, &[]), vec!["AF".to_string()]);
    }

    #[test]
    fn empty_in_list_is_invalid() {
        let a = adapter();
        let req = request(
            Some(Expr::In {
                value: Box::new(col("carrid")),
                list: vec![],
            }),
            vec![],
            None,
            vec![],
        );
        let err = a.check(&req).unwrap_err();
        assert_eq!(err.kind, super::super::adapter::SqlErrorKind::Invalid);
    }

    #[test]
    fn like_matches_pattern() {
        let mut a = adapter();
        let req = request(
            Some(Expr::Like {
                value: Box::new(col("carrid")),
                pattern: Box::new(text("L%")),
            }),
            vec![],
            None,
            vec![],
        );
        assert_eq!(run(&mut a, &req, &[]).len(), 2);
    }

    #[test]
    fn not_inverts_condition() {
        let mut a = adapter();
        let req = request(
            Some(Expr::Not {
                operand: Box::new(cmp(CmpOp::Eq, col("carrid"), text("LH"))),
            }),
            vec![],
            None,
            vec![],
        );
        assert_eq!(run(&mut a, &req, &[]).len(), 2);
    }

    #[test]
    fn or_matches_either_side() {
        let mut a = adapter();
        let req = request(
            Some(Expr::Or {
                left: Box::new(cmp(CmpOp::Eq, col("carrid"), text("AF"))),
                right: Box::new(cmp(CmpOp::Eq, col("seats"), int(180))),
            }),
            vec![],
            None,
            vec![],
        );
        assert_eq!(run(&mut a, &req, &[]).len(), 2);
    }

    #[test]
    fn not_equal_excludes_the_value() {
        let mut a = adapter();
        let req = request(
            Some(cmp(CmpOp::Ne, col("carrid"), text("LH"))),
            vec![],
            None,
            vec![],
        );
        assert_eq!(run(&mut a, &req, &[]).len(), 2);
    }

    #[test]
    fn less_equal_and_greater_equal_include_the_bound() {
        let mut a = adapter();
        let le = request(
            Some(cmp(CmpOp::Le, col("seats"), int(200))),
            vec![],
            None,
            vec![],
        );
        assert_eq!(run(&mut a, &le, &[]).len(), 3);
        let ge = request(
            Some(cmp(CmpOp::Ge, col("seats"), int(200))),
            vec![],
            None,
            vec![],
        );
        assert_eq!(run(&mut a, &ge, &[]).len(), 2);
    }

    #[test]
    fn date_stored_as_text_compares_as_text() {
        let mut a = adapter();
        let req = request(
            Some(cmp(CmpOp::Eq, col("fldate"), text("20260102"))),
            vec![],
            None,
            vec![],
        );
        assert_eq!(
            run(&mut a, &req, &[]),
            vec!["LH|0401|#200|20260102".to_string()]
        );
    }

    #[test]
    fn integer_column_is_returned_as_integer_and_text_column_as_text() {
        let mut a = adapter();
        let req = request(
            Some(cmp(CmpOp::Eq, col("connid"), text("0010"))),
            vec![],
            None,
            vec![],
        );
        let rows = a.execute(&req, &[]).unwrap().rows;
        assert_eq!(rows[0][2], RawValue::Int(220));
        assert_eq!(rows[0][0], RawValue::Text("AF".into()));
    }

    #[test]
    fn integer_parameter_is_bound_as_integer() {
        let mut a = adapter();
        let req = request(
            Some(cmp(CmpOp::Gt, col("seats"), Expr::Param { index: 0 })),
            vec![],
            None,
            vec![Param {
                name: "lv_min".into(),
                ty: AbapType::Int,
            }],
        );
        assert_eq!(run(&mut a, &req, &[ParamValue::Int(199)]).len(), 2);
    }

    #[test]
    fn missing_host_variable_value_is_invalid() {
        let mut a = adapter();
        let req = request(
            Some(cmp(CmpOp::Eq, col("carrid"), Expr::Param { index: 0 })),
            vec![],
            None,
            vec![],
        );
        let err = a.execute(&req, &[]).unwrap_err();
        assert_eq!(err.kind, super::super::adapter::SqlErrorKind::Invalid);
    }

    #[test]
    fn cursor_buffers_rows_and_returns_none_at_the_end() {
        let mut a = adapter();
        let id = a.open_cursor(&all(), &[]).unwrap();
        let mut count = 0;
        while a.fetch(id).unwrap().is_some() {
            count += 1;
        }
        assert_eq!(count, 4);
        a.close_cursor(id);
        assert!(a.fetch(id).is_err());
    }

    #[test]
    fn transaction_calls_succeed_on_autocommit_connection() {
        let mut a = adapter();
        a.begin().unwrap();
        a.rollback().unwrap();
        a.begin().unwrap();
        a.commit().unwrap();
    }

    #[test]
    fn capabilities_match_the_specification() {
        let a = adapter();
        let caps = a.capabilities();
        assert!(caps.select && caps.where_clause && caps.like && caps.limit && caps.single);
        assert!(!caps.concurrent_cursors);
    }

    #[test]
    fn missing_database_file_is_an_error_and_is_not_created() {
        let path = std::env::temp_dir().join(format!(
            "abap-rt-missing-{}-{}.sqlite",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let err = SqliteAdapter::open_file(&path, vec![])
            .err()
            .expect("error");
        assert!(
            err.message.contains("database file not found"),
            "{}",
            err.message
        );
        assert!(!path.exists());
    }

    /// Tabelle `flug` mit zusammengesetztem Schlüssel (carrid, connid) für die Schreibtests.
    fn write_database() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE flug (carrid TEXT, connid TEXT, seats INTEGER, fldate TEXT, PRIMARY KEY (carrid, connid));
             INSERT INTO flug VALUES ('LH', '0400', 180, '20260101');
             INSERT INTO flug VALUES ('AF', '0010', 220, '20260103');
             INSERT INTO flug VALUES ('LH', '0010', 90, '20260102');",
        )
        .unwrap();
        conn
    }

    fn keyed_table(key: &[&str]) -> TableConfig {
        TableConfig {
            ddic: "ZFLUG".into(),
            connection: "DEFAULT".into(),
            target: "flug".into(),
            columns: HashMap::new(),
            key: key.iter().map(|k| k.to_string()).collect(),
        }
    }

    fn write_adapter() -> SqliteAdapter {
        SqliteAdapter::new(write_database(), vec![keyed_table(&["CARRID", "CONNID"])]).unwrap()
    }

    /// Zeilenquelle `carrid`, `connid` (NUMC 4), `seats` (INT), `fldate` (DATS). Die Werte stehen in dieser
    /// Reihenfolge als Parameter (ohne Hostparameter, also Parameter 0 bis 3).
    fn flug_source(mode: RowMode) -> RowSource {
        RowSource {
            mode,
            fields: vec![
                RowField {
                    column: "carrid".into(),
                    ty: char(2, CharType::Char),
                    offset: 0,
                },
                RowField {
                    column: "connid".into(),
                    ty: char(4, CharType::Numc),
                    offset: 4,
                },
                RowField {
                    column: "seats".into(),
                    ty: AbapType::Int,
                    offset: 12,
                },
                RowField {
                    column: "fldate".into(),
                    ty: char(8, CharType::Dats),
                    offset: 16,
                },
            ],
            size: 32,
        }
    }

    fn flug_values(carrid: &str, connid: &str, seats: i64, fldate: &str) -> Vec<ParamValue> {
        vec![
            ParamValue::Text(carrid.into()),
            ParamValue::Text(connid.into()),
            ParamValue::Int(seats),
            ParamValue::Text(fldate.into()),
        ]
    }

    fn dml(op: SqlOp) -> SqlRequest {
        SqlRequest {
            ir_version: IR_VERSION,
            line: 3,
            connection: "DEFAULT".into(),
            op,
        }
    }

    fn count(adapter: &SqliteAdapter, sql: &str) -> i64 {
        adapter.conn.query_row(sql, [], |row| row.get(0)).unwrap()
    }

    fn lh_seats(adapter: &SqliteAdapter) -> i64 {
        count(
            adapter,
            "SELECT seats FROM flug WHERE carrid = 'LH' AND connid = '0400'",
        )
    }

    fn insert_request(accepting: bool) -> SqlRequest {
        dml(SqlOp::Insert(abap_sql_ir::InsertOp {
            table: "ZFLUG".into(),
            rows: flug_source(RowMode::Single),
            accepting_duplicate_keys: accepting,
        }))
    }

    #[test]
    fn insert_binds_row_values_as_parameters_and_stores_them() {
        let mut a = write_adapter();
        let req = insert_request(false);
        let outcome = a
            .execute(&req, &flug_values("XX", "0001", 7, "20260104"))
            .unwrap();
        assert_eq!(outcome.dbcnt, 1);
        assert!(outcome.rows.is_empty());
        let stored = count(
            &a,
            "SELECT seats FROM flug WHERE carrid = 'XX' AND connid = '0001' AND fldate = '20260104'",
        );
        assert_eq!(stored, 7);
    }

    #[test]
    fn host_value_with_quote_in_insert_is_stored_as_data_and_does_not_change_the_statement() {
        let mut a = write_adapter();
        let req = insert_request(false);
        let hostile = "x' OR '1'='1";
        a.execute(&req, &flug_values(hostile, "0001", 1, "20260104"))
            .unwrap();
        // Genau eine neue Zeile, der Wert steht unverändert in der Spalte.
        assert_eq!(count(&a, "SELECT COUNT(*) FROM flug"), 4);
        assert_eq!(
            count(
                &a,
                "SELECT COUNT(*) FROM flug WHERE carrid = 'x'' OR ''1''=''1'"
            ),
            1
        );
        assert!(!a.statement_text(&req).unwrap().contains("OR '1'"));
    }

    #[test]
    fn statement_text_has_placeholders_only() {
        let a = write_adapter();
        let text = a.statement_text(&insert_request(false)).unwrap();
        assert!(
            text.starts_with(
                "INSERT INTO \"flug\" (\"carrid\", \"connid\", \"seats\", \"fldate\")"
            ),
            "{text}"
        );
        assert!(text.ends_with("VALUES (?1, ?2, ?3, ?4)"), "{text}");
    }

    #[test]
    fn update_set_binds_assignment_and_where_host_variables_and_counts_rows() {
        use abap_sql_ir::{Assignment, CmpOp, Expr, UpdateOp};
        let mut a = write_adapter();
        let req = dml(SqlOp::Update(UpdateOp {
            table: "ZFLUG".into(),
            assignments: vec![Assignment {
                column: "seats".into(),
                value: Expr::Param { index: 0 },
            }],
            filter: Some(Expr::Compare {
                op: CmpOp::Eq,
                left: Box::new(col("carrid")),
                right: Box::new(Expr::Param { index: 1 }),
            }),
            rows: None,
            params: vec![
                Param {
                    name: "lv_seats".into(),
                    ty: AbapType::Int,
                },
                Param {
                    name: "lv_carrid".into(),
                    ty: char(2, CharType::Char),
                },
            ],
        }));
        let outcome = a
            .execute(&req, &[ParamValue::Int(300), ParamValue::Text("LH".into())])
            .unwrap();
        assert_eq!(outcome.dbcnt, 2, "both LH rows change");
        assert_eq!(count(&a, "SELECT COUNT(*) FROM flug WHERE seats = 300"), 2);
    }

    #[test]
    fn update_set_without_matching_row_has_dbcnt_zero() {
        use abap_sql_ir::{Assignment, CmpOp, Expr, UpdateOp};
        let mut a = write_adapter();
        let req = dml(SqlOp::Update(UpdateOp {
            table: "ZFLUG".into(),
            assignments: vec![Assignment {
                column: "seats".into(),
                value: Expr::Literal {
                    value: IrValue::Int { value: 1 },
                },
            }],
            filter: Some(Expr::Compare {
                op: CmpOp::Eq,
                left: Box::new(col("carrid")),
                right: Box::new(text("ZZ")),
            }),
            rows: None,
            params: vec![],
        }));
        assert_eq!(a.execute(&req, &[]).unwrap().dbcnt, 0);
    }

    #[test]
    fn update_from_row_changes_only_non_key_columns_and_matches_by_configured_key() {
        use abap_sql_ir::UpdateOp;
        let mut a = write_adapter();
        let req = dml(SqlOp::Update(UpdateOp {
            table: "ZFLUG".into(),
            assignments: vec![],
            filter: None,
            rows: Some(flug_source(RowMode::Single)),
            params: vec![],
        }));
        let text = a.statement_text(&req).unwrap();
        // Die Platzhalter sind in Textreihenfolge nummeriert: erst SET, dann WHERE.
        assert_eq!(
            text,
            "UPDATE \"flug\" SET \"seats\" = ?1, \"fldate\" = ?2 WHERE \"carrid\" = ?3 AND \"connid\" = ?4"
        );
        let outcome = a
            .execute(&req, &flug_values("LH", "0400", 999, "20260111"))
            .unwrap();
        assert_eq!(outcome.dbcnt, 1);
        assert_eq!(lh_seats(&a), 999);
        let missing = a
            .execute(&req, &flug_values("ZZ", "0400", 1, "20260111"))
            .unwrap();
        assert_eq!(missing.dbcnt, 0);
    }

    #[test]
    fn update_from_row_without_key_in_configuration_is_an_error() {
        use abap_sql_ir::UpdateOp;
        let mut a = SqliteAdapter::new(write_database(), vec![keyed_table(&[])]).unwrap();
        let req = dml(SqlOp::Update(UpdateOp {
            table: "ZFLUG".into(),
            assignments: vec![],
            filter: None,
            rows: Some(flug_source(RowMode::Single)),
            params: vec![],
        }));
        let err = a
            .execute(&req, &flug_values("LH", "0400", 1, "20260111"))
            .unwrap_err();
        assert_eq!(err.kind, super::super::adapter::SqlErrorKind::Invalid);
        assert_eq!(err.message, "table ZFLUG has no key in configuration");
    }

    #[test]
    fn update_set_that_creates_a_duplicate_key_is_reported_as_duplicate() {
        use abap_sql_ir::{Assignment, CmpOp, Expr, UpdateOp};
        let mut a = write_adapter();
        // Setzt carrid auf LH für die Zeile mit connid 0010 (AF). Dann gibt es LH/0010 schon.
        let req = dml(SqlOp::Update(UpdateOp {
            table: "ZFLUG".into(),
            assignments: vec![Assignment {
                column: "carrid".into(),
                value: text("LH"),
            }],
            filter: Some(Expr::Compare {
                op: CmpOp::Eq,
                left: Box::new(col("connid")),
                right: Box::new(text("0010")),
            }),
            rows: None,
            params: vec![],
        }));
        let err = a.execute(&req, &[]).unwrap_err();
        assert_eq!(err.kind, super::super::adapter::SqlErrorKind::Duplicate);
        // Die Anweisung ist atomar: AF/0010 bleibt unverändert.
        assert_eq!(
            count(&a, "SELECT COUNT(*) FROM flug WHERE carrid = 'AF'"),
            1
        );
    }

    #[test]
    fn duplicate_single_insert_is_reported_as_duplicate_kind() {
        let mut a = write_adapter();
        let req = insert_request(false);
        let err = a
            .execute(&req, &flug_values("LH", "0400", 1, "20260111"))
            .unwrap_err();
        assert_eq!(err.kind, super::super::adapter::SqlErrorKind::Duplicate);
        assert_eq!(lh_seats(&a), 180, "the existing row is unchanged");
    }

    #[test]
    fn check_constraint_violation_stays_a_database_error_and_is_not_a_duplicate() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE flug (carrid TEXT, connid TEXT, seats INTEGER CHECK (seats > 0), fldate TEXT, PRIMARY KEY (carrid, connid));")
            .unwrap();
        let mut a = SqliteAdapter::new(conn, vec![keyed_table(&["CARRID", "CONNID"])]).unwrap();
        let req = insert_request(false);
        let err = a
            .execute(&req, &flug_values("XX", "0001", 0, "20260101"))
            .unwrap_err();
        assert_eq!(err.kind, super::super::adapter::SqlErrorKind::Db);
        assert!(err.message.contains("CHECK"), "{}", err.message);
    }

    #[test]
    fn delete_without_where_removes_every_row() {
        use abap_sql_ir::DeleteOp;
        let mut a = write_adapter();
        let req = dml(SqlOp::Delete(DeleteOp {
            table: "ZFLUG".into(),
            filter: None,
            rows: None,
            params: vec![],
        }));
        assert_eq!(a.execute(&req, &[]).unwrap().dbcnt, 3);
        assert_eq!(count(&a, "SELECT COUNT(*) FROM flug"), 0);
    }

    #[test]
    fn delete_with_where_counts_only_the_matching_rows() {
        use abap_sql_ir::{CmpOp, DeleteOp, Expr};
        let mut a = write_adapter();
        let req = dml(SqlOp::Delete(DeleteOp {
            table: "ZFLUG".into(),
            filter: Some(Expr::Compare {
                op: CmpOp::Eq,
                left: Box::new(col("carrid")),
                right: Box::new(Expr::Param { index: 0 }),
            }),
            rows: None,
            params: vec![Param {
                name: "lv_carrid".into(),
                ty: char(2, CharType::Char),
            }],
        }));
        assert_eq!(
            a.execute(&req, &[ParamValue::Text("LH".into())])
                .unwrap()
                .dbcnt,
            2
        );
        assert_eq!(count(&a, "SELECT COUNT(*) FROM flug"), 1);
    }

    #[test]
    fn delete_by_key_removes_exactly_the_row_with_that_key() {
        use abap_sql_ir::DeleteOp;
        let mut a = write_adapter();
        let req = dml(SqlOp::Delete(DeleteOp {
            table: "ZFLUG".into(),
            filter: None,
            rows: Some(flug_source(RowMode::Single)),
            params: vec![],
        }));
        let outcome = a
            .execute(&req, &flug_values("LH", "0400", 0, "20260101"))
            .unwrap();
        assert_eq!(outcome.dbcnt, 1);
        assert_eq!(count(&a, "SELECT COUNT(*) FROM flug"), 2);
        assert_eq!(
            count(
                &a,
                "SELECT COUNT(*) FROM flug WHERE carrid = 'LH' AND connid = '0400'"
            ),
            0
        );
    }

    #[test]
    fn modify_inserts_a_missing_row_and_updates_an_existing_one() {
        let mut a = write_adapter();
        let req = dml(SqlOp::Modify(abap_sql_ir::ModifyOp {
            table: "ZFLUG".into(),
            rows: flug_source(RowMode::Single),
        }));
        assert_eq!(
            a.execute(&req, &flug_values("XX", "0001", 5, "20260104"))
                .unwrap()
                .dbcnt,
            1
        );
        assert_eq!(count(&a, "SELECT COUNT(*) FROM flug"), 4);
        assert_eq!(
            a.execute(&req, &flug_values("LH", "0400", 777, "20260101"))
                .unwrap()
                .dbcnt,
            1
        );
        assert_eq!(count(&a, "SELECT COUNT(*) FROM flug"), 4);
        assert_eq!(lh_seats(&a), 777);
    }

    #[test]
    fn modify_with_only_key_columns_still_counts_the_row() {
        use abap_sql_ir::ModifyOp;
        let mut a = write_adapter();
        let source = RowSource {
            mode: RowMode::Single,
            fields: vec![
                RowField {
                    column: "carrid".into(),
                    ty: char(2, CharType::Char),
                    offset: 0,
                },
                RowField {
                    column: "connid".into(),
                    ty: char(4, CharType::Numc),
                    offset: 4,
                },
            ],
            size: 12,
        };
        let req = dml(SqlOp::Modify(ModifyOp {
            table: "ZFLUG".into(),
            rows: source,
        }));
        let params = vec![
            ParamValue::Text("LH".into()),
            ParamValue::Text("0400".into()),
        ];
        assert_eq!(a.execute(&req, &params).unwrap().dbcnt, 1);
    }

    #[test]
    fn modify_without_key_in_configuration_is_an_error() {
        use abap_sql_ir::ModifyOp;
        let a = SqliteAdapter::new(write_database(), vec![keyed_table(&[])]).unwrap();
        let req = dml(SqlOp::Modify(ModifyOp {
            table: "ZFLUG".into(),
            rows: flug_source(RowMode::Single),
        }));
        let err = a.check(&req).unwrap_err();
        assert_eq!(err.message, "table ZFLUG has no key in configuration");
    }

    #[test]
    fn modify_row_without_a_key_column_is_an_error() {
        use abap_sql_ir::ModifyOp;
        let a = write_adapter();
        let mut source = flug_source(RowMode::Single);
        source.fields.retain(|f| f.column != "connid");
        let req = dml(SqlOp::Modify(ModifyOp {
            table: "ZFLUG".into(),
            rows: source,
        }));
        let err = a.check(&req).unwrap_err();
        assert_eq!(
            err.message,
            "key column CONNID is missing in the row source"
        );
    }

    #[test]
    fn like_is_case_sensitive() {
        let mut a = adapter();
        let req = request(
            Some(Expr::Like {
                value: Box::new(col("carrid")),
                pattern: Box::new(text("lh%")),
            }),
            vec![],
            None,
            vec![],
        );
        assert!(run(&mut a, &req, &[]).is_empty());
        let upper = request(
            Some(Expr::Like {
                value: Box::new(col("carrid")),
                pattern: Box::new(text("LH%")),
            }),
            vec![],
            None,
            vec![],
        );
        assert_eq!(run(&mut a, &upper, &[]).len(), 2);
    }

    /// Tabelle mit Werten, die die Metazeichen von `GLOB` und `LIKE` enthalten.
    fn metachar_adapter() -> SqliteAdapter {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE flug (carrid TEXT, connid TEXT, seats INTEGER, fldate TEXT);
             INSERT INTO flug VALUES ('a*b', '0001', 1, '20260101');
             INSERT INTO flug VALUES ('axb', '0002', 2, '20260101');
             INSERT INTO flug VALUES ('a?b', '0003', 3, '20260101');
             INSERT INTO flug VALUES ('[x', '0004', 4, '20260101');
             INSERT INTO flug VALUES ('a%b', '0005', 5, '20260101');",
        )
        .unwrap();
        SqliteAdapter::new(conn, vec![table_config(&[])]).unwrap()
    }

    fn carriers_matching(a: &mut SqliteAdapter, pattern: &str) -> Vec<String> {
        let req = request(
            Some(Expr::Like {
                value: Box::new(col("carrid")),
                pattern: Box::new(text(pattern)),
            }),
            vec![OrderItem {
                column: "connid".into(),
                descending: false,
            }],
            None,
            vec![],
        );
        carriers(a, &req, &[])
    }

    #[test]
    fn glob_metacharacters_in_a_like_pattern_are_matched_literally() {
        let mut a = metachar_adapter();
        // `*` im Muster ist ein Zeichen, nicht die Platzhalterfolge.
        assert_eq!(carriers_matching(&mut a, "a*b"), vec!["a*b".to_string()]);
        // `?` im Muster ist ein Zeichen.
        assert_eq!(carriers_matching(&mut a, "a?b"), vec!["a?b".to_string()]);
        // `[` im Muster öffnet keine Zeichenklasse.
        assert_eq!(carriers_matching(&mut a, "[x"), vec!["[x".to_string()]);
    }

    #[test]
    fn like_wildcards_are_still_wildcards_and_percent_matches_any_run() {
        let mut a = metachar_adapter();
        // `_` ist genau ein Zeichen: a_b passt auf axb, a*b, a?b, a%b.
        assert_eq!(carriers_matching(&mut a, "a_b").len(), 4);
        // Ein Muster aus `%` passt auf alles.
        assert_eq!(carriers_matching(&mut a, "%").len(), 5);
        // `%` ohne Ende passt auch auf den leeren Rest.
        assert_eq!(carriers_matching(&mut a, "a%").len(), 4);
    }

    #[test]
    fn like_pattern_from_a_host_variable_has_the_same_rules_as_a_literal() {
        let mut a = metachar_adapter();
        let req = request(
            Some(Expr::Like {
                value: Box::new(col("carrid")),
                pattern: Box::new(Expr::Param { index: 0 }),
            }),
            vec![],
            None,
            vec![Param {
                name: "lv_pattern".into(),
                ty: char(3, CharType::Char),
            }],
        );
        // `a*b` als Hostwert: nur der Wert a*b, nicht axb.
        let rows = carriers(&mut a, &req, &[ParamValue::Text("a*b".into())]);
        assert_eq!(rows, vec!["a*b".to_string()]);
        // Ein nachgestelltes Leerzeichen im Hostwert wird wie bei den Vergleichen entfernt.
        let spaced = carriers(&mut a, &req, &[ParamValue::Text("a*b  ".into())]);
        assert_eq!(spaced, vec!["a*b".to_string()]);
    }

    #[test]
    fn like_to_glob_escapes_glob_characters_and_maps_like_wildcards() {
        assert_eq!(like_to_glob("L%"), "L*");
        assert_eq!(like_to_glob("a_b"), "a?b");
        assert_eq!(like_to_glob("*?["), "[*][?][[]");
        assert_eq!(like_to_glob(""), "");
    }

    #[test]
    fn commit_keeps_the_change_and_rollback_discards_it() {
        let path = std::env::temp_dir().join(format!("abap-rt-luw-{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE flug (carrid TEXT, connid TEXT, seats INTEGER, fldate TEXT, PRIMARY KEY (carrid, connid));
                 INSERT INTO flug VALUES ('LH', '0400', 180, '20260101');",
            )
            .unwrap();
        }
        let mut a =
            SqliteAdapter::open_file(&path, vec![keyed_table(&["CARRID", "CONNID"])]).unwrap();
        let req = insert_request(false);
        a.begin().unwrap();
        a.execute(&req, &flug_values("XX", "0001", 1, "20260104"))
            .unwrap();
        a.rollback().unwrap();
        assert_eq!(count(&a, "SELECT COUNT(*) FROM flug"), 1);
        a.begin().unwrap();
        a.execute(&req, &flug_values("XX", "0001", 1, "20260104"))
            .unwrap();
        a.commit().unwrap();
        // Eine zweite Verbindung sieht die festgeschriebene Zeile.
        let other = Connection::open(&path).unwrap();
        let seen: i64 = other
            .query_row("SELECT COUNT(*) FROM flug", [], |row| row.get(0))
            .unwrap();
        assert_eq!(seen, 2);
        drop(a);
        drop(other);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn build_statement_has_no_sql_text_for_commit_or_rollback() {
        let err = build_statement(&dml(SqlOp::Commit), &keyed_table(&[])).unwrap_err();
        assert_eq!(err.kind, super::super::adapter::SqlErrorKind::Invalid);
    }

    #[test]
    fn existing_database_file_is_read() {
        let path = std::env::temp_dir().join(format!("abap-rt-read-{}.sqlite", std::process::id()));
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "DROP TABLE IF EXISTS flug;
                 CREATE TABLE flug (carrid TEXT, connid TEXT, seats INTEGER, fldate TEXT);
                 INSERT INTO flug VALUES ('LH', '0400', 180, '20260101');",
            )
            .unwrap();
        }
        let mut a = SqliteAdapter::open_file(&path, vec![table_config(&[])]).unwrap();
        assert_eq!(run(&mut a, &all(), &[]).len(), 1);
        drop(a);
        let _ = std::fs::remove_file(&path);
    }
}
