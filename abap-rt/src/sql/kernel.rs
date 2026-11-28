//! Kern der SQL-Laufzeit: Auflösung der Tabelle, Aufruf des Adapters, Übertragung in den Zielbereich.
//!
//! Die Funktionen sind ohne Prozessende geschrieben und geben `Result<_, String>` zurück. Die Meldung
//! ist fertig formuliert (englisch, mit Zeile und Verbindung). Die C-ABI-Schicht in `sql/mod.rs` beendet
//! das Programm damit.

use std::time::Duration;

use abap_sql_ir::{
    Expr, Param, ResultColumn, ResultMode, RowField, RowMode, RowSource, SqlOp, SqlRequest,
};

use super::adapter::{
    missing_capabilities, operation_name, CursorId, Outcome, ParamValue, RawValue, SqlAdapter,
    SqlError, SqlErrorKind,
};
use super::config::{AdapterKind, Config, ConnectionConfig, TableConfig};
use super::external::ExternalAdapter;
use super::sqlite::SqliteAdapter;
use super::value::{field_size, row_value, write_field, ConvError};
use crate::RtTable;

/// Umgebungsvariable, die die Protokollierung der SQL-Anweisungen auf stderr einschaltet. Ist sie gesetzt
/// und nicht leer, schreibt die Laufzeit je Anweisung eine Zeile. Der Compiler hat dafür keinen Schalter.
pub const TRACE_ENV: &str = "ABAPC_TRACE_SQL";

/// Ort einer Anweisung für Meldungen: Zeile, Verbindung (wie konfiguriert) und DDIC-Tabelle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Site {
    pub line: u32,
    pub connection: String,
    pub table: String,
}

/// Zielbereich einer Anweisung.
pub enum Target<'a> {
    /// Arbeitsbereich oder Feld, genau `target_size` Bytes.
    Work(&'a mut [u8]),
    /// Interne Tabelle (`*mut RtTable`) für `INTO TABLE` und `APPENDING TABLE`.
    Table(*mut RtTable),
}

/// Ergebnis einer Anweisung: Wert für `SY-SUBRC` und `SY-DBCNT`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Summary {
    pub subrc: i32,
    pub dbcnt: i64,
}

/// Schlüssel der Adapter-Tabelle: Verbindungsnamen sind ohne Unterschied der Groß- und Kleinschreibung.
pub fn connection_key(name: &str) -> String {
    name.to_uppercase()
}

/// Zu einer DDIC-Tabelle die Tabellen- und Verbindungskonfiguration suchen.
pub fn resolve<'a>(
    config: &'a Config,
    table: &str,
    line: u32,
) -> Result<(&'a TableConfig, &'a ConnectionConfig), String> {
    let table_config = config
        .table(table)
        .ok_or_else(|| format!("table {table} is not configured (line {line})"))?;
    let connection = config.connection(&table_config.connection).ok_or_else(|| {
        format!(
            "connection {} is not defined in the configuration (table {table}, line {line})",
            table_config.connection
        )
    })?;
    Ok((table_config, connection))
}

/// Legt einen Adapter für die Verbindung an. Phase 1: SQLite und extern. Die anderen Arten sind
/// in der Konfiguration lesbar, aber nicht implementiert.
pub fn open_adapter(
    config: &Config,
    connection: &ConnectionConfig,
    site: &Site,
) -> Result<Box<dyn SqlAdapter>, String> {
    match connection.kind {
        AdapterKind::Sqlite => {
            let path = connection
                .path
                .as_deref()
                .ok_or_else(|| format!("connection {} has no database file", connection.name))?;
            let tables = config.tables_of(&connection.name);
            SqliteAdapter::open_file(path, tables)
                .map(|a| Box::new(a) as Box<dyn SqlAdapter>)
                .map_err(|e| format_sql_error(&e, site))
        }
        AdapterKind::External => {
            let command = connection
                .command
                .as_deref()
                .ok_or_else(|| format!("connection {} has no command", connection.name))?;
            ExternalAdapter::start(
                &connection.name,
                command,
                &connection.args,
                Duration::from_millis(connection.timeout_ms),
            )
            .map(|a| Box::new(a) as Box<dyn SqlAdapter>)
            .map_err(|e| format_sql_error(&e, site))
        }
        AdapterKind::Postgres | AdapterKind::Odata => Err(format!(
            "adapter not available for connection {}: {}",
            connection.name,
            connection.kind.as_str()
        )),
    }
}

/// Meldung für einen Adapterfehler, mit Zeile und Verbindung.
pub fn format_sql_error(error: &SqlError, site: &Site) -> String {
    let line = site.line;
    match error.kind {
        SqlErrorKind::Db | SqlErrorKind::Duplicate => format!(
            "db error at line {line} (connection {}, table {}): {}",
            site.connection, site.table, error.message
        ),
        SqlErrorKind::Capability => format!(
            "capability missing at line {line} (connection {}): {}",
            site.connection, error.message
        ),
        SqlErrorKind::Protocol => format!(
            "adapter protocol error at line {line} (connection {}): {}",
            site.connection, error.message
        ),
        SqlErrorKind::Invalid => format!("invalid SQL IR at line {line}: {}", error.message),
        SqlErrorKind::Process => match &error.detail {
            Some(detail) => format!("{} at line {line}: {detail}", error.message),
            None => format!("{} at line {line}", error.message),
        },
    }
}

fn invalid(site: &Site, reason: impl AsRef<str>) -> String {
    format!("invalid SQL IR at line {}: {}", site.line, reason.as_ref())
}

/// Prüft die Hostvariablen-Indizes der Filter und Zuweisungen: jede `Expr::Param` muss existieren.
/// `param_count` ist die Zahl der Hostparameter der Anweisung (ohne die Werte der Zeilenquelle).
fn validate(site: &Site, request: &SqlRequest, param_count: usize) -> Result<(), String> {
    fn check_expr(expr: &Expr, count: usize) -> Result<(), u32> {
        match expr {
            Expr::Param { index } => {
                if (*index as usize) < count {
                    Ok(())
                } else {
                    Err(*index)
                }
            }
            Expr::Column { .. } | Expr::Literal { .. } => Ok(()),
            Expr::Compare { left, right, .. }
            | Expr::And { left, right }
            | Expr::Or { left, right } => {
                check_expr(left, count)?;
                check_expr(right, count)
            }
            Expr::Between { value, low, high } => {
                check_expr(value, count)?;
                check_expr(low, count)?;
                check_expr(high, count)
            }
            Expr::In { value, list } => {
                check_expr(value, count)?;
                list.iter().try_for_each(|e| check_expr(e, count))
            }
            Expr::Like { value, pattern } => {
                check_expr(value, count)?;
                check_expr(pattern, count)
            }
            Expr::Not { operand } => check_expr(operand, count),
        }
    }
    let (filter, values): (Option<&Expr>, Vec<&Expr>) = match &request.op {
        SqlOp::Select(op) => (op.filter.as_ref(), Vec::new()),
        SqlOp::Update(op) => (
            op.filter.as_ref(),
            op.assignments.iter().map(|a| &a.value).collect(),
        ),
        SqlOp::Delete(op) => (op.filter.as_ref(), Vec::new()),
        SqlOp::Insert(_) | SqlOp::Modify(_) | SqlOp::Commit | SqlOp::Rollback => (None, Vec::new()),
    };
    for expr in filter.into_iter().chain(values) {
        check_expr(expr, param_count).map_err(|index| {
            invalid(
                site,
                format!("host variable index {index} is out of range ({param_count} given)"),
            )
        })?;
    }
    Ok(())
}

/// Gemeinsame Vorarbeit vor jeder Ausführung: Prüfung der Hostvariablen, Fähigkeiten (zuerst `SELECT`, dann alle
/// übrigen Fähigkeiten der Anweisung), Protokollierung und `check` des Adapters.
fn prepare(
    adapter: &dyn SqlAdapter,
    site: &Site,
    request: &SqlRequest,
    host: &[ParamValue],
) -> Result<(), String> {
    validate(site, request, host.len())?;
    if matches!(request.op, SqlOp::Select(_)) && !adapter.capabilities().select {
        return Err(format_sql_error(
            &SqlError::capability("SELECT not supported by this adapter"),
            site,
        ));
    }
    let missing = missing_capabilities(adapter.capabilities(), request);
    if !missing.is_empty() {
        return Err(format_sql_error(
            &SqlError::capability(format!(
                "not supported by this adapter: {}",
                missing.join(", ")
            )),
            site,
        ));
    }
    adapter
        .check(request)
        .map_err(|e| format_sql_error(&e, site))
}

/// Ob die Protokollierung eingeschaltet ist: die Umgebungsvariable `ABAPC_TRACE_SQL` ist gesetzt und nicht leer.
pub fn trace_enabled() -> bool {
    std::env::var_os(TRACE_ENV).is_some_and(|value| !value.is_empty())
}

/// Eine Protokollzeile: Verbindung, Operation, Tabelle, Namen der Werte und bei Bedarf der SQL-Text.
/// Feldwerte stehen nie in der Zeile.
pub fn trace_line(
    connection: &str,
    op: &str,
    table: &str,
    names: &[String],
    sql: Option<&str>,
) -> String {
    let table = if table.is_empty() { "-" } else { table };
    let names = if names.is_empty() {
        "-".to_string()
    } else {
        names.join(",")
    };
    let mut line = format!("[sql] connection={connection} op={op} table={table} params={names}");
    if let Some(sql) = sql {
        line.push_str(" sql=");
        line.push_str(sql);
    }
    line
}

/// Protokolliert eine Anweisung, wenn die Protokollierung eingeschaltet ist.
fn trace_statement(adapter: &dyn SqlAdapter, site: &Site, request: &SqlRequest) {
    if !trace_enabled() {
        return;
    }
    let sql = adapter.statement_text(request);
    eprintln!(
        "{}",
        trace_line(
            &site.connection,
            operation_name(request),
            &site.table,
            &value_names(request),
            sql.as_deref(),
        )
    );
}

/// Protokolliert eine Operation auf der Verbindung (`COMMIT WORK`, `ROLLBACK WORK`), wenn die Protokollierung
/// eingeschaltet ist.
pub fn trace_luw(connection: &str, op: &str) {
    if trace_enabled() {
        eprintln!("{}", trace_line(connection, op, "", &[], None));
    }
}

/// Namen der Werte einer Anweisung: die Hostvariablen, danach die Spalten der Zeilenquelle.
fn value_names(request: &SqlRequest) -> Vec<String> {
    let host: &[Param] = match &request.op {
        SqlOp::Select(op) => &op.params,
        SqlOp::Update(op) => &op.params,
        SqlOp::Delete(op) => &op.params,
        SqlOp::Insert(_) | SqlOp::Modify(_) | SqlOp::Commit | SqlOp::Rollback => &[],
    };
    let mut names: Vec<String> = host.iter().map(|p| p.name.clone()).collect();
    let source: Option<&RowSource> = match &request.op {
        SqlOp::Insert(op) => Some(&op.rows),
        SqlOp::Modify(op) => Some(&op.rows),
        SqlOp::Update(op) => op.rows.as_ref(),
        SqlOp::Delete(op) => op.rows.as_ref(),
        SqlOp::Select(_) | SqlOp::Commit | SqlOp::Rollback => None,
    };
    if let Some(source) = source {
        names.extend(source.fields.iter().map(|f| f.column.clone()));
    }
    names
}

/// Zeilenpuffer der Größe `size` mit den Initialwerten der Zielkomponenten. Zeichen sind blank, Zahlen 0,
/// `NUMC` aus Nullen, gepackte Zahlen 0 mit positivem Vorzeichen. Spalten überschreiben diese Werte danach;
/// nicht zugeordnete Komponenten bleiben initial (`INTO CORRESPONDING FIELDS OF TABLE`).
pub fn initial_row(site: &Site, fields: &[RowField], size: usize) -> Result<Vec<u8>, String> {
    let mut buffer = vec![0u8; size];
    for field in fields {
        let start = usize::try_from(field.offset)
            .map_err(|_| invalid(site, format!("offset of field {} too large", field.column)))?;
        let end = start
            .checked_add(field_size(&field.ty))
            .filter(|end| *end <= size)
            .ok_or_else(|| {
                format!(
                    "target area too small at line {} (field {})",
                    site.line, field.column
                )
            })?;
        write_field(&mut buffer[start..end], &field.ty, &RawValue::Null).map_err(|_| {
            format!(
                "internal error: initial value of field {} at line {}",
                field.column, site.line
            )
        })?;
    }
    Ok(buffer)
}

/// Schreibt eine Zeile in den Puffer `base` (genau `target_size` Bytes). Jede Spalte wird an ihrem Offset
/// geschrieben. Ein Fehler lässt den Zielbereich unverändert, weil der Aufrufer mit einer Kopie arbeitet.
pub fn encode_row(
    site: &Site,
    columns: &[ResultColumn],
    row: &[RawValue],
    base: &mut [u8],
) -> Result<(), String> {
    if row.len() != columns.len() {
        return Err(format_sql_error(
            &SqlError::protocol(format!(
                "row has {} values, expected {}",
                row.len(),
                columns.len()
            )),
            site,
        ));
    }
    for (column, raw) in columns.iter().zip(row) {
        let size = field_size(&column.target);
        let start = usize::try_from(column.offset)
            .map_err(|_| invalid(site, format!("offset of field {} too large", column.name)))?;
        let end = start
            .checked_add(size)
            .filter(|end| *end <= base.len())
            .ok_or_else(|| {
                format!(
                    "target area too small at line {} (field {})",
                    site.line, column.name
                )
            })?;
        write_field(&mut base[start..end], &column.target, raw).map_err(|e| match e {
            ConvError::NotSupported => format!(
                "conversion not supported at line {} (field {})",
                site.line, column.name
            ),
            ConvError::Failed(reason) => format!(
                "conversion error at line {} (field {}): {reason}",
                site.line, column.name
            ),
        })?;
    }
    Ok(())
}

/// Führt eine `SELECT`-Anweisung für `SINGLE`, `INTO TABLE` und `APPENDING TABLE` aus.
pub fn run_select(
    adapter: &mut dyn SqlAdapter,
    site: &Site,
    request: &SqlRequest,
    params: &[ParamValue],
    target: Target<'_>,
) -> Result<Summary, String> {
    let SqlOp::Select(select) = &request.op else {
        return Err(invalid(site, "not a SELECT statement"));
    };
    if select.result == ResultMode::Loop {
        return Err(invalid(site, "loop result needs a cursor"));
    }
    // Prüfung gegen die IR des Programms (wie der Compiler sie prüft).
    prepare(adapter, site, request, params)?;
    // Ein Einzelsatz liefert höchstens eine Zeile. Das Limit setzt der Kern, nicht der Adapter, und die
    // Fähigkeit `limit` wird dafür nicht verlangt.
    let mut effective = request.clone();
    let SqlOp::Select(limited) = &mut effective.op else {
        return Err(invalid(site, "not a SELECT statement"));
    };
    if select.result == ResultMode::Single {
        limited.up_to = Some(limited.up_to.map_or(1, |n| n.min(1)));
    }
    trace_statement(adapter, site, &effective);
    let outcome: Outcome = adapter
        .execute(&effective, params)
        .map_err(|e| format_sql_error(&e, site))?;
    let rows = outcome.rows;

    match (select.result, target) {
        (ResultMode::Single, Target::Work(work)) => {
            let Some(row) = rows.first() else {
                return Ok(Summary { subrc: 4, dbcnt: 0 });
            };
            let mut buffer = work.to_vec();
            encode_row(site, &select.columns, row, &mut buffer)?;
            work.copy_from_slice(&buffer);
            Ok(Summary { subrc: 0, dbcnt: 1 })
        }
        (ResultMode::Table | ResultMode::Append, Target::Table(table)) => {
            if table.is_null() {
                return Err(format!(
                    "invalid internal table handle at line {}",
                    site.line
                ));
            }
            // Erst alle Zeilen umwandeln, dann die Tabelle ändern. So bleibt sie bei einem Fehler unverändert.
            let size = select.target_size as usize;
            let mut buffers = Vec::with_capacity(rows.len());
            for row in &rows {
                let mut buffer = initial_row(site, &select.target_fields, size)?;
                encode_row(site, &select.columns, row, &mut buffer)?;
                buffers.push(buffer);
            }
            if select.result == ResultMode::Table {
                // SAFETY: `table` ist ein Handle aus `abap_rt_table_new` und ungleich null.
                unsafe { crate::abap_rt_table_clear(table) };
            }
            for buffer in &buffers {
                // SAFETY: `buffer` ist gültig für `buffer.len()` Bytes, `table` wie oben.
                unsafe { crate::abap_rt_table_append(table, buffer.as_ptr(), buffer.len() as u64) };
            }
            let count = buffers.len() as i64;
            Ok(Summary {
                subrc: if count > 0 { 0 } else { 4 },
                dbcnt: count,
            })
        }
        _ => Err(format!(
            "internal error: target does not match result mode at line {}",
            site.line
        )),
    }
}

/// Liest die Zeilen einer Zeilenquelle aus ihren Puffern (je Puffer eine Zeile der Größe `source.size`).
/// Das Ergebnis enthält je Zeile die Werte der Felder in der Reihenfolge von `source.fields`. Ein Fehler bei der
/// Umwandlung nennt Zeile, Feld und die Zeile der Anweisung.
pub fn row_values(
    site: &Site,
    source: &RowSource,
    buffers: &[&[u8]],
) -> Result<Vec<Vec<ParamValue>>, String> {
    let size = source.size as usize;
    let mut rows = Vec::with_capacity(buffers.len());
    for (number, buffer) in buffers.iter().enumerate() {
        if buffer.len() != size {
            return Err(format!(
                "internal error: row size mismatch at line {} (expected {size}, got {})",
                site.line,
                buffer.len()
            ));
        }
        let mut values = Vec::with_capacity(source.fields.len());
        for field in &source.fields {
            let start = field.offset as usize;
            let bytes = buffer
                .get(start..start + field_size(&field.ty))
                .ok_or_else(|| {
                    format!(
                        "internal error: field {} outside its row at line {}",
                        field.column, site.line
                    )
                })?;
            let value = row_value(bytes, &field.ty).map_err(|reason| {
                format!(
                    "conversion error at line {} (field {}, row {}): {reason}",
                    site.line,
                    field.column,
                    number + 1
                )
            })?;
            values.push(value);
        }
        rows.push(values);
    }
    Ok(rows)
}

/// SY-SUBRC einer Schreibanweisung aus der Zahl der betroffenen Zeilen: 0 bei mindestens einer, sonst 4.
fn subrc_of(dbcnt: i64) -> i32 {
    if dbcnt > 0 {
        0
    } else {
        4
    }
}

/// Die Hostparameter gefolgt von den Feldwerten einer Zeile (IR-Konvention: Feld `k` ist Parameter `n + k`).
fn with_row(host: &[ParamValue], row: &[ParamValue]) -> Vec<ParamValue> {
    host.iter().chain(row).cloned().collect()
}

/// Führt eine Anweisung aus. Fehler sind fertig formuliert; eine Dublette wird als Fehler gemeldet.
fn exec(
    adapter: &mut dyn SqlAdapter,
    site: &Site,
    request: &SqlRequest,
    params: &[ParamValue],
) -> Result<Outcome, String> {
    adapter
        .execute(request, params)
        .map_err(|e| format_sql_error(&e, site))
}

/// Führt eine Schreibanweisung aus (`INSERT`, `UPDATE`, `DELETE`, `MODIFY`) und liefert SY-SUBRC und SY-DBCNT.
///
/// `host` sind die Hostparameter der Anweisung, `rows` die Zeilen der Zeilenquelle als Feldwerte (leer ohne
/// Zeilenquelle, sonst eine Zeile bei `FROM wa` und eine je Zeile bei `FROM TABLE`). Die Anweisung läuft je Zeile
/// mit den Parametern `host` gefolgt von den Feldwerten. Die LUW ist Sache des Aufrufers: die Verbindung muss
/// zu diesem Zeitpunkt eine offene LUW haben.
///
/// Regeln (Belegbericht): siehe `doc/sql-spezifikation.md`, Abschnitt 10, und die Kommentare je Anweisung.
pub fn run_write(
    adapter: &mut dyn SqlAdapter,
    site: &Site,
    request: &SqlRequest,
    host: &[ParamValue],
    rows: &[Vec<ParamValue>],
) -> Result<Summary, String> {
    if !matches!(
        request.op,
        SqlOp::Insert(_) | SqlOp::Update(_) | SqlOp::Delete(_) | SqlOp::Modify(_)
    ) {
        return Err(invalid(site, "not a write statement"));
    }
    prepare(adapter, site, request, host)?;
    trace_statement(adapter, site, request);
    match &request.op {
        SqlOp::Insert(op) => run_insert(
            adapter,
            site,
            request,
            op.rows.mode,
            op.accepting_duplicate_keys,
            rows,
        ),
        SqlOp::Update(op) => run_update(adapter, site, request, host, rows, op.rows.is_some()),
        SqlOp::Delete(op) => run_delete(adapter, site, request, host, rows, op.rows.as_ref()),
        SqlOp::Modify(op) => run_modify(adapter, site, request, op.rows.mode, rows),
        SqlOp::Select(_) | SqlOp::Commit | SqlOp::Rollback => {
            Err(invalid(site, "not a write statement"))
        }
    }
}

/// `INSERT`. Einzelzeile: Dublette ergibt SY-SUBRC 4 ohne Ausnahme. `FROM TABLE`: die Zeilen laufen nacheinander,
/// die ersten Zeilen bleiben in der LUW. Ohne `ACCEPTING DUPLICATE KEYS` ist die erste Dublette ein Fehler, mit
/// dem Zusatz wird sie übersprungen (SY-SUBRC 4, SY-DBCNT = eingefügte Zeilen). Leere Tabelle: SY-SUBRC 0,
/// SY-DBCNT 0 (Annahme, nicht im Belegbericht).
fn run_insert(
    adapter: &mut dyn SqlAdapter,
    site: &Site,
    request: &SqlRequest,
    mode: RowMode,
    accepting: bool,
    rows: &[Vec<ParamValue>],
) -> Result<Summary, String> {
    match mode {
        RowMode::Single => {
            let [row] = rows else {
                return Err(invalid(site, "INSERT needs exactly one row"));
            };
            match adapter.execute(request, row) {
                Ok(outcome) => Ok(Summary {
                    subrc: subrc_of(outcome.dbcnt),
                    dbcnt: outcome.dbcnt,
                }),
                Err(e) if e.kind == SqlErrorKind::Duplicate => Ok(Summary { subrc: 4, dbcnt: 0 }),
                Err(e) => Err(format_sql_error(&e, site)),
            }
        }
        RowMode::Table => {
            let mut inserted = 0;
            let mut skipped = false;
            for row in rows {
                match adapter.execute(request, row) {
                    Ok(outcome) => inserted += outcome.dbcnt,
                    Err(e) if e.kind == SqlErrorKind::Duplicate => {
                        if !accepting {
                            return Err(format_sql_error(&SqlError::db("duplicate key"), site));
                        }
                        skipped = true;
                    }
                    Err(e) => return Err(format_sql_error(&e, site)),
                }
            }
            Ok(Summary {
                subrc: if skipped { 4 } else { 0 },
                dbcnt: inserted,
            })
        }
    }
}

/// `UPDATE`. Menge (`SET ... WHERE`): SY-DBCNT = Zeilen, SY-SUBRC 4 bei 0. Nach Schlüssel (`FROM wa`): SY-DBCNT 1
/// bei Änderung, sonst 0 (Annahme). Eine Dublette durch die Änderung ergibt SY-SUBRC 4 ohne Änderung.
fn run_update(
    adapter: &mut dyn SqlAdapter,
    site: &Site,
    request: &SqlRequest,
    host: &[ParamValue],
    rows: &[Vec<ParamValue>],
    by_key: bool,
) -> Result<Summary, String> {
    let params = if by_key {
        let [row] = rows else {
            return Err(invalid(site, "UPDATE FROM needs exactly one row"));
        };
        with_row(host, row)
    } else {
        host.to_vec()
    };
    match adapter.execute(request, &params) {
        Ok(outcome) => Ok(Summary {
            subrc: subrc_of(outcome.dbcnt),
            dbcnt: if by_key {
                i64::from(outcome.dbcnt > 0)
            } else {
                outcome.dbcnt
            },
        }),
        Err(e) if e.kind == SqlErrorKind::Duplicate => Ok(Summary { subrc: 4, dbcnt: 0 }),
        Err(e) => Err(format_sql_error(&e, site)),
    }
}

/// `DELETE`. Menge (`WHERE` oder ohne Bedingung): SY-DBCNT = gelöschte Zeilen. `FROM wa`: SY-DBCNT 1 bei Löschung,
/// sonst 0 (Annahme). `FROM TABLE itab`: SY-SUBRC 0, wenn jede Zeile gelöscht wurde oder itab leer ist, sonst 4;
/// SY-DBCNT = Summe der gelöschten Zeilen.
fn run_delete(
    adapter: &mut dyn SqlAdapter,
    site: &Site,
    request: &SqlRequest,
    host: &[ParamValue],
    rows: &[Vec<ParamValue>],
    source: Option<&RowSource>,
) -> Result<Summary, String> {
    match source {
        None => {
            let outcome = exec(adapter, site, request, host)?;
            Ok(Summary {
                subrc: subrc_of(outcome.dbcnt),
                dbcnt: outcome.dbcnt,
            })
        }
        Some(source) if source.mode == RowMode::Single => {
            let [row] = rows else {
                return Err(invalid(site, "DELETE FROM needs exactly one row"));
            };
            let outcome = exec(adapter, site, request, &with_row(host, row))?;
            Ok(Summary {
                subrc: subrc_of(outcome.dbcnt),
                dbcnt: i64::from(outcome.dbcnt > 0),
            })
        }
        Some(_) => {
            let mut deleted = 0;
            let mut all = true;
            for row in rows {
                let outcome = exec(adapter, site, request, &with_row(host, row))?;
                deleted += outcome.dbcnt;
                all &= outcome.dbcnt > 0;
            }
            Ok(Summary {
                subrc: if all { 0 } else { 4 },
                dbcnt: deleted,
            })
        }
    }
}

/// `MODIFY`. Einzelzeile: SY-SUBRC 0 bei Anlage oder Änderung, SY-DBCNT 1. `FROM TABLE`: SY-DBCNT = Zeilen, die
/// verarbeitet wurden; SY-SUBRC 0, wenn alle Zeilen verarbeitet wurden, sonst 4 (Annahme, da der Belegbericht nur
/// den Einzelsatz und die Doku die Regel "nicht alle" nennt).
fn run_modify(
    adapter: &mut dyn SqlAdapter,
    site: &Site,
    request: &SqlRequest,
    mode: RowMode,
    rows: &[Vec<ParamValue>],
) -> Result<Summary, String> {
    match mode {
        RowMode::Single => {
            let [row] = rows else {
                return Err(invalid(site, "MODIFY needs exactly one row"));
            };
            let processed = exec(adapter, site, request, row)?.dbcnt > 0;
            Ok(Summary {
                subrc: if processed { 0 } else { 4 },
                dbcnt: i64::from(processed),
            })
        }
        RowMode::Table => {
            let mut processed = 0i64;
            for row in rows {
                if exec(adapter, site, request, row)?.dbcnt > 0 {
                    processed += 1;
                }
            }
            Ok(Summary {
                subrc: if processed as usize == rows.len() {
                    0
                } else {
                    4
                },
                dbcnt: processed,
            })
        }
    }
}

/// Öffnet den Cursor für `SELECT ... ENDSELECT` (Ergebnis `loop`).
pub fn open_cursor(
    adapter: &mut dyn SqlAdapter,
    site: &Site,
    request: &SqlRequest,
    params: &[ParamValue],
) -> Result<CursorId, String> {
    let SqlOp::Select(select) = &request.op else {
        return Err(invalid(site, "not a SELECT statement"));
    };
    if select.result != ResultMode::Loop {
        return Err(invalid(site, "cursor needs a loop result"));
    }
    prepare(adapter, site, request, params)?;
    trace_statement(adapter, site, request);
    adapter
        .open_cursor(request, params)
        .map_err(|e| format_sql_error(&e, site))
}

/// Holt die nächste Zeile eines Cursors in den Arbeitsbereich `target`. Gibt `true` bei einer Zeile zurück,
/// `false` am Ende. `count` zählt die gelesenen Zeilen (SY-DBCNT).
pub fn fetch_row(
    adapter: &mut dyn SqlAdapter,
    site: &Site,
    request: &SqlRequest,
    cursor: CursorId,
    target: &mut [u8],
    count: &mut i64,
) -> Result<bool, String> {
    let SqlOp::Select(select) = &request.op else {
        return Err(invalid(site, "not a SELECT statement"));
    };
    match adapter
        .fetch(cursor)
        .map_err(|e| format_sql_error(&e, site))?
    {
        None => Ok(false),
        Some(row) => {
            let mut buffer = target.to_vec();
            encode_row(site, &select.columns, &row, &mut buffer)?;
            target.copy_from_slice(&buffer);
            *count += 1;
            Ok(true)
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::{Arc, Mutex};

    use abap_sql_ir::{AbapType, CharType, InsertOp, OrderItem, Param, RowField, SelectOp};

    use super::*;

    /// Adapter für Kern-Tests: liefert feste Zeilen und zeichnet das übergebene Limit auf. Schreibanweisungen
    /// bekommen ihre Ergebnisse der Reihe nach aus `writes` (leer: jede liefert `dbcnt` 1) und die Parameter
    /// werden in `seen_params` aufgezeichnet.
    pub(crate) struct FakeAdapter {
        pub rows: Vec<Vec<RawValue>>,
        pub caps: super::super::adapter::Capabilities,
        pub check_error: Option<SqlError>,
        pub seen_limit: Arc<Mutex<Vec<Option<u64>>>>,
        pub cursor_rows: Vec<Vec<RawValue>>,
        pub cursor_pos: usize,
        pub writes: std::collections::VecDeque<Result<i64, SqlError>>,
        pub seen_params: Arc<Mutex<Vec<Vec<ParamValue>>>>,
    }

    impl FakeAdapter {
        pub(crate) fn with_rows(rows: Vec<Vec<RawValue>>) -> FakeAdapter {
            FakeAdapter {
                rows,
                caps: super::super::adapter::Capabilities::sqlite(),
                check_error: None,
                seen_limit: Arc::new(Mutex::new(Vec::new())),
                cursor_rows: Vec::new(),
                cursor_pos: 0,
                writes: std::collections::VecDeque::new(),
                seen_params: Arc::new(Mutex::new(Vec::new())),
            }
        }

        /// Adapter für Schreibanweisungen mit den Ergebnissen `writes` in dieser Reihenfolge.
        pub(crate) fn with_writes(writes: Vec<Result<i64, SqlError>>) -> FakeAdapter {
            let mut adapter = FakeAdapter::with_rows(vec![]);
            adapter.writes = writes.into();
            adapter
        }
    }

    impl SqlAdapter for FakeAdapter {
        fn capabilities(&self) -> &super::super::adapter::Capabilities {
            &self.caps
        }
        fn check(&self, _req: &SqlRequest) -> Result<(), SqlError> {
            match &self.check_error {
                Some(e) => Err(e.clone()),
                None => Ok(()),
            }
        }
        fn execute(
            &mut self,
            req: &SqlRequest,
            params: &[ParamValue],
        ) -> Result<Outcome, SqlError> {
            match &req.op {
                SqlOp::Select(s) => {
                    self.seen_limit
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .push(s.up_to);
                    Ok(Outcome {
                        rows: self.rows.clone(),
                        dbcnt: 0,
                    })
                }
                _ => {
                    self.seen_params
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .push(params.to_vec());
                    let dbcnt = self.writes.pop_front().unwrap_or(Ok(1))?;
                    Ok(Outcome {
                        rows: Vec::new(),
                        dbcnt,
                    })
                }
            }
        }
        fn open_cursor(
            &mut self,
            _req: &SqlRequest,
            _params: &[ParamValue],
        ) -> Result<CursorId, SqlError> {
            self.cursor_pos = 0;
            Ok(1)
        }
        fn fetch(&mut self, _cur: CursorId) -> Result<Option<Vec<RawValue>>, SqlError> {
            let row = self.cursor_rows.get(self.cursor_pos).cloned();
            self.cursor_pos += 1;
            Ok(row)
        }
        fn close_cursor(&mut self, _cur: CursorId) {}
        fn begin(&mut self) -> Result<(), SqlError> {
            Ok(())
        }
        fn commit(&mut self) -> Result<(), SqlError> {
            Ok(())
        }
        fn rollback(&mut self) -> Result<(), SqlError> {
            Ok(())
        }
    }

    pub(crate) fn site() -> Site {
        Site {
            line: 9,
            connection: "DEFAULT".into(),
            table: "ZFLUG".into(),
        }
    }

    fn char(length: u32) -> AbapType {
        AbapType::Char {
            length,
            dtype: CharType::Char,
        }
    }

    /// Zwei Spalten: `carrid` (CHAR 2) bei Offset 0 und `seatsmax` (INT) bei Offset 4.
    pub(crate) fn request(result: ResultMode, up_to: Option<u64>, target_size: u64) -> SqlRequest {
        SqlRequest {
            ir_version: abap_sql_ir::IR_VERSION,
            line: 9,
            connection: "DEFAULT".into(),
            op: SqlOp::Select(SelectOp {
                table: "ZFLUG".into(),
                columns: vec![
                    ResultColumn {
                        name: "carrid".into(),
                        source: char(2),
                        target: char(2),
                        offset: 0,
                    },
                    ResultColumn {
                        name: "seatsmax".into(),
                        source: AbapType::Int,
                        target: AbapType::Int,
                        offset: 4,
                    },
                ],
                filter: None,
                order_by: vec![OrderItem {
                    column: "carrid".into(),
                    descending: false,
                }],
                up_to,
                result,
                target_size,
                target_fields: vec![],
                params: Vec::<Param>::new(),
            }),
        }
    }

    pub(crate) fn text_units(bytes: &[u8]) -> String {
        let units: Vec<u16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| u16::from_ne_bytes(*p))
            .collect();
        String::from_utf16(&units).unwrap()
    }

    pub(crate) fn int_at(bytes: &[u8], offset: usize) -> i32 {
        i32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
    }

    pub(crate) fn row(carrid: &str, seats: i64) -> Vec<RawValue> {
        vec![RawValue::Text(carrid.into()), RawValue::Int(seats)]
    }

    /// Arbeitsbereich mit `size` Bytes, vorbelegt mit `'#'`-Muster, damit man Unveränderung sieht.
    fn work_area(size: usize) -> Vec<u8> {
        (0..size)
            .map(|i| if i % 2 == 0 { b'#' } else { 0 })
            .collect()
    }

    #[test]
    fn single_hit_writes_fields_and_sets_subrc_zero() {
        let mut adapter = FakeAdapter::with_rows(vec![row("LH", 180)]);
        let req = request(ResultMode::Single, None, 8);
        let mut area = vec![0u8; 8];
        let summary =
            run_select(&mut adapter, &site(), &req, &[], Target::Work(&mut area)).unwrap();
        assert_eq!(summary, Summary { subrc: 0, dbcnt: 1 });
        assert_eq!(text_units(&area[0..4]), "LH");
        assert_eq!(int_at(&area, 4), 180);
    }

    #[test]
    fn single_without_rows_keeps_target_and_sets_subrc_four() {
        let mut adapter = FakeAdapter::with_rows(vec![]);
        let req = request(ResultMode::Single, None, 8);
        let mut area = work_area(8);
        let before = area.clone();
        let summary =
            run_select(&mut adapter, &site(), &req, &[], Target::Work(&mut area)).unwrap();
        assert_eq!(summary, Summary { subrc: 4, dbcnt: 0 });
        assert_eq!(area, before);
    }

    #[test]
    fn single_forces_limit_one_on_the_request_passed_to_the_adapter() {
        let mut adapter = FakeAdapter::with_rows(vec![row("LH", 1)]);
        let seen = adapter.seen_limit.clone();
        let req = request(ResultMode::Single, Some(10), 8);
        let mut area = vec![0u8; 8];
        run_select(&mut adapter, &site(), &req, &[], Target::Work(&mut area)).unwrap();
        assert_eq!(*seen.lock().unwrap(), vec![Some(1)]);
    }

    #[test]
    fn null_values_become_initial_values_in_the_target() {
        let mut adapter = FakeAdapter::with_rows(vec![vec![RawValue::Null, RawValue::Null]]);
        let req = request(ResultMode::Single, None, 8);
        let mut area = vec![0x55u8; 8];
        run_select(&mut adapter, &site(), &req, &[], Target::Work(&mut area)).unwrap();
        assert_eq!(text_units(&area[0..4]), "  ");
        assert_eq!(int_at(&area, 4), 0);
    }

    #[test]
    fn character_value_is_cut_to_target_length() {
        let mut adapter = FakeAdapter::with_rows(vec![row("LHXX", 3)]);
        let req = request(ResultMode::Single, None, 8);
        let mut area = vec![0u8; 8];
        run_select(&mut adapter, &site(), &req, &[], Target::Work(&mut area)).unwrap();
        assert_eq!(text_units(&area[0..4]), "LH");
    }

    #[test]
    fn table_mode_clears_then_fills_the_table() {
        let table = crate::abap_rt_table_new(8);
        // Vorhandene Zeile, die danach verschwinden muss.
        let old = [9u8; 8];
        unsafe { crate::abap_rt_table_append(table, old.as_ptr(), 8) };
        let mut adapter = FakeAdapter::with_rows(vec![row("LH", 180), row("AF", 220)]);
        let req = request(ResultMode::Table, None, 8);
        let summary = run_select(&mut adapter, &site(), &req, &[], Target::Table(table)).unwrap();
        assert_eq!(summary, Summary { subrc: 0, dbcnt: 2 });
        let count = unsafe { crate::abap_rt_table_count(table) };
        assert_eq!(count, 2);
        let first = unsafe { std::slice::from_raw_parts(crate::abap_rt_table_row(table, 0), 8) };
        assert_eq!(text_units(&first[0..4]), "LH");
        assert_eq!(int_at(first, 4), 180);
        unsafe { drop(Box::from_raw(table)) };
    }

    #[test]
    fn table_mode_without_rows_empties_the_table_and_sets_subrc_four() {
        let table = crate::abap_rt_table_new(8);
        let old = [9u8; 8];
        unsafe { crate::abap_rt_table_append(table, old.as_ptr(), 8) };
        let mut adapter = FakeAdapter::with_rows(vec![]);
        let req = request(ResultMode::Table, None, 8);
        let summary = run_select(&mut adapter, &site(), &req, &[], Target::Table(table)).unwrap();
        assert_eq!(summary, Summary { subrc: 4, dbcnt: 0 });
        assert_eq!(unsafe { crate::abap_rt_table_count(table) }, 0);
        unsafe { drop(Box::from_raw(table)) };
    }

    #[test]
    fn append_mode_keeps_existing_rows_and_adds_hits() {
        let table = crate::abap_rt_table_new(8);
        let old = [9u8; 8];
        unsafe { crate::abap_rt_table_append(table, old.as_ptr(), 8) };
        let mut adapter = FakeAdapter::with_rows(vec![row("LH", 180)]);
        let req = request(ResultMode::Append, None, 8);
        let summary = run_select(&mut adapter, &site(), &req, &[], Target::Table(table)).unwrap();
        assert_eq!(summary, Summary { subrc: 0, dbcnt: 1 });
        assert_eq!(unsafe { crate::abap_rt_table_count(table) }, 2);
        let first = unsafe { std::slice::from_raw_parts(crate::abap_rt_table_row(table, 0), 8) };
        assert_eq!(first, &old);
        unsafe { drop(Box::from_raw(table)) };
    }

    #[test]
    fn conversion_error_in_a_later_row_leaves_the_table_unchanged() {
        let table = crate::abap_rt_table_new(8);
        let old = [9u8; 8];
        unsafe { crate::abap_rt_table_append(table, old.as_ptr(), 8) };
        // Zweite Zeile hat einen Text, der kein Integer ist.
        let bad = vec![RawValue::Text("AF".into()), RawValue::Text("many".into())];
        let mut adapter = FakeAdapter::with_rows(vec![row("LH", 180), bad]);
        let req = request(ResultMode::Table, None, 8);
        let err = run_select(&mut adapter, &site(), &req, &[], Target::Table(table)).unwrap_err();
        assert!(err.contains("line 9"), "{err}");
        assert!(err.contains("seatsmax"), "{err}");
        assert_eq!(unsafe { crate::abap_rt_table_count(table) }, 1);
        unsafe { drop(Box::from_raw(table)) };
    }

    #[test]
    fn integer_out_of_range_names_line_and_field() {
        let mut adapter = FakeAdapter::with_rows(vec![vec![
            RawValue::Text("LH".into()),
            RawValue::Int(i64::from(i32::MAX) + 1),
        ]]);
        let req = request(ResultMode::Single, None, 8);
        let mut area = vec![0u8; 8];
        let err =
            run_select(&mut adapter, &site(), &req, &[], Target::Work(&mut area)).unwrap_err();
        assert_eq!(
            err,
            "conversion error at line 9 (field seatsmax): value 2147483648 out of range for TYPE i"
        );
    }

    #[test]
    fn integer_source_into_char_target_is_reported_as_not_supported() {
        let mut adapter = FakeAdapter::with_rows(vec![vec![RawValue::Int(5), RawValue::Int(1)]]);
        let req = request(ResultMode::Single, None, 8);
        let mut area = vec![0u8; 8];
        let err =
            run_select(&mut adapter, &site(), &req, &[], Target::Work(&mut area)).unwrap_err();
        assert_eq!(err, "conversion not supported at line 9 (field carrid)");
    }

    #[test]
    fn target_area_too_small_is_rejected_without_writing() {
        // Die Zielspalte liegt bei Offset 4 mit 4 Bytes, der Bereich hat nur 6 Bytes.
        let mut adapter = FakeAdapter::with_rows(vec![row("LH", 1)]);
        let req = request(ResultMode::Single, None, 6);
        let mut area = work_area(6);
        let before = area.clone();
        let err =
            run_select(&mut adapter, &site(), &req, &[], Target::Work(&mut area)).unwrap_err();
        assert_eq!(err, "target area too small at line 9 (field seatsmax)");
        assert_eq!(area, before);
    }

    #[test]
    fn row_with_wrong_number_of_values_is_a_protocol_error() {
        let mut adapter = FakeAdapter::with_rows(vec![vec![RawValue::Int(1)]]);
        let req = request(ResultMode::Single, None, 8);
        let mut area = vec![0u8; 8];
        let err =
            run_select(&mut adapter, &site(), &req, &[], Target::Work(&mut area)).unwrap_err();
        assert!(err.contains("row has 1 values, expected 2"), "{err}");
    }

    #[test]
    fn capability_missing_is_reported_before_execution() {
        let mut adapter = FakeAdapter::with_rows(vec![row("LH", 1)]);
        adapter.caps = super::super::adapter::Capabilities::default();
        let req = request(ResultMode::Single, None, 8);
        let mut area = vec![0u8; 8];
        let err =
            run_select(&mut adapter, &site(), &req, &[], Target::Work(&mut area)).unwrap_err();
        assert_eq!(
            err,
            "capability missing at line 9 (connection DEFAULT): SELECT not supported by this adapter"
        );
        assert!(adapter.seen_limit.lock().unwrap().is_empty());
    }

    #[test]
    fn check_error_of_the_adapter_stops_before_execute() {
        let mut adapter = FakeAdapter::with_rows(vec![row("LH", 1)]);
        adapter.check_error = Some(SqlError::db("connection refused"));
        let req = request(ResultMode::Single, None, 8);
        let mut area = vec![0u8; 8];
        let err =
            run_select(&mut adapter, &site(), &req, &[], Target::Work(&mut area)).unwrap_err();
        assert_eq!(
            err,
            "db error at line 9 (connection DEFAULT, table ZFLUG): connection refused"
        );
        assert!(adapter.seen_limit.lock().unwrap().is_empty());
    }

    #[test]
    fn parameter_index_out_of_range_is_invalid_ir() {
        let mut adapter = FakeAdapter::with_rows(vec![]);
        let mut req = request(ResultMode::Single, None, 8);
        let SqlOp::Select(s) = &mut req.op else {
            unreachable!("select request")
        };
        s.filter = Some(Expr::Compare {
            op: abap_sql_ir::CmpOp::Eq,
            left: Box::new(Expr::Column {
                name: "carrid".into(),
            }),
            right: Box::new(Expr::Param { index: 1 }),
        });
        let mut area = vec![0u8; 8];
        let err = run_select(
            &mut adapter,
            &site(),
            &req,
            &[ParamValue::Text("LH".into())],
            Target::Work(&mut area),
        )
        .unwrap_err();
        assert!(err.starts_with("invalid SQL IR at line 9"), "{err}");
    }

    #[test]
    fn loop_result_is_rejected_by_run_select() {
        let mut adapter = FakeAdapter::with_rows(vec![]);
        let req = request(ResultMode::Loop, None, 8);
        let mut area = vec![0u8; 8];
        let err =
            run_select(&mut adapter, &site(), &req, &[], Target::Work(&mut area)).unwrap_err();
        assert!(err.contains("needs a cursor"), "{err}");
    }

    #[test]
    fn cursor_fetch_writes_rows_and_counts_them() {
        let mut adapter = FakeAdapter::with_rows(vec![]);
        adapter.cursor_rows = vec![row("LH", 1), row("AF", 2)];
        let req = request(ResultMode::Loop, None, 8);
        let id = open_cursor(&mut adapter, &site(), &req, &[]).unwrap();
        let mut area = vec![0u8; 8];
        let mut count = 0i64;
        assert!(fetch_row(&mut adapter, &site(), &req, id, &mut area, &mut count).unwrap());
        assert_eq!(
            (text_units(&area[0..4]), int_at(&area, 4), count),
            ("LH".into(), 1, 1)
        );
        assert!(fetch_row(&mut adapter, &site(), &req, id, &mut area, &mut count).unwrap());
        assert_eq!(
            (text_units(&area[0..4]), int_at(&area, 4), count),
            ("AF".into(), 2, 2)
        );
        assert!(!fetch_row(&mut adapter, &site(), &req, id, &mut area, &mut count).unwrap());
        assert_eq!(count, 2);
    }

    #[test]
    fn cursor_open_needs_loop_result() {
        let mut adapter = FakeAdapter::with_rows(vec![]);
        let req = request(ResultMode::Single, None, 8);
        let err = open_cursor(&mut adapter, &site(), &req, &[]).unwrap_err();
        assert!(err.contains("needs a loop result"), "{err}");
    }

    #[test]
    fn process_error_appends_stderr_after_the_line() {
        let error = SqlError {
            kind: SqlErrorKind::Process,
            message: "external adapter for connection EXTERN ended during request".into(),
            detail: Some("boom".into()),
        };
        let text = format_sql_error(&error, &site());
        assert_eq!(
            text,
            "external adapter for connection EXTERN ended during request at line 9: boom"
        );
    }

    #[test]
    fn unconfigured_table_is_reported_with_line() {
        let config = Config::default();
        let err = resolve(&config, "ZFLUG", 9).unwrap_err();
        assert_eq!(err, "table ZFLUG is not configured (line 9)");
    }

    #[test]
    fn table_with_undefined_connection_is_reported_with_table_and_line() {
        let dir = std::env::temp_dir();
        let config = Config::from_toml(
            "[tables.ZFLUG]\nconnection = \"ARCHIV\"\ntarget = \"flug\"\n",
            &dir,
        )
        .unwrap();
        let err = resolve(&config, "ZFLUG", 9).unwrap_err();
        assert_eq!(
            err,
            "connection ARCHIV is not defined in the configuration (table ZFLUG, line 9)"
        );
    }

    #[test]
    fn postgres_adapter_is_reported_as_not_available() {
        let dir = std::env::temp_dir();
        let config = Config::from_toml(
            "[connections.ARCHIV]\nadapter = \"postgres\"\n[tables.ZFLUG]\nconnection = \"ARCHIV\"\ntarget = \"flug\"\n",
            &dir,
        )
        .unwrap();
        let (_, connection) = resolve(&config, "ZFLUG", 9).unwrap();
        let err = match open_adapter(&config, connection, &site()) {
            Ok(_) => panic!("postgres must not open"),
            Err(e) => e,
        };
        assert_eq!(err, "adapter not available for connection ARCHIV: postgres");
    }

    #[test]
    fn initial_row_has_blank_characters_and_positive_packed_zero() {
        let fields = vec![
            RowField {
                column: "extra".into(),
                ty: char(2),
                offset: 0,
            },
            RowField {
                column: "amount".into(),
                ty: AbapType::Packed { bytes: 2, scale: 0 },
                offset: 4,
            },
            RowField {
                column: "seats".into(),
                ty: AbapType::Int,
                offset: 6,
            },
        ];
        let buffer = initial_row(&site(), &fields, 10).unwrap();
        assert_eq!(text_units(&buffer[0..4]), "  ");
        // Gepackte Null mit positivem Vorzeichen (0x0C) in der letzten Nibble
        assert_eq!(&buffer[4..6], &[0x00, 0x0C]);
        assert_eq!(int_at(&buffer, 6), 0);
    }

    #[test]
    fn initial_row_field_outside_the_area_is_rejected() {
        let fields = vec![RowField {
            column: "seats".into(),
            ty: AbapType::Int,
            offset: 8,
        }];
        let err = initial_row(&site(), &fields, 10).unwrap_err();
        assert_eq!(err, "target area too small at line 9 (field seats)");
    }

    #[test]
    fn invalid_ir_message_names_the_line() {
        assert_eq!(
            invalid(&site(), "broken"),
            "invalid SQL IR at line 9: broken"
        );
    }

    /// Zeilenquelle mit `carrid` (CHAR 2, Offset 0) und `seatsmax` (INT, Offset 4), Zeilengröße 8.
    fn write_source(mode: RowMode) -> RowSource {
        RowSource {
            mode,
            fields: vec![
                RowField {
                    column: "carrid".into(),
                    ty: char(2),
                    offset: 0,
                },
                RowField {
                    column: "seatsmax".into(),
                    ty: AbapType::Int,
                    offset: 4,
                },
            ],
            size: 8,
        }
    }

    fn write_request(op: SqlOp) -> SqlRequest {
        SqlRequest {
            ir_version: abap_sql_ir::IR_VERSION,
            line: 9,
            connection: "DEFAULT".into(),
            op,
        }
    }

    fn insert_request(mode: RowMode, accepting: bool) -> SqlRequest {
        write_request(SqlOp::Insert(InsertOp {
            table: "ZFLUG".into(),
            rows: write_source(mode),
            accepting_duplicate_keys: accepting,
        }))
    }

    /// Feldwerte einer Zeile der Zeilenquelle aus `write_source`.
    fn value_row(carrid: &str, seats: i32) -> Vec<ParamValue> {
        vec![
            ParamValue::Text(carrid.into()),
            ParamValue::Int(i64::from(seats)),
        ]
    }

    fn dup() -> Result<i64, SqlError> {
        Err(SqlError::duplicate("UNIQUE constraint failed"))
    }

    #[test]
    fn single_insert_with_duplicate_key_sets_subrc_four_without_error() {
        let mut adapter = FakeAdapter::with_writes(vec![dup()]);
        let req = insert_request(RowMode::Single, false);
        let summary = run_write(&mut adapter, &site(), &req, &[], &[value_row("LH", 180)]).unwrap();
        assert_eq!(summary, Summary { subrc: 4, dbcnt: 0 });
    }

    #[test]
    fn single_insert_passes_only_the_row_values_as_parameters() {
        let mut adapter = FakeAdapter::with_writes(vec![Ok(1)]);
        let seen = adapter.seen_params.clone();
        let req = insert_request(RowMode::Single, false);
        let summary = run_write(&mut adapter, &site(), &req, &[], &[value_row("LH", 180)]).unwrap();
        assert_eq!(summary, Summary { subrc: 0, dbcnt: 1 });
        assert_eq!(*seen.lock().unwrap(), vec![value_row("LH", 180)]);
    }

    #[test]
    fn from_table_insert_without_accepting_stops_at_first_duplicate_and_keeps_earlier_rows() {
        // Zeile 1 wird eingefügt, Zeile 2 ist eine Dublette, Zeile 3 wird nicht mehr ausgeführt.
        let mut adapter = FakeAdapter::with_writes(vec![Ok(1), dup(), Ok(1)]);
        let seen = adapter.seen_params.clone();
        let req = insert_request(RowMode::Table, false);
        let rows = vec![value_row("LH", 1), value_row("AF", 2), value_row("BA", 3)];
        let err = run_write(&mut adapter, &site(), &req, &[], &rows).unwrap_err();
        assert_eq!(
            err,
            "db error at line 9 (connection DEFAULT, table ZFLUG): duplicate key"
        );
        assert_eq!(
            seen.lock().unwrap().len(),
            2,
            "rows after the duplicate are not sent"
        );
    }

    #[test]
    fn from_table_insert_with_accepting_skips_duplicates_and_sets_subrc_four() {
        let mut adapter = FakeAdapter::with_writes(vec![Ok(1), dup(), Ok(1)]);
        let seen = adapter.seen_params.clone();
        let req = insert_request(RowMode::Table, true);
        let rows = vec![value_row("LH", 1), value_row("AF", 2), value_row("BA", 3)];
        let summary = run_write(&mut adapter, &site(), &req, &[], &rows).unwrap();
        assert_eq!(summary, Summary { subrc: 4, dbcnt: 2 });
        assert_eq!(seen.lock().unwrap().len(), 3, "every row is attempted");
    }

    #[test]
    fn from_table_insert_without_duplicates_sets_subrc_zero_and_counts_the_rows() {
        let mut adapter = FakeAdapter::with_writes(vec![]);
        let req = insert_request(RowMode::Table, false);
        let rows = vec![value_row("LH", 1), value_row("AF", 2)];
        let summary = run_write(&mut adapter, &site(), &req, &[], &rows).unwrap();
        assert_eq!(summary, Summary { subrc: 0, dbcnt: 2 });
    }

    #[test]
    fn from_table_insert_of_empty_table_sets_subrc_zero_and_dbcnt_zero() {
        // Annahme: nicht im Belegbericht. Eine leere Tabelle ist keine Fehlanzeige.
        let mut adapter = FakeAdapter::with_writes(vec![]);
        let seen = adapter.seen_params.clone();
        let req = insert_request(RowMode::Table, false);
        let summary = run_write(&mut adapter, &site(), &req, &[], &[]).unwrap();
        assert_eq!(summary, Summary { subrc: 0, dbcnt: 0 });
        assert!(seen.lock().unwrap().is_empty());
    }

    /// `UPDATE zflug SET seatsmax = @lv WHERE carrid = @lv` mit einem Hostparameter.
    fn update_set_request() -> SqlRequest {
        use abap_sql_ir::{Assignment, CmpOp, Expr, UpdateOp};
        write_request(SqlOp::Update(UpdateOp {
            table: "ZFLUG".into(),
            assignments: vec![Assignment {
                column: "seatsmax".into(),
                value: Expr::Param { index: 0 },
            }],
            filter: Some(Expr::Compare {
                op: CmpOp::Eq,
                left: Box::new(Expr::Column {
                    name: "carrid".into(),
                }),
                right: Box::new(Expr::Param { index: 0 }),
            }),
            rows: None,
            params: vec![Param {
                name: "lv".into(),
                ty: AbapType::Int,
            }],
        }))
    }

    #[test]
    fn update_without_matching_row_sets_subrc_four_and_dbcnt_zero() {
        let mut adapter = FakeAdapter::with_writes(vec![Ok(0)]);
        let seen = adapter.seen_params.clone();
        let req = update_set_request();
        let summary = run_write(&mut adapter, &site(), &req, &[ParamValue::Int(5)], &[]).unwrap();
        assert_eq!(summary, Summary { subrc: 4, dbcnt: 0 });
        assert_eq!(*seen.lock().unwrap(), vec![vec![ParamValue::Int(5)]]);
    }

    #[test]
    fn update_with_matching_rows_sets_dbcnt_to_their_number() {
        let mut adapter = FakeAdapter::with_writes(vec![Ok(3)]);
        let req = update_set_request();
        let summary = run_write(&mut adapter, &site(), &req, &[ParamValue::Int(5)], &[]).unwrap();
        assert_eq!(summary, Summary { subrc: 0, dbcnt: 3 });
    }

    #[test]
    fn update_from_row_sets_dbcnt_one_when_the_row_changed() {
        let mut adapter = FakeAdapter::with_writes(vec![Ok(1), Ok(0)]);
        let seen = adapter.seen_params.clone();
        let req = write_request(SqlOp::Update(abap_sql_ir::UpdateOp {
            table: "ZFLUG".into(),
            assignments: vec![],
            filter: None,
            rows: Some(write_source(RowMode::Single)),
            params: vec![],
        }));
        let row = value_row("LH", 180);
        let changed =
            run_write(&mut adapter, &site(), &req, &[], std::slice::from_ref(&row)).unwrap();
        assert_eq!(changed, Summary { subrc: 0, dbcnt: 1 });
        let missing =
            run_write(&mut adapter, &site(), &req, &[], std::slice::from_ref(&row)).unwrap();
        assert_eq!(missing, Summary { subrc: 4, dbcnt: 0 });
        // Die Zeilenwerte stehen als Parameter an der Stelle n + k (hier n = 0).
        assert_eq!(seen.lock().unwrap()[0], row);
    }

    #[test]
    fn update_duplicate_keeps_the_target_and_sets_subrc_four() {
        let mut adapter = FakeAdapter::with_writes(vec![dup()]);
        let req = update_set_request();
        let summary = run_write(&mut adapter, &site(), &req, &[ParamValue::Int(5)], &[]).unwrap();
        assert_eq!(summary, Summary { subrc: 4, dbcnt: 0 });
    }

    #[test]
    fn delete_without_where_sets_dbcnt_to_all_removed_rows() {
        let mut adapter = FakeAdapter::with_writes(vec![Ok(5)]);
        let req = write_request(SqlOp::Delete(abap_sql_ir::DeleteOp {
            table: "ZFLUG".into(),
            filter: None,
            rows: None,
            params: vec![],
        }));
        let summary = run_write(&mut adapter, &site(), &req, &[], &[]).unwrap();
        assert_eq!(summary, Summary { subrc: 0, dbcnt: 5 });
    }

    #[test]
    fn delete_from_table_is_subrc_four_when_a_row_was_not_deleted() {
        let mut adapter = FakeAdapter::with_writes(vec![Ok(1), Ok(0)]);
        let req = write_request(SqlOp::Delete(abap_sql_ir::DeleteOp {
            table: "ZFLUG".into(),
            filter: None,
            rows: Some(write_source(RowMode::Table)),
            params: vec![],
        }));
        let rows = vec![value_row("LH", 1), value_row("XX", 2)];
        let summary = run_write(&mut adapter, &site(), &req, &[], &rows).unwrap();
        assert_eq!(summary, Summary { subrc: 4, dbcnt: 1 });
    }

    #[test]
    fn delete_from_table_is_subrc_zero_when_every_row_was_deleted() {
        let mut adapter = FakeAdapter::with_writes(vec![Ok(1), Ok(1)]);
        let req = write_request(SqlOp::Delete(abap_sql_ir::DeleteOp {
            table: "ZFLUG".into(),
            filter: None,
            rows: Some(write_source(RowMode::Table)),
            params: vec![],
        }));
        let rows = vec![value_row("LH", 1), value_row("AF", 2)];
        let summary = run_write(&mut adapter, &site(), &req, &[], &rows).unwrap();
        assert_eq!(summary, Summary { subrc: 0, dbcnt: 2 });
    }

    #[test]
    fn delete_from_single_row_sets_dbcnt_one_or_zero() {
        let mut adapter = FakeAdapter::with_writes(vec![Ok(1), Ok(0)]);
        let req = write_request(SqlOp::Delete(abap_sql_ir::DeleteOp {
            table: "ZFLUG".into(),
            filter: None,
            rows: Some(write_source(RowMode::Single)),
            params: vec![],
        }));
        let row = value_row("LH", 1);
        let deleted =
            run_write(&mut adapter, &site(), &req, &[], std::slice::from_ref(&row)).unwrap();
        assert_eq!(deleted, Summary { subrc: 0, dbcnt: 1 });
        let none = run_write(&mut adapter, &site(), &req, &[], &[row]).unwrap();
        assert_eq!(none, Summary { subrc: 4, dbcnt: 0 });
    }

    #[test]
    fn modify_single_row_is_subrc_zero_with_dbcnt_one() {
        let mut adapter = FakeAdapter::with_writes(vec![Ok(1), Ok(0)]);
        let req = write_request(SqlOp::Modify(abap_sql_ir::ModifyOp {
            table: "ZFLUG".into(),
            rows: write_source(RowMode::Single),
        }));
        let row = value_row("LH", 1);
        let done = run_write(&mut adapter, &site(), &req, &[], std::slice::from_ref(&row)).unwrap();
        assert_eq!(done, Summary { subrc: 0, dbcnt: 1 });
        let none = run_write(&mut adapter, &site(), &req, &[], &[row]).unwrap();
        assert_eq!(none, Summary { subrc: 4, dbcnt: 0 });
    }

    #[test]
    fn modify_table_counts_processed_rows_and_is_subrc_four_when_not_all_were_processed() {
        let mut adapter = FakeAdapter::with_writes(vec![Ok(1), Ok(0)]);
        let req = write_request(SqlOp::Modify(abap_sql_ir::ModifyOp {
            table: "ZFLUG".into(),
            rows: write_source(RowMode::Table),
        }));
        let rows = vec![value_row("LH", 1), value_row("AF", 2)];
        let summary = run_write(&mut adapter, &site(), &req, &[], &rows).unwrap();
        assert_eq!(summary, Summary { subrc: 4, dbcnt: 1 });
    }

    #[test]
    fn single_row_statement_with_another_row_count_is_an_internal_error() {
        let mut adapter = FakeAdapter::with_writes(vec![]);
        let req = insert_request(RowMode::Single, false);
        let err = run_write(&mut adapter, &site(), &req, &[], &[]).unwrap_err();
        assert_eq!(
            err,
            "invalid SQL IR at line 9: INSERT needs exactly one row"
        );
    }

    #[test]
    fn run_write_rejects_a_select_statement() {
        let mut adapter = FakeAdapter::with_rows(vec![]);
        let req = request(ResultMode::Table, None, 8);
        let err = run_write(&mut adapter, &site(), &req, &[], &[]).unwrap_err();
        assert_eq!(err, "invalid SQL IR at line 9: not a write statement");
    }

    #[test]
    fn write_without_capability_is_reported_with_the_missing_names() {
        let mut adapter = FakeAdapter::with_writes(vec![]);
        adapter.caps = super::super::adapter::Capabilities::default();
        let req = insert_request(RowMode::Single, false);
        let err = run_write(&mut adapter, &site(), &req, &[], &[value_row("LH", 1)]).unwrap_err();
        assert_eq!(
            err,
            "capability missing at line 9 (connection DEFAULT): not supported by this adapter: insert"
        );
        assert!(adapter.seen_params.lock().unwrap().is_empty());
    }

    #[test]
    fn assignment_host_index_out_of_range_is_invalid_ir() {
        let mut adapter = FakeAdapter::with_writes(vec![]);
        let req = update_set_request();
        let SqlOp::Update(mut op) = req.op.clone() else {
            unreachable!("update request");
        };
        op.assignments[0].value = abap_sql_ir::Expr::Param { index: 3 };
        let req = write_request(SqlOp::Update(op));
        let err = run_write(&mut adapter, &site(), &req, &[ParamValue::Int(5)], &[]).unwrap_err();
        assert_eq!(
            err,
            "invalid SQL IR at line 9: host variable index 3 is out of range (1 given)"
        );
    }

    #[test]
    fn row_values_read_each_field_at_its_offset() {
        let mut buffer = vec![0u8; 8];
        buffer[0..4].copy_from_slice(
            &"LH"
                .encode_utf16()
                .flat_map(|u| u.to_ne_bytes())
                .collect::<Vec<u8>>(),
        );
        buffer[4..8].copy_from_slice(&180i32.to_le_bytes());
        let values = row_values(&site(), &write_source(RowMode::Single), &[&buffer]).unwrap();
        assert_eq!(values, vec![value_row("LH", 180)]);
    }

    #[test]
    fn row_conversion_error_names_line_row_and_field() {
        // Zwei Zeilen mit NUMC(4) an Offset 0 (Größe 8). Die zweite Zeile enthält kein Ziffernfeld.
        let source = RowSource {
            mode: RowMode::Table,
            fields: vec![RowField {
                column: "connid".into(),
                ty: AbapType::Char {
                    length: 4,
                    dtype: CharType::Numc,
                },
                offset: 0,
            }],
            size: 8,
        };
        let good: Vec<u8> = "0012"
            .encode_utf16()
            .flat_map(|u| u.to_ne_bytes())
            .collect();
        let bad: Vec<u8> = "1A"
            .encode_utf16()
            .flat_map(|u| u.to_ne_bytes())
            .chain([0, 0, 0, 0])
            .collect();
        let err = row_values(&site(), &source, &[&good, &bad]).unwrap_err();
        assert!(
            err.starts_with("conversion error at line 9 (field connid, row 2)"),
            "{err}"
        );
    }

    #[test]
    fn row_buffer_of_wrong_size_is_an_internal_error() {
        let buffer = vec![0u8; 6];
        let err = row_values(&site(), &write_source(RowMode::Single), &[&buffer]).unwrap_err();
        assert_eq!(
            err,
            "internal error: row size mismatch at line 9 (expected 8, got 6)"
        );
    }

    #[test]
    fn trace_line_shows_connection_operation_table_and_names_but_no_values() {
        let names = vec!["carrid".to_string(), "seatsmax".to_string()];
        let line = trace_line(
            "DEFAULT",
            "insert",
            "ZFLUG",
            &names,
            Some("INSERT INTO \"flug\" (\"carrid\", \"seats\") VALUES (?1, ?2)"),
        );
        assert_eq!(
            line,
            "[sql] connection=DEFAULT op=insert table=ZFLUG params=carrid,seatsmax sql=INSERT INTO \"flug\" (\"carrid\", \"seats\") VALUES (?1, ?2)"
        );
        assert!(!line.contains("LH"));
    }

    #[test]
    fn single_select_does_not_need_the_limit_capability() {
        // Der Kern begrenzt SELECT SINGLE selbst auf eine Zeile; die Fähigkeit `limit` ist dafür nicht nötig.
        let mut adapter = FakeAdapter::with_rows(vec![row("LH", 180)]);
        adapter.caps.limit = false;
        let req = request(ResultMode::Single, None, 8);
        let mut area = vec![0u8; 8];
        let summary =
            run_select(&mut adapter, &site(), &req, &[], Target::Work(&mut area)).unwrap();
        assert_eq!(summary, Summary { subrc: 0, dbcnt: 1 });
    }

    #[test]
    fn trace_line_without_table_or_names_uses_dashes() {
        assert_eq!(
            trace_line("DEFAULT", "commit", "", &[], None),
            "[sql] connection=DEFAULT op=commit table=- params=-"
        );
    }
}
