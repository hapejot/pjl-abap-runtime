//! SQL-Laufzeit für `SELECT` und die Schreibanweisungen (Phase 2). Der Codegen ruft die C-ABI-Funktionen dieses
//! Moduls auf: `abap_rt_sql_select`, `abap_rt_sql_execute` (Schreiben), `abap_rt_sql_commit`,
//! `abap_rt_sql_rollback`, `abap_rt_sql_finish` (Programmende) und die Cursor-Funktionen.
//!
//! LUW: Je Verbindung beginnt eine LUW beim ersten schreibenden Zugriff. `COMMIT WORK` schreibt alle offenen LUWs
//! fest, `ROLLBACK WORK` verwirft sie. Am Programmende werden offene LUWs zurückgerollt (`abap_rt_sql_finish`),
//! ebenso bei einem Laufzeitfehler (`fail`, nur wenn der Zustand gerade frei ist).
//!
//! Protokollierung: Ist die Umgebungsvariable `ABAPC_TRACE_SQL` gesetzt und nicht leer, schreibt die Laufzeit je
//! Anweisung eine Zeile nach stderr (Verbindung, Operation, Tabelle, Namen der Werte, bei SQLite den SQL-Text).
//! Feldwerte stehen nie darin. Der Compiler hat keinen Schalter dafür, Programme nehmen keine Argumente.
//!
//! Aufbau:
//! - `config`: Verbindungen und Tabellen aus der TOML-Datei (Suchreihenfolge, Pfade).
//! - `adapter`: Vertrag `SqlAdapter` und die Typen, die Adapter und Kern austauschen.
//! - `kernel`: Auflösung der Tabelle, Ausführung nach Ergebnisart, Übertragung in den Zielbereich.
//! - `value`: Hostvariablen lesen, ABAP-Felder schreiben (Padding, NUMC, Packed, Ganzzahl).
//! - `sqlite`, `external`: die beiden implementierten Adapter.
//!
//! Der Kern meldet Fehler als Rückgabe `Result<_, String>`. Erst die C-ABI-Funktionen dieses Moduls beenden
//! das Programm (`Runtime error: ...`, Status 1). Der IR-Text wird je `ir_id` einmal dekodiert.

pub mod adapter;
pub mod config;
pub mod external;
pub mod kernel;
pub mod sqlite;
pub mod value;

use std::collections::{BTreeSet, HashMap};
use std::io::Write;
use std::ptr::NonNull;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, TryLockError};

use abap_sql_ir::{Param, ResultMode, RowMode, RowSource, SqlOp, SqlRequest};

use self::adapter::{table_of, CursorId, ParamValue, SqlAdapter};
use self::config::Config;
use self::kernel::{Site, Summary, Target};
use crate::RtTable;

/// Laufzeitzustand: dekodierte IR, die Adapter je Verbindung und die Verbindungen mit offener LUW.
#[derive(Default)]
pub struct State {
    irs: IrCache,
    adapters: HashMap<String, Box<dyn SqlAdapter>>,
    /// Verbindungen mit offener LUW (Schlüssel wie in `kernel::connection_key`). Eine LUW beginnt beim ersten
    /// schreibenden Zugriff je Verbindung.
    luw: BTreeSet<String>,
}

/// Ende der LUWs einer Verbindung: festschreiben oder verwerfen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LuwEnd {
    Commit,
    Rollback,
}

/// Ein offener Cursor (`SELECT ... ENDSELECT`). Wird als `Box` an den Codegen übergeben.
pub struct CursorHandle {
    adapter_key: String,
    request: Arc<SqlRequest>,
    site: Site,
    cursor: CursorId,
    target_size: usize,
    count: i64,
}

/// Argumente einer Anweisung, wie sie der Codegen übergibt.
pub struct IrArgs {
    pub ir_id: u32,
    pub ir_json: *const u8,
    pub ir_len: u64,
    pub params: *const *mut u8,
    pub n_params: u64,
}

/// Dekodierte Anweisungen je `ir_id`. Jede Anweisung wird nur einmal dekodiert.
#[derive(Default)]
pub struct IrCache {
    entries: HashMap<u32, Arc<SqlRequest>>,
    decodes: u64,
}

impl IrCache {
    /// Gibt die gecachte Anweisung zurück oder dekodiert sie mit `decode` und merkt sie sich.
    pub fn get_or_decode<F>(&mut self, ir_id: u32, decode: F) -> Result<Arc<SqlRequest>, String>
    where
        F: FnOnce() -> Result<SqlRequest, String>,
    {
        if let Some(request) = self.entries.get(&ir_id) {
            return Ok(Arc::clone(request));
        }
        let request = Arc::new(decode()?);
        self.decodes += 1;
        self.entries.insert(ir_id, Arc::clone(&request));
        Ok(request)
    }

    /// Anzahl der tatsächlichen Dekodierungen.
    #[cfg(test)]
    pub fn decode_count(&self) -> u64 {
        self.decodes
    }
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static CONFIG: OnceLock<Result<Config, String>> = OnceLock::new();

fn lock_state() -> MutexGuard<'static, Option<State>> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Konfiguration des Programms, beim ersten SQL-Zugriff geladen und dann wiederverwendet.
fn config() -> Result<&'static Config, String> {
    CONFIG
        .get_or_init(config::load_default)
        .as_ref()
        .map_err(|e| e.clone())
}

/// Laufzeitfehler: Ausgabe leeren, offene LUWs zurückrollen, Meldung nach stderr, Status 1 (wie in `lib.rs`).
fn fail(message: &str) -> ! {
    let _ = std::io::stdout().flush();
    rollback_open_luws_quietly();
    eprintln!("Runtime error: {message}");
    std::process::exit(1)
}

/// Rollt offene LUWs beim Laufzeitfehler zurück (best effort, Fehler werden ignoriert). Der Mutex wird nicht
/// blockiert: hält ihn gerade ein anderer Aufruf, bleibt die Rückrollung aus.
fn rollback_open_luws_quietly() {
    let mut guard = match STATE.try_lock() {
        Ok(guard) => guard,
        Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
        Err(TryLockError::WouldBlock) => return,
    };
    if let Some(state) = guard.as_mut() {
        let _ = end_luws(state, LuwEnd::Rollback, false);
    }
}

/// Beendet die LUWs aller Verbindungen mit offener LUW. Der erste Fehler wird gemeldet, alle Verbindungen
/// werden trotzdem beendet. `trace` schreibt je Verbindung eine Protokollzeile (nur bei expliziten Anweisungen).
fn end_luws(state: &mut State, end: LuwEnd, trace: bool) -> Result<(), String> {
    let (op, word) = match end {
        LuwEnd::Commit => ("commit", "COMMIT WORK"),
        LuwEnd::Rollback => ("rollback", "ROLLBACK WORK"),
    };
    let mut first_error = None;
    // Eine Verbindung bleibt in der Menge, wenn ihr Ende fehlschlägt: der Fehlerpfad rollt sie dann zurück.
    let open: Vec<String> = state.luw.iter().cloned().collect();
    for key in open {
        if trace {
            kernel::trace_luw(&key, op);
        }
        let Some(adapter) = state.adapters.get_mut(&key) else {
            state.luw.remove(&key);
            continue;
        };
        let result = match end {
            LuwEnd::Commit => adapter.commit(),
            LuwEnd::Rollback => adapter.rollback(),
        };
        match result {
            Ok(()) => {
                state.luw.remove(&key);
            }
            Err(error) => {
                first_error.get_or_insert_with(|| {
                    format!("db error at {word} (connection {key}): {}", error.message)
                });
            }
        }
    }
    match first_error {
        Some(message) => Err(message),
        None => Ok(()),
    }
}

/// `COMMIT WORK` für alle offenen LUWs. Ohne offene LUW ist das kein Fehler.
pub fn commit_in(state: &mut State) -> Result<(), String> {
    end_luws(state, LuwEnd::Commit, true)
}

/// `ROLLBACK WORK` für alle offenen LUWs. Ohne offene LUW ist das kein Fehler.
pub fn rollback_in(state: &mut State) -> Result<(), String> {
    end_luws(state, LuwEnd::Rollback, true)
}

/// Programmende: offene LUWs werden zurückgerollt, Fehler werden ignoriert.
pub fn finish_in(state: &mut State) {
    let _ = end_luws(state, LuwEnd::Rollback, false);
}

fn invalid(line: u32, reason: impl AsRef<str>) -> String {
    format!("invalid SQL IR at line {line}: {}", reason.as_ref())
}

/// Dekodiert den IR-Text und prüft die Formatversion.
pub fn decode_text(text: &str) -> Result<SqlRequest, String> {
    abap_sql_ir::decode(text)
}

/// Liest den IR-Text aus dem Speicher des Programms.
///
/// # Safety
/// `ir_json` zeigt auf `ir_len` gültige Bytes.
unsafe fn decode_raw(ir_json: *const u8, ir_len: u64) -> Result<SqlRequest, String> {
    if ir_json.is_null() {
        return Err("invalid SQL IR: missing IR text".to_string());
    }
    let bytes = std::slice::from_raw_parts(ir_json, ir_len as usize);
    let text = std::str::from_utf8(bytes).map_err(|e| format!("invalid SQL IR: {e}"))?;
    decode_text(text)
}

/// Hostparameter der Anweisung. Die Werte der Zeilenquelle von `INSERT`, `UPDATE FROM`, `DELETE FROM` und
/// `MODIFY` gehören nicht dazu: sie stehen im Zielbereich (siehe `row_buffers`).
fn host_params(op: &SqlOp) -> &[Param] {
    match op {
        SqlOp::Select(select) => &select.params,
        SqlOp::Update(update) => &update.params,
        SqlOp::Delete(delete) => &delete.params,
        SqlOp::Insert(_) | SqlOp::Modify(_) | SqlOp::Commit | SqlOp::Rollback => &[],
    }
}

/// Liest alle Hostvariablen nach den Typen aus der IR.
///
/// # Safety
/// `params` zeigt auf `n_params` gültige Zeiger, und jeder zeigt auf einen Speicher mit der Größe des Typs.
unsafe fn read_host_params(
    request: &SqlRequest,
    params: *const *mut u8,
    n_params: u64,
) -> Result<Vec<ParamValue>, String> {
    let host = host_params(&request.op);
    let expected = host.len();
    if n_params as usize != expected {
        return Err(invalid(
            request.line,
            format!("expected {expected} host variables, got {n_params}"),
        ));
    }
    let mut values = Vec::with_capacity(expected);
    if expected == 0 {
        return Ok(values);
    }
    if params.is_null() {
        return Err(invalid(request.line, "missing host variable list"));
    }
    for (index, param) in host.iter().enumerate() {
        let pointer = *params.add(index);
        if pointer.is_null() {
            return Err(format!(
                "missing host variable {} at line {}",
                param.name, request.line
            ));
        }
        let bytes = std::slice::from_raw_parts(pointer as *const u8, value::field_size(&param.ty));
        let value = value::host_value(bytes, &param.ty).map_err(|reason| {
            format!(
                "invalid host variable {} at line {}: {reason}",
                param.name, request.line
            )
        })?;
        values.push(value);
    }
    Ok(values)
}

/// Arbeitsbereich aus Zeiger und Größe. Größe 0 ergibt einen leeren Bereich.
///
/// # Safety
/// Bei `size > 0` zeigt `ptr` auf `size` gültige Bytes, die während der Verwendung nicht anders benutzt werden.
unsafe fn work_area<'a>(ptr: *mut u8, size: u64, line: u32) -> Result<&'a mut [u8], String> {
    if size == 0 {
        return Ok(std::slice::from_raw_parts_mut(
            NonNull::dangling().as_ptr(),
            0,
        ));
    }
    if ptr.is_null() {
        return Err(format!(
            "internal error: missing target area at line {line}"
        ));
    }
    Ok(std::slice::from_raw_parts_mut(ptr, size as usize))
}

/// Die Tabelle für `INTO TABLE` bzw. `APPENDING TABLE`.
fn table_handle(ptr: *mut u8, line: u32) -> Result<*mut RtTable, String> {
    if ptr.is_null() {
        return Err(format!(
            "internal error: missing internal table at line {line}"
        ));
    }
    Ok(ptr as *mut RtTable)
}

/// Ort der Anweisung (Zeile, Verbindung, Tabelle). Prüft, dass Tabelle und Verbindung konfiguriert sind.
fn site_of(config: &Config, request: &SqlRequest) -> Result<Site, String> {
    let table = table_of(request).unwrap_or("");
    let (_, connection) = kernel::resolve(config, table, request.line)?;
    Ok(Site {
        line: request.line,
        connection: connection.name.clone(),
        table: table.to_string(),
    })
}

/// Adapter der Verbindung des Orts. Legt ihn beim ersten Zugriff an.
fn adapter_for<'a>(
    adapters: &'a mut HashMap<String, Box<dyn SqlAdapter>>,
    config: &Config,
    site: &Site,
) -> Result<&'a mut Box<dyn SqlAdapter>, String> {
    let (_, connection) = kernel::resolve(config, &site.table, site.line)?;
    let key = kernel::connection_key(&connection.name);
    if !adapters.contains_key(&key) {
        let adapter = kernel::open_adapter(config, connection, site)?;
        adapters.insert(key.clone(), adapter);
    }
    adapters
        .get_mut(&key)
        .ok_or_else(|| "internal error: adapter missing".to_string())
}

/// `SELECT` ohne Cursor: Einzelsatz, `INTO TABLE`, `APPENDING TABLE`. Gibt SY-SUBRC und SY-DBCNT zurück.
///
/// # Safety
/// `target` zeigt je nach Ergebnisart auf den Arbeitsbereich (`target_size` Bytes) oder auf eine `RtTable`.
pub unsafe fn select_in(
    state: &mut State,
    config: &Config,
    args: IrArgs,
    target: *mut u8,
) -> Result<Summary, String> {
    let request = state
        .irs
        .get_or_decode(args.ir_id, || decode_raw(args.ir_json, args.ir_len))?;
    let SqlOp::Select(select) = &request.op else {
        return Err(invalid(request.line, "not a SELECT statement"));
    };
    let host = read_host_params(&request, args.params, args.n_params)?;
    let target = match select.result {
        ResultMode::Single => Target::Work(work_area(target, select.target_size, request.line)?),
        ResultMode::Table | ResultMode::Append => {
            Target::Table(table_handle(target, request.line)?)
        }
        ResultMode::Loop => {
            return Err(invalid(request.line, "loop result needs a cursor"));
        }
    };
    let site = site_of(config, &request)?;
    let adapter = adapter_for(&mut state.adapters, config, &site)?;
    kernel::run_select(&mut **adapter, &site, &request, &host, target)
}

/// Zeilen einer Zeilenquelle als Bytes. `FROM wa` liest den Arbeitsbereich, `FROM TABLE` jede Zeile der
/// internen Tabelle. Die Größe der Zeilen muss zur Zeilenquelle passen.
///
/// # Safety
/// `target` zeigt je nach `source.mode` auf den Arbeitsbereich (`source.size` Bytes) oder auf eine `RtTable`.
unsafe fn row_buffers<'a>(
    source: &RowSource,
    target: *mut u8,
    line: u32,
) -> Result<Vec<&'a [u8]>, String> {
    match source.mode {
        RowMode::Single => {
            let area: &[u8] = work_area(target, source.size, line)?;
            Ok(vec![area])
        }
        RowMode::Table => {
            let table = table_handle(target, line)?;
            let elem_bytes = (*table).elem_bytes;
            if elem_bytes != source.size as usize {
                return Err(format!(
                    "internal error: internal table row size {elem_bytes} does not match the row source ({}) at line {line}",
                    source.size
                ));
            }
            let count = crate::abap_rt_table_count(table);
            let mut buffers = Vec::with_capacity(count.max(0) as usize);
            for index in 0..count {
                let row = crate::abap_rt_table_row(table, index);
                if row.is_null() {
                    return Err(format!("internal error: missing table row at line {line}"));
                }
                buffers.push(std::slice::from_raw_parts(row as *const u8, elem_bytes));
            }
            Ok(buffers)
        }
    }
}

/// Die Zeilenquelle der Schreibanweisung, falls sie eine hat.
fn row_source_of(op: &SqlOp) -> Option<&RowSource> {
    match op {
        SqlOp::Insert(insert) => Some(&insert.rows),
        SqlOp::Update(update) => update.rows.as_ref(),
        SqlOp::Delete(delete) => delete.rows.as_ref(),
        SqlOp::Modify(modify) => Some(&modify.rows),
        SqlOp::Select(_) | SqlOp::Commit | SqlOp::Rollback => None,
    }
}

/// Schreibanweisung (`INSERT`, `UPDATE`, `DELETE`, `MODIFY`) mit Summary für SY-SUBRC und SY-DBCNT. Eine IR für
/// `COMMIT` oder `ROLLBACK` wird wie der Aufruf der C-Funktionen behandelt. Die erste schreibende Anweisung je
/// Verbindung beginnt die LUW.
///
/// # Safety
/// `target` zeigt bei einer Zeilenquelle auf den Arbeitsbereich oder die interne Tabelle (siehe `row_buffers`).
/// Ohne Zeilenquelle wird `target` nicht gelesen.
pub unsafe fn write_in(
    state: &mut State,
    config: &Config,
    args: IrArgs,
    target: *mut u8,
) -> Result<Summary, String> {
    let request = state
        .irs
        .get_or_decode(args.ir_id, || decode_raw(args.ir_json, args.ir_len))?;
    match &request.op {
        SqlOp::Select(_) => {
            return Err(format!(
                "internal error: SELECT passed to abap_rt_sql_execute at line {}",
                request.line
            ));
        }
        SqlOp::Commit => {
            commit_in(state)?;
            return Ok(Summary { subrc: 0, dbcnt: 0 });
        }
        SqlOp::Rollback => {
            rollback_in(state)?;
            return Ok(Summary { subrc: 0, dbcnt: 0 });
        }
        SqlOp::Insert(_) | SqlOp::Update(_) | SqlOp::Delete(_) | SqlOp::Modify(_) => {}
    }
    let host = read_host_params(&request, args.params, args.n_params)?;
    let site = site_of(config, &request)?;
    let rows = match row_source_of(&request.op) {
        Some(source) => {
            let buffers = row_buffers(source, target, request.line)?;
            kernel::row_values(&site, source, &buffers)?
        }
        None => Vec::new(),
    };
    let adapter = adapter_for(&mut state.adapters, config, &site)?;
    let key = kernel::connection_key(&site.connection);
    if !state.luw.contains(&key) {
        adapter
            .begin()
            .map_err(|e| kernel::format_sql_error(&e, &site))?;
        state.luw.insert(key);
    }
    kernel::run_write(&mut **adapter, &site, &request, &host, &rows)
}

/// Öffnet einen Cursor für `SELECT ... ENDSELECT`.
///
/// # Safety
/// Die Argumente in `args` müssen gültig sein (siehe `select_in`).
pub unsafe fn cursor_open_in(
    state: &mut State,
    config: &Config,
    args: IrArgs,
) -> Result<CursorHandle, String> {
    let request = state
        .irs
        .get_or_decode(args.ir_id, || decode_raw(args.ir_json, args.ir_len))?;
    let SqlOp::Select(select) = &request.op else {
        return Err(invalid(request.line, "not a SELECT statement"));
    };
    let target_size = select.target_size as usize;
    let host = read_host_params(&request, args.params, args.n_params)?;
    let site = site_of(config, &request)?;
    let adapter = adapter_for(&mut state.adapters, config, &site)?;
    let cursor = kernel::open_cursor(&mut **adapter, &site, &request, &host)?;
    Ok(CursorHandle {
        adapter_key: kernel::connection_key(&site.connection),
        request,
        site,
        cursor,
        target_size,
        count: 0,
    })
}

/// Holt die nächste Zeile in den Arbeitsbereich. Gibt `(gefunden, Anzahl gelesener Zeilen)` zurück.
///
/// # Safety
/// `handle` ist ein Handle aus `cursor_open_in`, `target` zeigt auf `target_size` Bytes.
pub unsafe fn cursor_fetch_in(
    state: &mut State,
    handle: &mut CursorHandle,
    target: *mut u8,
) -> Result<(bool, i64), String> {
    let adapter = state
        .adapters
        .get_mut(&handle.adapter_key)
        .ok_or_else(|| "internal error: no adapter for open cursor".to_string())?;
    let work = work_area(target, handle.target_size as u64, handle.site.line)?;
    let found = kernel::fetch_row(
        &mut **adapter,
        &handle.site,
        &handle.request,
        handle.cursor,
        work,
        &mut handle.count,
    )?;
    Ok((found, handle.count))
}

/// SY-SUBRC für den geschlossenen Cursor: 0, wenn mindestens eine Zeile gelesen wurde, sonst 4.
pub fn cursor_close_in(state: &mut State, handle: CursorHandle) -> i32 {
    if let Some(adapter) = state.adapters.get_mut(&handle.adapter_key) {
        adapter.close_cursor(handle.cursor);
    }
    if handle.count > 0 {
        0
    } else {
        4
    }
}

/// `SELECT` ohne Cursor. Gibt SY-SUBRC zurück (0 bei Treffer, 4 ohne Treffer). `dbcnt_out` erhält SY-DBCNT.
/// Laufzeitfehler beenden das Programm.
///
/// # Safety
/// Siehe `select_in`. `ir_json` zeigt auf `ir_len` Bytes UTF-8, `params` auf `n_params` Zeiger,
/// `dbcnt_out` ist null oder ein gültiger Zeiger.
#[no_mangle]
pub unsafe extern "C" fn abap_rt_sql_select(
    ir_id: u32,
    ir_json: *const u8,
    ir_len: u64,
    params: *const *mut u8,
    n_params: u64,
    target: *mut u8,
    dbcnt_out: *mut i64,
) -> i32 {
    let args = IrArgs {
        ir_id,
        ir_json,
        ir_len,
        params,
        n_params,
    };
    let result = match config() {
        Ok(config) => {
            let mut guard = lock_state();
            let state = guard.get_or_insert_with(State::default);
            // SAFETY: die Aufrufer-Zusicherungen gelten unverändert für diese Funktion.
            unsafe { select_in(state, config, args, target) }
        }
        Err(message) => Err(message),
    };
    match result {
        Ok(summary) => {
            if !dbcnt_out.is_null() {
                // SAFETY: nicht null, die Aufrufer-Zusicherung gilt.
                unsafe { *dbcnt_out = summary.dbcnt };
            }
            summary.subrc
        }
        Err(message) => fail(&message),
    }
}

/// Öffnet einen Cursor für `SELECT ... ENDSELECT` (Ergebnisart `loop`). Liefert ein Handle.
///
/// # Safety
/// Siehe `abap_rt_sql_select`. Das Handle muss mit `abap_rt_sql_cursor_close` freigegeben werden.
#[no_mangle]
pub unsafe extern "C" fn abap_rt_sql_cursor_open(
    ir_id: u32,
    ir_json: *const u8,
    ir_len: u64,
    params: *const *mut u8,
    n_params: u64,
) -> *mut u8 {
    let args = IrArgs {
        ir_id,
        ir_json,
        ir_len,
        params,
        n_params,
    };
    let result = match config() {
        Ok(config) => {
            let mut guard = lock_state();
            let state = guard.get_or_insert_with(State::default);
            // SAFETY: die Aufrufer-Zusicherungen gelten unverändert für diese Funktion.
            unsafe { cursor_open_in(state, config, args) }
        }
        Err(message) => Err(message),
    };
    match result {
        Ok(handle) => Box::into_raw(Box::new(handle)) as *mut u8,
        Err(message) => fail(&message),
    }
}

/// Holt die nächste Zeile. Gibt 1 zurück, wenn eine Zeile in `target` steht, 0 am Ende.
/// `dbcnt_out` erhält die Zahl der bisher gelesenen Zeilen.
///
/// # Safety
/// `cur` ist ein Handle aus `abap_rt_sql_cursor_open` und noch nicht geschlossen. `target` zeigt auf den
/// Arbeitsbereich mit der Größe aus der IR. `dbcnt_out` ist null oder gültig.
#[no_mangle]
pub unsafe extern "C" fn abap_rt_sql_cursor_fetch(
    cur: *mut u8,
    target: *mut u8,
    dbcnt_out: *mut i64,
) -> i32 {
    let Some(handle) = (cur as *mut CursorHandle).as_mut() else {
        fail("internal error: no open cursor");
    };
    let result = {
        let mut guard = lock_state();
        let state = guard.get_or_insert_with(State::default);
        // SAFETY: die Aufrufer-Zusicherungen gelten unverändert für diese Funktion.
        unsafe { cursor_fetch_in(state, handle, target) }
    };
    match result {
        Ok((found, count)) => {
            if !dbcnt_out.is_null() {
                // SAFETY: nicht null, die Aufrufer-Zusicherung gilt.
                unsafe { *dbcnt_out = count };
            }
            i32::from(found)
        }
        Err(message) => fail(&message),
    }
}

/// Schreibanweisung (`INSERT`, `UPDATE`, `DELETE`, `MODIFY`). Gibt SY-SUBRC zurück und schreibt SY-DBCNT nach
/// `dbcnt_out`. Laufzeitfehler beenden das Programm.
///
/// Der Zielbereich `target` ist der Arbeitsbereich der Zeilenquelle (`FROM wa`) bzw. die `RtTable` (`FROM TABLE`).
/// Ohne Zeilenquelle (Form `SET` und `WHERE`) darf `target` null sein. Die Werte der Zeilenquelle kommen nicht aus
/// `params`, nur die Hostvariablen der Anweisung.
///
/// # Safety
/// `ir_json` zeigt auf `ir_len` gültige Bytes UTF-8, `params` auf `n_params` Zeiger, `target` wie oben,
/// `dbcnt_out` ist null oder ein gültiger Zeiger.
#[no_mangle]
pub unsafe extern "C" fn abap_rt_sql_execute(
    ir_id: u32,
    ir_json: *const u8,
    ir_len: u64,
    params: *const *mut u8,
    n_params: u64,
    target: *mut u8,
    dbcnt_out: *mut i64,
) -> i32 {
    let args = IrArgs {
        ir_id,
        ir_json,
        ir_len,
        params,
        n_params,
    };
    let result = match config() {
        Ok(config) => {
            let mut guard = lock_state();
            let state = guard.get_or_insert_with(State::default);
            // SAFETY: die Aufrufer-Zusicherungen gelten unverändert für diese Funktion.
            unsafe { write_in(state, config, args, target) }
        }
        Err(message) => Err(message),
    };
    match result {
        Ok(summary) => {
            if !dbcnt_out.is_null() {
                // SAFETY: nicht null, die Aufrufer-Zusicherung gilt.
                unsafe { *dbcnt_out = summary.dbcnt };
            }
            summary.subrc
        }
        Err(message) => fail(&message),
    }
}

/// `COMMIT WORK`: schreibt die offenen LUWs aller Verbindungen fest. Ohne offene LUW passiert nichts.
/// Gibt 0 zurück; ein Fehler beendet das Programm.
#[no_mangle]
pub extern "C" fn abap_rt_sql_commit() -> i32 {
    let result = {
        let mut guard = lock_state();
        let state = guard.get_or_insert_with(State::default);
        commit_in(state)
    };
    match result {
        Ok(()) => 0,
        Err(message) => fail(&message),
    }
}

/// `ROLLBACK WORK`: verwirft die offenen LUWs aller Verbindungen. Gibt 0 zurück; ein Fehler beendet das Programm.
#[no_mangle]
pub extern "C" fn abap_rt_sql_rollback() -> i32 {
    let result = {
        let mut guard = lock_state();
        let state = guard.get_or_insert_with(State::default);
        rollback_in(state)
    };
    match result {
        Ok(()) => 0,
        Err(message) => fail(&message),
    }
}

/// Programmende: rollt alle offenen LUWs zurück. Fehler werden ignoriert. Der Codegen ruft die Funktion am Ende
/// des Programms auf, auch wenn kein SQL-Zugriff stattfand.
#[no_mangle]
pub extern "C" fn abap_rt_sql_finish() {
    let mut guard = lock_state();
    if let Some(state) = guard.as_mut() {
        finish_in(state);
    }
}

/// Schließt den Cursor und gibt das Handle frei. Gibt SY-SUBRC zurück: 0, wenn mindestens eine Zeile
/// gelesen wurde, sonst 4. Der Codegen ruft es bei jedem Schleifenende auf, auch bei EXIT und RETURN.
///
/// # Safety
/// `cur` ist ein Handle aus `abap_rt_sql_cursor_open`. Nach dem Aufruf darf es nicht mehr benutzt werden.
#[no_mangle]
pub unsafe extern "C" fn abap_rt_sql_cursor_close(cur: *mut u8) -> i32 {
    if cur.is_null() {
        return 4;
    }
    let handle = Box::from_raw(cur as *mut CursorHandle);
    let mut guard = lock_state();
    let state = guard.get_or_insert_with(State::default);
    cursor_close_in(state, *handle)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use abap_sql_ir::{AbapType, CharType, Param, ResultColumn, RowField, SelectOp, IR_VERSION};

    use super::*;

    fn temp_path(tag: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!(
            "abap-rt-sql-{tag}-{}-{nanos}.sqlite",
            std::process::id()
        ))
    }

    /// Datenbankdatei mit zwei Zeilen für die Ende-zu-Ende-Tests.
    fn database_file(tag: &str) -> std::path::PathBuf {
        let path = temp_path(tag);
        let conn = rusqlite::Connection::open(&path).expect("create database");
        conn.execute_batch(
            "CREATE TABLE flug (carrid TEXT, seats INTEGER);
             INSERT INTO flug VALUES ('LH', 180);
             INSERT INTO flug VALUES ('AF', 220);",
        )
        .expect("populate");
        path
    }

    fn config_for(path: &std::path::Path) -> Config {
        let text = format!(
            "[connections.DEFAULT]\nadapter = \"sqlite\"\npath = {:?}\n\n[tables.ZFLUG]\nconnection = \"DEFAULT\"\ntarget = \"flug\"\n",
            path.to_str().expect("utf-8 path")
        );
        Config::from_toml(&text, &std::env::temp_dir()).expect("config parses")
    }

    /// `SELECT carrid, seats FROM zflug WHERE carrid = @lv_carrid`. Zielgröße 8 Bytes.
    fn select_request(result: ResultMode, target_size: u64, up_to: Option<u64>) -> SqlRequest {
        use abap_sql_ir::{CmpOp, Expr};
        SqlRequest {
            ir_version: IR_VERSION,
            line: 12,
            connection: "DEFAULT".into(),
            op: SqlOp::Select(SelectOp {
                table: "ZFLUG".into(),
                columns: vec![
                    ResultColumn {
                        name: "carrid".into(),
                        source: AbapType::Char {
                            length: 2,
                            dtype: CharType::Char,
                        },
                        target: AbapType::Char {
                            length: 2,
                            dtype: CharType::Char,
                        },
                        offset: 0,
                    },
                    ResultColumn {
                        name: "seats".into(),
                        source: AbapType::Int,
                        target: AbapType::Int,
                        offset: 4,
                    },
                ],
                filter: Some(Expr::Compare {
                    op: CmpOp::Ne,
                    left: Box::new(Expr::Column {
                        name: "carrid".into(),
                    }),
                    right: Box::new(Expr::Param { index: 0 }),
                }),
                order_by: vec![],
                up_to,
                result,
                target_size,
                target_fields: vec![],
                params: vec![Param {
                    name: "lv_skip".into(),
                    ty: AbapType::Char {
                        length: 2,
                        dtype: CharType::Char,
                    },
                }],
            }),
        }
    }

    /// Parameterzeiger für eine Hostvariable `CHAR 2` mit dem Wert `text`.
    fn host_char(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(|u| u.to_ne_bytes()).collect()
    }

    fn units(bytes: &[u8]) -> String {
        let u: Vec<u16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| u16::from_ne_bytes(*p))
            .collect();
        String::from_utf16(&u).unwrap()
    }

    fn args_for(ir_id: u32, json: &str, host: &mut [*mut u8]) -> IrArgs {
        IrArgs {
            ir_id,
            ir_json: json.as_ptr(),
            ir_len: json.len() as u64,
            params: host.as_ptr(),
            n_params: host.len() as u64,
        }
    }

    #[test]
    fn ir_cache_decodes_each_ir_id_only_once() {
        let mut cache = IrCache::default();
        let calls = Cell::new(0);
        let make = || {
            calls.set(calls.get() + 1);
            Ok(select_request(ResultMode::Single, 8, None))
        };
        let first = cache.get_or_decode(5, make).unwrap();
        let second = cache
            .get_or_decode(5, || Err("must not decode again".to_string()))
            .unwrap();
        assert_eq!(calls.get(), 1);
        assert_eq!(cache.decode_count(), 1);
        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn ir_cache_does_not_store_a_failed_decode() {
        let mut cache = IrCache::default();
        assert!(cache
            .get_or_decode(6, || Err("broken".to_string()))
            .is_err());
        assert_eq!(cache.decode_count(), 0);
        assert!(cache
            .get_or_decode(6, || Ok(select_request(ResultMode::Single, 8, None)))
            .is_ok());
    }

    #[test]
    fn ir_with_wrong_version_is_rejected() {
        let mut request = select_request(ResultMode::Single, 8, None);
        request.ir_version = IR_VERSION + 1;
        let text = abap_sql_ir::encode(&request);
        let err = decode_text(&text).unwrap_err();
        assert!(err.contains("version"), "{err}");
    }

    #[test]
    fn broken_ir_text_is_rejected_with_message() {
        let err = decode_text("{\"ir_version\":1").unwrap_err();
        assert!(err.starts_with("invalid SQL IR"), "{err}");
    }

    #[test]
    fn null_ir_pointer_is_rejected() {
        let err = unsafe { decode_raw(std::ptr::null(), 0) }.unwrap_err();
        assert!(err.contains("missing IR text"), "{err}");
    }

    #[test]
    fn host_parameters_are_read_by_their_declared_types() {
        let request = select_request(ResultMode::Single, 8, None);
        let mut text = host_char("LH");
        let ptrs = [text.as_mut_ptr()];
        let values = unsafe { read_host_params(&request, ptrs.as_ptr(), 1) }.unwrap();
        assert_eq!(values, vec![ParamValue::Text("LH".into())]);
    }

    #[test]
    fn host_parameter_count_mismatch_is_rejected() {
        let request = select_request(ResultMode::Single, 8, None);
        let err = unsafe { read_host_params(&request, std::ptr::null(), 0) }.unwrap_err();
        assert!(err.contains("expected 1 host variables, got 0"), "{err}");
    }

    #[test]
    fn packed_host_parameter_is_read_as_exact_decimal_text() {
        let mut request = select_request(ResultMode::Single, 8, None);
        let SqlOp::Select(select) = &mut request.op else {
            unreachable!("select request")
        };
        select.params = vec![Param {
            name: "lv_amount".into(),
            ty: AbapType::Packed { bytes: 2, scale: 2 },
        }];
        // -1.25: Nibbles 1, 2, 5 und Vorzeichen Minus.
        let mut field = [0x12u8, 0x5D];
        let ptrs = [field.as_mut_ptr()];
        let values = unsafe { read_host_params(&request, ptrs.as_ptr(), 1) }.unwrap();
        assert_eq!(values, vec![ParamValue::Text("-1.25".into())]);
    }

    #[test]
    fn select_single_reads_the_database_file_into_the_work_area() {
        let path = database_file("single");
        let config = config_for(&path);
        let json = abap_sql_ir::encode(&select_request(ResultMode::Single, 8, None));
        let mut state = State::default();
        let mut skip = host_char("XX");
        let mut ptrs = [skip.as_mut_ptr()];
        let mut work = vec![0u8; 8];
        let args = args_for(101, &json, &mut ptrs);
        let summary = unsafe { select_in(&mut state, &config, args, work.as_mut_ptr()) }.unwrap();
        assert_eq!(summary, Summary { subrc: 0, dbcnt: 1 });
        assert_eq!(units(&work[0..4]), "LH");
        assert_eq!(i32::from_le_bytes(work[4..8].try_into().unwrap()), 180);
        drop(state);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn select_table_mode_fills_the_internal_table() {
        let path = database_file("table");
        let config = config_for(&path);
        let json = abap_sql_ir::encode(&select_request(ResultMode::Table, 8, None));
        let mut state = State::default();
        let table = crate::abap_rt_table_new(8);
        let mut skip = host_char("XX");
        let mut ptrs = [skip.as_mut_ptr()];
        let args = args_for(103, &json, &mut ptrs);
        let summary = unsafe { select_in(&mut state, &config, args, table as *mut u8) }.unwrap();
        assert_eq!(summary, Summary { subrc: 0, dbcnt: 2 });
        assert_eq!(unsafe { crate::abap_rt_table_count(table) }, 2);
        unsafe { drop(Box::from_raw(table)) };
        drop(state);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn select_with_up_to_one_row_reads_only_the_first_row_for_table_mode() {
        let path = database_file("upto");
        let config = config_for(&path);
        let json = abap_sql_ir::encode(&select_request(ResultMode::Table, 8, Some(1)));
        let mut state = State::default();
        let table = crate::abap_rt_table_new(8);
        let mut skip = host_char("XX");
        let mut ptrs = [skip.as_mut_ptr()];
        let args = args_for(104, &json, &mut ptrs);
        let summary = unsafe { select_in(&mut state, &config, args, table as *mut u8) }.unwrap();
        assert_eq!(summary.dbcnt, 1);
        unsafe { drop(Box::from_raw(table)) };
        drop(state);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn loop_result_is_rejected_by_select() {
        let path = database_file("loop-select");
        let config = config_for(&path);
        let json = abap_sql_ir::encode(&select_request(ResultMode::Loop, 8, None));
        let mut state = State::default();
        let mut skip = host_char("XX");
        let mut ptrs = [skip.as_mut_ptr()];
        let mut work = vec![0u8; 8];
        let args = args_for(105, &json, &mut ptrs);
        let err = unsafe { select_in(&mut state, &config, args, work.as_mut_ptr()) }.unwrap_err();
        assert!(err.contains("needs a cursor"), "{err}");
        drop(state);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn cursor_reads_rows_one_by_one_and_closes_with_subrc_zero() {
        let path = database_file("cursor");
        let config = config_for(&path);
        let json = abap_sql_ir::encode(&select_request(ResultMode::Loop, 8, None));
        let mut state = State::default();
        let mut skip = host_char("XX");
        let mut ptrs = [skip.as_mut_ptr()];
        let args = args_for(106, &json, &mut ptrs);
        let mut handle = unsafe { cursor_open_in(&mut state, &config, args) }.unwrap();
        let mut work = vec![0u8; 8];
        let (found, count) =
            unsafe { cursor_fetch_in(&mut state, &mut handle, work.as_mut_ptr()) }.unwrap();
        assert_eq!((found, count), (true, 1));
        assert_eq!(units(&work[0..4]), "LH");
        let (found, count) =
            unsafe { cursor_fetch_in(&mut state, &mut handle, work.as_mut_ptr()) }.unwrap();
        assert_eq!((found, count), (true, 2));
        assert_eq!(units(&work[0..4]), "AF");
        let (found, count) =
            unsafe { cursor_fetch_in(&mut state, &mut handle, work.as_mut_ptr()) }.unwrap();
        assert_eq!((found, count), (false, 2));
        assert_eq!(cursor_close_in(&mut state, handle), 0);
        drop(state);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn cursor_closed_without_rows_returns_subrc_four() {
        let path = database_file("cursor-empty");
        let config = config_for(&path);
        let json = abap_sql_ir::encode(&select_request(ResultMode::Loop, 8, None));
        let mut state = State::default();
        let mut skip = host_char("XX");
        let mut ptrs = [skip.as_mut_ptr()];
        let args = args_for(107, &json, &mut ptrs);
        let handle = unsafe { cursor_open_in(&mut state, &config, args) }.unwrap();
        // Ohne fetch geschlossen: keine Zeile gelesen.
        assert_eq!(cursor_close_in(&mut state, handle), 4);
        drop(state);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn cursor_open_needs_loop_result() {
        let path = database_file("cursor-wrong");
        let config = config_for(&path);
        let json = abap_sql_ir::encode(&select_request(ResultMode::Single, 8, None));
        let mut state = State::default();
        let mut skip = host_char("XX");
        let mut ptrs = [skip.as_mut_ptr()];
        let args = args_for(108, &json, &mut ptrs);
        let err = unsafe { cursor_open_in(&mut state, &config, args) }
            .err()
            .expect("error");
        assert!(err.contains("needs a loop result"), "{err}");
        drop(state);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn unconfigured_table_is_reported_before_any_access() {
        let config = Config::default();
        let json = abap_sql_ir::encode(&select_request(ResultMode::Single, 8, None));
        let mut state = State::default();
        let mut skip = host_char("XX");
        let mut ptrs = [skip.as_mut_ptr()];
        let mut work = vec![0u8; 8];
        let args = args_for(109, &json, &mut ptrs);
        let err = unsafe { select_in(&mut state, &config, args, work.as_mut_ptr()) }.unwrap_err();
        assert_eq!(err, "table ZFLUG is not configured (line 12)");
    }

    #[test]
    fn target_area_of_zero_size_is_an_empty_slice() {
        let slice = unsafe { work_area(std::ptr::null_mut(), 0, 1) }.unwrap();
        assert!(slice.is_empty());
        assert!(unsafe { work_area(std::ptr::null_mut(), 4, 1) }.is_err());
    }

    // ---- Schreibanweisungen, LUW und Dubletten (Phase 2) ----

    /// Schreibbare Datenbankdatei: Tabelle `flug` mit Schlüssel (carrid, connid) und drei Zeilen.
    fn write_file(tag: &str) -> std::path::PathBuf {
        let path = temp_path(tag);
        let conn = rusqlite::Connection::open(&path).expect("create database");
        conn.execute_batch(
            "CREATE TABLE flug (carrid TEXT, connid TEXT, seats INTEGER, fldate TEXT, PRIMARY KEY (carrid, connid));
             INSERT INTO flug VALUES ('LH', '0400', 180, '20260101');
             INSERT INTO flug VALUES ('AF', '0010', 220, '20260103');
             INSERT INTO flug VALUES ('LH', '0010', 90, '20260102');",
        )
        .expect("populate");
        path
    }

    /// Konfiguration mit Schlüssel `CARRID, CONNID` für die Tabelle ZFLUG.
    fn keyed_config_for(path: &std::path::Path) -> Config {
        let text = format!(
            "[connections.DEFAULT]\nadapter = \"sqlite\"\npath = {:?}\n\n[tables.ZFLUG]\nconnection = \"DEFAULT\"\ntarget = \"flug\"\nkey = [\"CARRID\", \"CONNID\"]\n",
            path.to_str().expect("utf-8 path")
        );
        Config::from_toml(&text, &std::env::temp_dir()).expect("config parses")
    }

    /// UTF-16 in nativer Byte-Reihenfolge, auf `units` Einheiten mit Leerzeichen aufgefüllt.
    fn utf16_padded(text: &str, units: usize) -> Vec<u8> {
        let mut out: Vec<u16> = text.encode_utf16().collect();
        out.resize(units, 0x20);
        out.iter().flat_map(|u| u.to_ne_bytes()).collect()
    }

    /// Zeilenquelle `carrid` (CHAR 2, Offset 0), `connid` (NUMC 4, Offset 4), `seats` (INT, Offset 12),
    /// `fldate` (CHAR 8, Offset 16). Zeilengröße 32.
    fn flug_source(mode: RowMode) -> RowSource {
        RowSource {
            mode,
            fields: vec![
                RowField {
                    column: "carrid".into(),
                    ty: AbapType::Char {
                        length: 2,
                        dtype: CharType::Char,
                    },
                    offset: 0,
                },
                RowField {
                    column: "connid".into(),
                    ty: AbapType::Char {
                        length: 4,
                        dtype: CharType::Numc,
                    },
                    offset: 4,
                },
                RowField {
                    column: "seats".into(),
                    ty: AbapType::Int,
                    offset: 12,
                },
                RowField {
                    column: "fldate".into(),
                    ty: AbapType::Char {
                        length: 8,
                        dtype: CharType::Dats,
                    },
                    offset: 16,
                },
            ],
            size: 32,
        }
    }

    /// Arbeitsbereich einer Zeile von `flug_source` mit den gegebenen Werten.
    fn flug_row(carrid: &str, connid: &str, seats: i32, fldate: &str) -> Vec<u8> {
        let mut row = vec![0u8; 32];
        row[0..4].copy_from_slice(&utf16_padded(carrid, 2));
        row[4..12].copy_from_slice(&utf16_padded(connid, 4));
        row[12..16].copy_from_slice(&seats.to_le_bytes());
        row[16..32].copy_from_slice(&utf16_padded(fldate, 8));
        row
    }

    fn write_request(line: u32, op: SqlOp) -> SqlRequest {
        SqlRequest {
            ir_version: IR_VERSION,
            line,
            connection: "DEFAULT".into(),
            op,
        }
    }

    /// Führt eine Schreibanweisung über `write_in` aus. Die IR-Kennung muss je Zustand eindeutig sein.
    fn exec(
        state: &mut State,
        config: &Config,
        ir_id: u32,
        request: &SqlRequest,
        host: &mut [*mut u8],
        target: *mut u8,
    ) -> Result<Summary, String> {
        let json = abap_sql_ir::encode(request);
        let args = IrArgs {
            ir_id,
            ir_json: json.as_ptr(),
            ir_len: json.len() as u64,
            params: host.as_ptr(),
            n_params: host.len() as u64,
        };
        // SAFETY: JSON, Hostzeiger und Zielbereich leben während des Aufrufs; der Aufrufer übergibt
        // Zielbereiche mit der Größe der Zeilenquelle.
        unsafe { write_in(state, config, args, target) }
    }

    /// Zählt die Zeilen der Datei über eine eigene, zweite Verbindung (sieht nur festgeschriebene Änderungen).
    fn stored_rows(path: &std::path::Path, where_clause: &str) -> i64 {
        let conn = rusqlite::Connection::open(path).expect("open database");
        conn.query_row(
            &format!("SELECT COUNT(*) FROM flug {where_clause}"),
            [],
            |row| row.get(0),
        )
        .expect("count")
    }

    fn insert_op(mode: RowMode, accepting: bool) -> SqlOp {
        SqlOp::Insert(abap_sql_ir::InsertOp {
            table: "ZFLUG".into(),
            rows: flug_source(mode),
            accepting_duplicate_keys: accepting,
        })
    }

    /// Tabelle mit den gegebenen Zeilen (je 32 Bytes), wie sie der Codegen für `INSERT FROM TABLE` füllt.
    fn internal_table(rows: &[Vec<u8>]) -> *mut RtTable {
        let table = crate::abap_rt_table_new(32);
        for row in rows {
            unsafe { crate::abap_rt_table_append(table, row.as_ptr(), row.len() as u64) };
        }
        table
    }

    #[test]
    fn insert_from_work_area_is_visible_after_commit_only() {
        let path = write_file("insert-commit");
        let config = keyed_config_for(&path);
        let mut state = State::default();
        let request = write_request(5, insert_op(RowMode::Single, false));
        let mut row = flug_row("XX", "0001", 7, "20260104");
        let summary = exec(
            &mut state,
            &config,
            201,
            &request,
            &mut [],
            row.as_mut_ptr(),
        )
        .unwrap();
        assert_eq!(summary, Summary { subrc: 0, dbcnt: 1 });
        // Vor COMMIT WORK sieht eine zweite Verbindung die Zeile nicht.
        assert_eq!(stored_rows(&path, "WHERE carrid = 'XX'"), 0);
        commit_in(&mut state).unwrap();
        assert_eq!(stored_rows(&path, "WHERE carrid = 'XX' AND seats = 7"), 1);
        drop(state);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn rollback_discards_the_inserted_row() {
        let path = write_file("insert-rollback");
        let config = keyed_config_for(&path);
        let mut state = State::default();
        let request = write_request(5, insert_op(RowMode::Single, false));
        let mut row = flug_row("XX", "0001", 7, "20260104");
        exec(
            &mut state,
            &config,
            202,
            &request,
            &mut [],
            row.as_mut_ptr(),
        )
        .unwrap();
        rollback_in(&mut state).unwrap();
        assert_eq!(stored_rows(&path, ""), 3);
        drop(state);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn open_luw_is_rolled_back_at_program_end() {
        let path = write_file("finish");
        let config = keyed_config_for(&path);
        let mut state = State::default();
        let request = write_request(5, insert_op(RowMode::Single, false));
        let mut row = flug_row("XX", "0001", 7, "20260104");
        exec(
            &mut state,
            &config,
            203,
            &request,
            &mut [],
            row.as_mut_ptr(),
        )
        .unwrap();
        // Das Programm endet ohne COMMIT WORK: der Abschluss rollt die LUW zurück.
        finish_in(&mut state);
        assert_eq!(stored_rows(&path, ""), 3);
        drop(state);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn commit_without_open_luw_is_not_an_error() {
        let mut state = State::default();
        assert_eq!(commit_in(&mut state), Ok(()));
        assert_eq!(rollback_in(&mut state), Ok(()));
    }

    #[test]
    fn two_writes_share_one_luw_so_one_rollback_undoes_both() {
        let path = write_file("two-writes");
        let config = keyed_config_for(&path);
        let mut state = State::default();
        let request = write_request(5, insert_op(RowMode::Single, false));
        let mut first = flug_row("XX", "0001", 1, "20260104");
        let mut second = flug_row("YY", "0002", 2, "20260105");
        exec(
            &mut state,
            &config,
            204,
            &request,
            &mut [],
            first.as_mut_ptr(),
        )
        .unwrap();
        // Ein zweites BEGIN innerhalb der LUW würde SQLite ablehnen: die Anweisung läuft also in derselben LUW.
        exec(
            &mut state,
            &config,
            204,
            &request,
            &mut [],
            second.as_mut_ptr(),
        )
        .unwrap();
        rollback_in(&mut state).unwrap();
        assert_eq!(stored_rows(&path, ""), 3);
        drop(state);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn char_and_numc_values_are_written_like_abap_moves() {
        let path = write_file("char-numc");
        let config = keyed_config_for(&path);
        let mut state = State::default();
        let request = write_request(5, insert_op(RowMode::Single, false));
        // Zeichen "LH" mit nachgestellten Leerzeichen und NUMC "12" mit Leerzeichen: gespeichert als "LH" und "0012".
        let mut row = flug_row("LH", "12  ", 3, "20260105");
        exec(
            &mut state,
            &config,
            205,
            &request,
            &mut [],
            row.as_mut_ptr(),
        )
        .unwrap();
        commit_in(&mut state).unwrap();
        assert_eq!(
            stored_rows(&path, "WHERE carrid = 'LH' AND connid = '0012'"),
            1
        );
        drop(state);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn single_insert_with_duplicate_key_is_subrc_four_without_error() {
        let path = write_file("single-dup");
        let config = keyed_config_for(&path);
        let mut state = State::default();
        let request = write_request(5, insert_op(RowMode::Single, false));
        let mut row = flug_row("LH", "0400", 1, "20260106");
        let summary = exec(
            &mut state,
            &config,
            206,
            &request,
            &mut [],
            row.as_mut_ptr(),
        )
        .unwrap();
        assert_eq!(summary, Summary { subrc: 4, dbcnt: 0 });
        drop(state);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn from_table_insert_without_accepting_keeps_the_rows_before_the_duplicate() {
        let path = write_file("from-table-dup");
        let config = keyed_config_for(&path);
        let mut state = State::default();
        let table = internal_table(&[
            flug_row("XX", "0001", 1, "20260104").to_vec(),
            flug_row("LH", "0400", 2, "20260104").to_vec(),
            flug_row("YY", "0002", 3, "20260104").to_vec(),
        ]);
        let request = write_request(5, insert_op(RowMode::Table, false));
        let err = exec(
            &mut state,
            &config,
            207,
            &request,
            &mut [],
            table as *mut u8,
        )
        .unwrap_err();
        assert_eq!(
            err,
            "db error at line 5 (connection DEFAULT, table ZFLUG): duplicate key"
        );
        // Die Zeile vor der Dublette bleibt in der LUW, die danach wird nicht versucht.
        commit_in(&mut state).unwrap();
        assert_eq!(stored_rows(&path, "WHERE carrid = 'XX'"), 1);
        assert_eq!(stored_rows(&path, "WHERE carrid = 'YY'"), 0);
        unsafe { drop(Box::from_raw(table)) };
        drop(state);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn from_table_insert_with_accepting_skips_the_duplicate_and_counts_the_inserted_rows() {
        let path = write_file("from-table-accept");
        let config = keyed_config_for(&path);
        let mut state = State::default();
        let table = internal_table(&[
            flug_row("XX", "0001", 1, "20260104").to_vec(),
            flug_row("LH", "0400", 2, "20260104").to_vec(),
            flug_row("YY", "0002", 3, "20260104").to_vec(),
        ]);
        let request = write_request(5, insert_op(RowMode::Table, true));
        let summary = exec(
            &mut state,
            &config,
            208,
            &request,
            &mut [],
            table as *mut u8,
        )
        .unwrap();
        assert_eq!(summary, Summary { subrc: 4, dbcnt: 2 });
        commit_in(&mut state).unwrap();
        assert_eq!(stored_rows(&path, "WHERE carrid IN ('XX', 'YY')"), 2);
        unsafe { drop(Box::from_raw(table)) };
        drop(state);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn update_binds_assignment_and_where_host_variables() {
        use abap_sql_ir::{Assignment, CmpOp, Expr, UpdateOp};
        let path = write_file("update-host");
        let config = keyed_config_for(&path);
        let mut state = State::default();
        let request = write_request(
            6,
            SqlOp::Update(UpdateOp {
                table: "ZFLUG".into(),
                assignments: vec![Assignment {
                    column: "seats".into(),
                    value: Expr::Param { index: 0 },
                }],
                filter: Some(Expr::Compare {
                    op: CmpOp::Eq,
                    left: Box::new(Expr::Column {
                        name: "carrid".into(),
                    }),
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
                        ty: AbapType::Char {
                            length: 2,
                            dtype: CharType::Char,
                        },
                    },
                ],
            }),
        );
        let mut seats = 300i32.to_le_bytes();
        let mut carrid = utf16_padded("LH", 2);
        let mut host = [seats.as_mut_ptr(), carrid.as_mut_ptr()];
        let summary = exec(
            &mut state,
            &config,
            209,
            &request,
            &mut host,
            std::ptr::null_mut(),
        )
        .unwrap();
        assert_eq!(summary, Summary { subrc: 0, dbcnt: 2 });
        commit_in(&mut state).unwrap();
        assert_eq!(stored_rows(&path, "WHERE carrid = 'LH' AND seats = 300"), 2);
        drop(state);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn host_value_with_a_quote_does_not_change_the_update_statement() {
        use abap_sql_ir::{Assignment, CmpOp, Expr, UpdateOp};
        let path = write_file("update-quote");
        let config = keyed_config_for(&path);
        let mut state = State::default();
        let request = write_request(
            6,
            SqlOp::Update(UpdateOp {
                table: "ZFLUG".into(),
                assignments: vec![Assignment {
                    column: "seats".into(),
                    value: Expr::Literal {
                        value: abap_sql_ir::Value::Int { value: 1 },
                    },
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
                    name: "lv_carrid".into(),
                    ty: AbapType::Char {
                        length: 12,
                        dtype: CharType::Char,
                    },
                }],
            }),
        );
        let mut carrid = utf16_padded("x' OR '1'='1", 12);
        let mut host = [carrid.as_mut_ptr()];
        let summary = exec(
            &mut state,
            &config,
            210,
            &request,
            &mut host,
            std::ptr::null_mut(),
        )
        .unwrap();
        assert_eq!(summary, Summary { subrc: 4, dbcnt: 0 });
        commit_in(&mut state).unwrap();
        assert_eq!(stored_rows(&path, ""), 3);
        assert_eq!(stored_rows(&path, "WHERE seats = 1"), 0);
        drop(state);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn delete_without_where_removes_every_row_with_subrc_zero() {
        use abap_sql_ir::DeleteOp;
        let path = write_file("delete-all");
        let config = keyed_config_for(&path);
        let mut state = State::default();
        let request = write_request(
            7,
            SqlOp::Delete(DeleteOp {
                table: "ZFLUG".into(),
                filter: None,
                rows: None,
                params: vec![],
            }),
        );
        let summary = exec(
            &mut state,
            &config,
            211,
            &request,
            &mut [],
            std::ptr::null_mut(),
        )
        .unwrap();
        assert_eq!(summary, Summary { subrc: 0, dbcnt: 3 });
        commit_in(&mut state).unwrap();
        assert_eq!(stored_rows(&path, ""), 0);
        drop(state);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn delete_from_work_area_removes_the_row_with_that_key() {
        use abap_sql_ir::DeleteOp;
        let path = write_file("delete-key");
        let config = keyed_config_for(&path);
        let mut state = State::default();
        let request = write_request(
            7,
            SqlOp::Delete(DeleteOp {
                table: "ZFLUG".into(),
                filter: None,
                rows: Some(flug_source(RowMode::Single)),
                params: vec![],
            }),
        );
        let mut row = flug_row("LH", "0400", 0, "20260101");
        let summary = exec(
            &mut state,
            &config,
            212,
            &request,
            &mut [],
            row.as_mut_ptr(),
        )
        .unwrap();
        assert_eq!(summary, Summary { subrc: 0, dbcnt: 1 });
        commit_in(&mut state).unwrap();
        assert_eq!(stored_rows(&path, ""), 2);
        drop(state);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn modify_inserts_a_missing_row_and_changes_an_existing_one() {
        let path = write_file("modify");
        let config = keyed_config_for(&path);
        let mut state = State::default();
        let request = write_request(
            8,
            SqlOp::Modify(abap_sql_ir::ModifyOp {
                table: "ZFLUG".into(),
                rows: flug_source(RowMode::Single),
            }),
        );
        let mut new_row = flug_row("XX", "0001", 5, "20260104");
        let inserted = exec(
            &mut state,
            &config,
            213,
            &request,
            &mut [],
            new_row.as_mut_ptr(),
        )
        .unwrap();
        assert_eq!(inserted, Summary { subrc: 0, dbcnt: 1 });
        let mut existing = flug_row("LH", "0400", 777, "20260101");
        let changed = exec(
            &mut state,
            &config,
            213,
            &request,
            &mut [],
            existing.as_mut_ptr(),
        )
        .unwrap();
        assert_eq!(changed, Summary { subrc: 0, dbcnt: 1 });
        commit_in(&mut state).unwrap();
        assert_eq!(stored_rows(&path, ""), 4);
        assert_eq!(
            stored_rows(
                &path,
                "WHERE carrid = 'LH' AND connid = '0400' AND seats = 777"
            ),
            1
        );
        drop(state);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn modify_without_key_in_configuration_is_an_error() {
        let path = write_file("modify-nokey");
        let config = config_for(&path);
        let mut state = State::default();
        let request = write_request(
            8,
            SqlOp::Modify(abap_sql_ir::ModifyOp {
                table: "ZFLUG".into(),
                rows: flug_source(RowMode::Single),
            }),
        );
        let mut row = flug_row("XX", "0001", 5, "20260104");
        let err = exec(
            &mut state,
            &config,
            214,
            &request,
            &mut [],
            row.as_mut_ptr(),
        )
        .unwrap_err();
        assert_eq!(
            err,
            "invalid SQL IR at line 8: table ZFLUG has no key in configuration"
        );
        drop(state);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn host_parameter_count_of_an_update_is_checked_against_the_ir() {
        use abap_sql_ir::UpdateOp;
        let path = write_file("host-count");
        let config = keyed_config_for(&path);
        let mut state = State::default();
        let request = write_request(
            9,
            SqlOp::Update(UpdateOp {
                table: "ZFLUG".into(),
                assignments: vec![],
                filter: None,
                rows: Some(flug_source(RowMode::Single)),
                params: vec![Param {
                    name: "lv".into(),
                    ty: AbapType::Int,
                }],
            }),
        );
        let mut row = flug_row("LH", "0400", 1, "20260101");
        let err = exec(
            &mut state,
            &config,
            215,
            &request,
            &mut [],
            row.as_mut_ptr(),
        )
        .unwrap_err();
        assert_eq!(
            err,
            "invalid SQL IR at line 9: expected 1 host variables, got 0"
        );
        drop(state);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn insert_takes_no_host_variables() {
        let path = write_file("insert-host");
        let config = keyed_config_for(&path);
        let mut state = State::default();
        let request = write_request(10, insert_op(RowMode::Single, false));
        let mut row = flug_row("XX", "0001", 1, "20260104");
        let mut extra = [row.as_mut_ptr()];
        let err = exec(
            &mut state,
            &config,
            216,
            &request,
            &mut extra,
            row.as_mut_ptr(),
        )
        .unwrap_err();
        assert_eq!(
            err,
            "invalid SQL IR at line 10: expected 0 host variables, got 1"
        );
        drop(state);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn select_passed_to_the_write_entry_point_is_an_internal_error() {
        let path = write_file("select-in-write");
        let config = keyed_config_for(&path);
        let mut state = State::default();
        let request = write_request(
            11,
            SqlOp::Select(SelectOp {
                table: "ZFLUG".into(),
                columns: vec![],
                filter: None,
                order_by: vec![],
                up_to: None,
                result: ResultMode::Single,
                target_size: 0,
                target_fields: vec![],
                params: vec![],
            }),
        );
        let err = exec(
            &mut state,
            &config,
            217,
            &request,
            &mut [],
            std::ptr::null_mut(),
        )
        .unwrap_err();
        assert_eq!(
            err,
            "internal error: SELECT passed to abap_rt_sql_execute at line 11"
        );
        drop(state);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn commit_as_ir_statement_commits_the_open_luw() {
        let path = write_file("commit-ir");
        let config = keyed_config_for(&path);
        let mut state = State::default();
        let request = write_request(12, insert_op(RowMode::Single, false));
        let mut row = flug_row("XX", "0001", 1, "20260104");
        exec(
            &mut state,
            &config,
            218,
            &request,
            &mut [],
            row.as_mut_ptr(),
        )
        .unwrap();
        let commit = write_request(13, SqlOp::Commit);
        let summary = exec(
            &mut state,
            &config,
            219,
            &commit,
            &mut [],
            std::ptr::null_mut(),
        )
        .unwrap();
        assert_eq!(summary, Summary { subrc: 0, dbcnt: 0 });
        assert_eq!(stored_rows(&path, ""), 4);
        drop(state);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn row_source_size_must_match_the_internal_table_rows() {
        let path = write_file("table-size");
        let config = keyed_config_for(&path);
        let mut state = State::default();
        let table = crate::abap_rt_table_new(16);
        let request = write_request(14, insert_op(RowMode::Table, false));
        let err = exec(
            &mut state,
            &config,
            220,
            &request,
            &mut [],
            table as *mut u8,
        )
        .unwrap_err();
        assert_eq!(
            err,
            "internal error: internal table row size 16 does not match the row source (32) at line 14"
        );
        unsafe { drop(Box::from_raw(table)) };
        drop(state);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn write_to_an_unconfigured_table_is_reported_with_the_line() {
        let config = Config::default();
        let mut state = State::default();
        let mut row = flug_row("XX", "0001", 1, "20260104");
        let request = write_request(15, insert_op(RowMode::Single, false));
        let err = exec(
            &mut state,
            &config,
            221,
            &request,
            &mut [],
            row.as_mut_ptr(),
        )
        .unwrap_err();
        assert_eq!(err, "table ZFLUG is not configured (line 15)");
    }
}
