//! Externer Adapter als eigenes Programm (`doc/sql-spezifikation.md`, Abschnitt 6.4).
//!
//! Protokoll (Annahmen dieser Umsetzung, im Bericht gekennzeichnet):
//! - Eine JSON-Zeile je Anfrage auf stdin, eine JSON-Zeile je Antwort auf stdout.
//! - Anfrage: `{"id": n, "method": "...", ...}`. Felder: `ir` (SqlRequest) bei `check`, `execute`, `open_cursor`;
//!   `params` (Array aus Ganzzahlen und Zeichenketten) bei `execute` und `open_cursor`; `cursor` bei
//!   `fetch` und `close_cursor`.
//! - `capabilities` antwortet mit `{"id", "ok": true, "capabilities": {"select": true, "insert": true, ...}}`.
//! - `execute` antwortet mit `"rows": [...]` (bei Schreibanweisungen `[]`) und optional `"dbcnt": n`
//!   (Anzahl betroffener Zeilen, fehlt es, gilt 0). Ein negativer oder kein ganzzahliger Wert ist ein Protokollfehler.
//! - `open_cursor` antwortet mit `"cursor": n`. `fetch` mit `"rows": [[...]]` (eine Zeile) bzw. `"rows": []` am Ende.
//! - Fehler: `{"id", "ok": false, "error": {"kind": "db"|"capability"|"protocol"|"duplicate", "message": "..."}}`.
//!   `duplicate` meldet die Verletzung eines eindeutigen Schlüssels.
//!
//! Der Prozess startet beim ersten Zugriff. Eine Anfrage läuft zur Zeit. Endet der Prozess oder überschreitet
//! er das Zeitlimit, wird er beendet und bleibt unbrauchbar. Antworten, die nicht zum Protokoll passen,
//! beenden den Prozess ebenfalls.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use abap_sql_ir::SqlRequest;
use serde_json::{json, Map, Value};

use super::adapter::{
    Capabilities, CursorId, Outcome, ParamValue, RawValue, SqlAdapter, SqlError, SqlErrorKind,
};

/// Höchstens so viele Zeichen stderr werden gesammelt (über den Grenzwert hinaus kann die letzte Ausgabe
/// einmal um einen Lesepuffer länger sein).
const STDERR_LIMIT: usize = 4096;

/// Prozesszustand: Ein-/Ausgabe und Zähler. Wird unter einem Mutex benutzt, weil `check` nur `&self` hat.
struct Io {
    name: String,
    timeout: Duration,
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<Result<String, String>>,
    stderr: Arc<Mutex<String>>,
    stderr_reader: Option<JoinHandle<()>>,
    next_id: u64,
    broken: bool,
}

/// Adapter, der über einen Prozess mit dem JSON-Protokoll spricht.
pub struct ExternalAdapter {
    capabilities: Capabilities,
    io: Mutex<Io>,
}

impl ExternalAdapter {
    /// Startet das Programm und fragt die Fähigkeiten ab. Schlägt das fehl, wird der Prozess beendet.
    pub fn start(
        name: &str,
        command: &Path,
        args: &[String],
        timeout: Duration,
    ) -> Result<ExternalAdapter, SqlError> {
        let mut child = Command::new(command)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                SqlError::process(format!(
                    "external adapter for connection {name} could not be started: {e}"
                ))
            })?;
        let (Some(stdin), Some(stdout), Some(stderr_pipe)) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(SqlError::process(format!(
                "external adapter for connection {name} has no standard streams"
            )));
        };

        let (sender, lines) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let message = match line {
                    Ok(text) => Ok(text),
                    Err(e) => Err(e.to_string()),
                };
                let failed = message.is_err();
                if sender.send(message).is_err() || failed {
                    return;
                }
            }
        });

        let stderr = Arc::new(Mutex::new(String::new()));
        let sink = Arc::clone(&stderr);
        let stderr_reader = thread::spawn(move || {
            let mut pipe = stderr_pipe;
            let mut chunk = [0u8; 512];
            loop {
                match pipe.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => {
                        if let Ok(mut text) = sink.lock() {
                            if text.len() < STDERR_LIMIT {
                                text.push_str(&String::from_utf8_lossy(&chunk[..count]));
                            }
                        }
                    }
                }
            }
        });

        let mut io = Io {
            name: name.to_string(),
            timeout,
            child,
            stdin,
            lines,
            stderr,
            stderr_reader: Some(stderr_reader),
            next_id: 0,
            broken: false,
        };
        let value = io.request("capabilities", Map::new())?;
        let capabilities = match value.get("capabilities") {
            Some(object) if object.is_object() => Capabilities::from_json(object),
            _ => return Err(io.protocol("capabilities response has no capabilities object")),
        };
        Ok(ExternalAdapter {
            capabilities,
            io: Mutex::new(io),
        })
    }

    fn call(&self, method: &str, fields: Map<String, Value>) -> Result<Value, SqlError> {
        let mut io = self.io.lock().unwrap_or_else(|e| e.into_inner());
        io.request(method, fields)
    }

    /// Protokollfehler bei einer Antwort mit ungültigem Inhalt. Der Prozess wird dabei beendet.
    fn protocol_failure(&self, message: &str) -> SqlError {
        let mut io = self.io.lock().unwrap_or_else(|e| e.into_inner());
        io.protocol(message)
    }
}

impl Io {
    /// Sendet eine Anfrage und wartet auf die Antwort mit derselben `id`.
    fn request(&mut self, method: &str, mut fields: Map<String, Value>) -> Result<Value, SqlError> {
        if self.broken {
            return Err(SqlError::process(format!(
                "external adapter for connection {} is no longer running",
                self.name
            )));
        }
        self.next_id += 1;
        let id = self.next_id;
        fields.insert("id".into(), json!(id));
        fields.insert("method".into(), json!(method));
        let line = Value::Object(fields).to_string();
        if writeln!(self.stdin, "{line}")
            .and_then(|_| self.stdin.flush())
            .is_err()
        {
            return Err(self.ended());
        }
        match self.lines.recv_timeout(self.timeout) {
            Ok(Ok(text)) => self.parse_response(id, &text),
            Ok(Err(_)) | Err(RecvTimeoutError::Disconnected) => Err(self.ended()),
            Err(RecvTimeoutError::Timeout) => {
                self.stop();
                Err(SqlError::process(format!(
                    "external adapter for connection {} timed out after {} ms",
                    self.name,
                    self.timeout.as_millis()
                )))
            }
        }
    }

    fn parse_response(&mut self, id: u64, text: &str) -> Result<Value, SqlError> {
        let value: Value = match serde_json::from_str(text) {
            Ok(value) => value,
            Err(e) => return Err(self.protocol(&format!("invalid JSON response: {e}"))),
        };
        if !value.is_object() {
            return Err(self.protocol("response is not a JSON object"));
        }
        if value.get("id").and_then(Value::as_u64) != Some(id) {
            return Err(self.protocol(&format!("response id does not match request id {id}")));
        }
        match value.get("ok").and_then(Value::as_bool) {
            Some(true) => Ok(value),
            Some(false) => {
                let Some(error) = value.get("error").and_then(Value::as_object) else {
                    return Err(self.protocol("error response has no error object"));
                };
                let Some(message) = error.get("message").and_then(Value::as_str) else {
                    return Err(self.protocol("error response has no message"));
                };
                let kind = match error.get("kind").and_then(Value::as_str) {
                    Some("db") => SqlErrorKind::Db,
                    Some("duplicate") => SqlErrorKind::Duplicate,
                    Some("capability") => SqlErrorKind::Capability,
                    Some("protocol") => SqlErrorKind::Protocol,
                    _ => return Err(self.protocol("error response has an unknown kind")),
                };
                // Ein gemeldeter Fehler beendet den Prozess nicht.
                Err(SqlError::new(kind, message))
            }
            None => Err(self.protocol("response has no boolean field ok")),
        }
    }

    /// Der Prozess ist während einer Anfrage beendet. Das gesammelte stderr wird angehängt.
    fn ended(&mut self) -> SqlError {
        self.stop();
        if let Some(handle) = self.stderr_reader.take() {
            let _ = handle.join();
        }
        let text = self
            .stderr
            .lock()
            .map(|t| t.trim().to_string())
            .unwrap_or_default();
        let mut error = SqlError::process(format!(
            "external adapter for connection {} ended during request",
            self.name
        ));
        if !text.is_empty() {
            error.detail = Some(text);
        }
        error
    }

    fn protocol(&mut self, reason: &str) -> SqlError {
        self.stop();
        SqlError::protocol(reason)
    }

    fn stop(&mut self) {
        self.broken = true;
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Io {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl SqlAdapter for ExternalAdapter {
    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    fn check(&self, req: &SqlRequest) -> Result<(), SqlError> {
        let mut fields = Map::new();
        fields.insert("ir".into(), ir_value(req)?);
        self.call("check", fields).map(|_| ())
    }

    fn execute(&mut self, req: &SqlRequest, params: &[ParamValue]) -> Result<Outcome, SqlError> {
        let value = self.call("execute", ir_and_params(req, params)?)?;
        parse_outcome(&value).map_err(|reason| self.protocol_failure(&reason))
    }

    fn open_cursor(
        &mut self,
        req: &SqlRequest,
        params: &[ParamValue],
    ) -> Result<CursorId, SqlError> {
        let value = self.call("open_cursor", ir_and_params(req, params)?)?;
        value
            .get("cursor")
            .and_then(Value::as_u64)
            .ok_or_else(|| self.protocol_failure("open_cursor response has no cursor number"))
    }

    fn fetch(&mut self, cur: CursorId) -> Result<Option<Vec<RawValue>>, SqlError> {
        let mut fields = Map::new();
        fields.insert("cursor".into(), json!(cur));
        let value = self.call("fetch", fields)?;
        let rows = value
            .get("rows")
            .and_then(Value::as_array)
            .ok_or_else(|| self.protocol_failure("fetch response has no rows array"))?;
        match rows.as_slice() {
            [] => Ok(None),
            [row] => parse_row(row)
                .map(Some)
                .map_err(|reason| self.protocol_failure(&reason)),
            more => Err(self.protocol_failure(&format!(
                "fetch returned {} rows, expected at most one",
                more.len()
            ))),
        }
    }

    fn close_cursor(&mut self, cur: CursorId) {
        let mut fields = Map::new();
        fields.insert("cursor".into(), json!(cur));
        // Fehler beim Schließen werden nicht gemeldet (Trait-Vertrag).
        let _ = self.call("close_cursor", fields);
    }

    fn begin(&mut self) -> Result<(), SqlError> {
        self.call("begin", Map::new()).map(|_| ())
    }

    fn commit(&mut self) -> Result<(), SqlError> {
        self.call("commit", Map::new()).map(|_| ())
    }

    fn rollback(&mut self) -> Result<(), SqlError> {
        self.call("rollback", Map::new()).map(|_| ())
    }
}

/// Auswertung der Antwort auf `execute`: `rows` ist Pflicht (bei Schreibanweisungen `[]`), `dbcnt` ist
/// optional und zählt die betroffenen Zeilen (fehlt es, gilt 0). Ein Wert, der keine Anzahl ist, ist ein Fehler.
fn parse_outcome(value: &Value) -> Result<Outcome, String> {
    let rows = value
        .get("rows")
        .and_then(Value::as_array)
        .ok_or_else(|| "execute response has no rows array".to_string())?;
    let rows = rows
        .iter()
        .map(parse_row)
        .collect::<Result<Vec<_>, String>>()?;
    let dbcnt = match value.get("dbcnt") {
        None => 0,
        Some(number) => number
            .as_i64()
            .filter(|count| *count >= 0)
            .ok_or_else(|| format!("dbcnt {number} is not a row count"))?,
    };
    Ok(Outcome { rows, dbcnt })
}

fn ir_value(req: &SqlRequest) -> Result<Value, SqlError> {
    serde_json::to_value(req).map_err(|e| SqlError::invalid(e.to_string()))
}

fn ir_and_params(req: &SqlRequest, params: &[ParamValue]) -> Result<Map<String, Value>, SqlError> {
    let mut fields = Map::new();
    fields.insert("ir".into(), ir_value(req)?);
    let values: Vec<Value> = params
        .iter()
        .map(|p| match p {
            ParamValue::Int(v) => json!(v),
            ParamValue::Text(t) => json!(t),
        })
        .collect();
    fields.insert("params".into(), Value::Array(values));
    Ok(fields)
}

/// Eine Zeile aus dem Protokoll: Array aus `null`, Ganzzahlen und Zeichenketten.
fn parse_row(row: &Value) -> Result<Vec<RawValue>, String> {
    let cells = row
        .as_array()
        .ok_or_else(|| "row is not a JSON array".to_string())?;
    cells.iter().map(parse_value).collect()
}

fn parse_value(value: &Value) -> Result<RawValue, String> {
    match value {
        Value::Null => Ok(RawValue::Null),
        Value::Number(number) => number
            .as_i64()
            .map(RawValue::Int)
            .ok_or_else(|| format!("value {number} is not an integer")),
        Value::String(text) => Ok(RawValue::Text(text.clone())),
        other => Err(format!("unsupported value {other}")),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use abap_sql_ir::{ResultMode, SelectOp, SqlOp, IR_VERSION};

    use super::*;

    /// Pfad des Testhelfers `examples/sql_stub`. Der Test liegt in `target/debug/deps`.
    fn stub_path() -> PathBuf {
        let exe = std::env::current_exe().expect("test executable path");
        let debug_dir = exe
            .parent()
            .and_then(|deps| deps.parent())
            .expect("target/debug directory");
        debug_dir
            .join("examples")
            .join(format!("sql_stub{}", std::env::consts::EXE_SUFFIX))
    }

    fn start_stub(mode: &str, timeout_ms: u64) -> Result<ExternalAdapter, SqlError> {
        let path = stub_path();
        assert!(
            path.is_file(),
            "sql_stub example missing at {}",
            path.display()
        );
        ExternalAdapter::start(
            "EXTERN",
            &path,
            &[mode.to_string()],
            Duration::from_millis(timeout_ms),
        )
    }

    fn request() -> SqlRequest {
        SqlRequest {
            ir_version: IR_VERSION,
            line: 9,
            connection: "DEFAULT".into(),
            op: SqlOp::Select(SelectOp {
                table: "ZFLUG".into(),
                columns: vec![],
                filter: None,
                order_by: vec![],
                up_to: None,
                result: ResultMode::Table,
                target_size: 0,
                target_fields: vec![],
                params: vec![],
            }),
        }
    }

    #[test]
    fn normal_adapter_reports_capabilities_and_rows() {
        let mut adapter = start_stub("normal", 5_000).expect("starts");
        assert!(adapter.capabilities().select);
        assert!(adapter.capabilities().limit);
        assert!(!adapter.capabilities().concurrent_cursors);
        adapter.check(&request()).expect("check accepted");
        let outcome = adapter
            .execute(
                &request(),
                &[ParamValue::Int(1), ParamValue::Text("x".into())],
            )
            .expect("execute");
        assert_eq!(
            outcome.rows,
            vec![vec![RawValue::Int(1), RawValue::Text("ab".into())]]
        );
    }

    #[test]
    fn cursor_returns_rows_then_end() {
        let mut adapter = start_stub("normal", 5_000).expect("starts");
        let cursor = adapter.open_cursor(&request(), &[]).expect("open");
        assert_eq!(cursor, 7);
        assert_eq!(
            adapter.fetch(cursor).unwrap(),
            Some(vec![RawValue::Int(1), RawValue::Text("ab".into())])
        );
        assert_eq!(
            adapter.fetch(cursor).unwrap(),
            Some(vec![RawValue::Int(2), RawValue::Text("cd".into())])
        );
        assert_eq!(adapter.fetch(cursor).unwrap(), None);
        adapter.close_cursor(cursor);
    }

    #[test]
    fn process_ending_during_request_is_a_process_error() {
        let adapter = start_stub("exit", 5_000).expect("starts");
        let err = adapter.check(&request()).unwrap_err();
        assert_eq!(err.kind, SqlErrorKind::Process);
        assert_eq!(
            err.message,
            "external adapter for connection EXTERN ended during request"
        );
    }

    #[test]
    fn process_ending_at_start_reports_collected_stderr() {
        let err = start_stub("stderr", 5_000).err().expect("start fails");
        assert_eq!(err.kind, SqlErrorKind::Process);
        assert!(
            err.message.contains("ended during request"),
            "{}",
            err.message
        );
        assert!(
            err.detail
                .as_deref()
                .unwrap_or("")
                .contains("stub diagnostic"),
            "{:?}",
            err.detail
        );
    }

    #[test]
    fn timeout_stops_the_process_and_reports_the_limit() {
        let adapter = start_stub("sleep", 300).expect("starts");
        let err = adapter.check(&request()).unwrap_err();
        assert_eq!(err.kind, SqlErrorKind::Process);
        assert_eq!(
            err.message,
            "external adapter for connection EXTERN timed out after 300 ms"
        );
        // Danach ist der Prozess unbrauchbar, es folgt kein neuer Start.
        let again = adapter.check(&request()).unwrap_err();
        assert!(
            again.message.contains("no longer running"),
            "{}",
            again.message
        );
    }

    #[test]
    fn invalid_json_answer_is_a_protocol_error() {
        let adapter = start_stub("badjson", 5_000).expect("starts");
        let err = adapter.check(&request()).unwrap_err();
        assert_eq!(err.kind, SqlErrorKind::Protocol);
        assert!(
            err.message.contains("invalid JSON response"),
            "{}",
            err.message
        );
    }

    #[test]
    fn answer_with_wrong_id_is_a_protocol_error() {
        let adapter = start_stub("wrongid", 5_000).expect("starts");
        let err = adapter.check(&request()).unwrap_err();
        assert_eq!(err.kind, SqlErrorKind::Protocol);
        assert!(
            err.message.contains("does not match request id 2"),
            "{}",
            err.message
        );
    }

    #[test]
    fn error_answer_keeps_kind_and_message_and_the_process_stays_usable() {
        let adapter = start_stub("error", 5_000).expect("starts");
        let err = adapter.check(&request()).unwrap_err();
        assert_eq!(err.kind, SqlErrorKind::Db);
        assert_eq!(err.message, "connection refused");
        // Eine Fehlerantwort ist kein Protokollfehler: die nächste Anfrage geht an denselben Prozess.
        let second = adapter.check(&request()).unwrap_err();
        assert_eq!(second.kind, SqlErrorKind::Db);
    }

    #[test]
    fn missing_executable_is_a_process_error() {
        let err = ExternalAdapter::start(
            "EXTERN",
            Path::new("definitiv-kein-programm-abap-rt.exe"),
            &[],
            Duration::from_millis(1000),
        )
        .err()
        .expect("start fails");
        assert_eq!(err.kind, SqlErrorKind::Process);
        assert!(
            err.message.contains("could not be started"),
            "{}",
            err.message
        );
    }

    #[test]
    fn dml_execute_reports_the_row_count_of_the_protocol() {
        let mut adapter = start_stub("dml", 5_000).expect("starts");
        assert!(adapter.capabilities().insert && adapter.capabilities().delete_by_set);
        let outcome = adapter.execute(&request(), &[]).expect("execute");
        assert!(outcome.rows.is_empty());
        assert_eq!(outcome.dbcnt, 2);
    }

    #[test]
    fn duplicate_error_kind_is_reported_and_the_process_stays_usable() {
        let mut adapter = start_stub("duplicate", 5_000).expect("starts");
        let err = adapter.execute(&request(), &[]).unwrap_err();
        assert_eq!(err.kind, SqlErrorKind::Duplicate);
        assert_eq!(err.message, "duplicate key");
        // Eine Fehlerantwort ist kein Protokollfehler: die nächste Anfrage geht an denselben Prozess.
        adapter.check(&request()).expect("check is still answered");
    }

    #[test]
    fn execute_answer_without_dbcnt_counts_zero_rows_changed() {
        let value = json!({"id": 1, "ok": true, "rows": []});
        assert_eq!(
            parse_outcome(&value).unwrap(),
            Outcome {
                rows: vec![],
                dbcnt: 0
            }
        );
    }

    #[test]
    fn execute_answer_reads_dbcnt_as_a_count() {
        let value = json!({"id": 1, "ok": true, "rows": [], "dbcnt": 7});
        assert_eq!(parse_outcome(&value).unwrap().dbcnt, 7);
    }

    #[test]
    fn negative_or_non_integer_dbcnt_is_a_protocol_error() {
        let negative = json!({"rows": [], "dbcnt": -1});
        assert!(parse_outcome(&negative)
            .unwrap_err()
            .contains("not a row count"));
        let text = json!({"rows": [], "dbcnt": "2"});
        assert!(parse_outcome(&text)
            .unwrap_err()
            .contains("not a row count"));
    }

    #[test]
    fn execute_answer_without_rows_is_a_protocol_error() {
        let value = json!({"id": 1, "ok": true, "dbcnt": 1});
        assert_eq!(
            parse_outcome(&value).unwrap_err(),
            "execute response has no rows array"
        );
    }

    #[test]
    fn row_values_map_null_integer_and_text() {
        let row = json!([null, 42, "0400"]);
        assert_eq!(
            parse_row(&row).unwrap(),
            vec![
                RawValue::Null,
                RawValue::Int(42),
                RawValue::Text("0400".into())
            ]
        );
    }

    #[test]
    fn fractional_number_in_row_is_rejected() {
        let row = json!([1.5]);
        assert!(parse_row(&row).unwrap_err().contains("not an integer"));
    }

    #[test]
    fn params_are_sent_as_integers_and_strings() {
        let fields = ir_and_params(
            &request(),
            &[ParamValue::Int(3), ParamValue::Text("LH".into())],
        )
        .unwrap();
        assert_eq!(fields["params"], json!([3, "LH"]));
        assert!(fields.contains_key("ir"));
    }

    #[test]
    fn ir_is_sent_as_json_object_with_op_tag() {
        let value = ir_value(&request()).unwrap();
        assert_eq!(value["op"]["op"], json!("select"));
        assert_eq!(value["ir_version"], json!(IR_VERSION));
    }
}
