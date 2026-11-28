//! Testhelfer für den externen SQL-Adapter (`abap-rt/src/sql/external.rs`). Kein Adapter für den Betrieb.
//!
//! Aufruf: `sql_stub <modus>`. Die `capabilities`-Anfrage wird in allen Modi beantwortet, außer `stderr`.
//! - `normal`: feste Antworten für alle Methoden, Cursor liefert zwei Zeilen und dann Ende.
//! - `exit`: beendet sich bei der ersten Anfrage nach `capabilities`, ohne zu antworten.
//! - `sleep`: antwortet auf die erste Anfrage nach `capabilities` erst nach 10 Sekunden.
//! - `badjson`: antwortet mit keinem gültigen JSON.
//! - `wrongid`: antwortet mit falscher `id`.
//! - `error`: antwortet mit `ok: false` (kind `db`, message `connection refused`).
//! - `stderr`: schreibt eine Zeile nach stderr und beendet sich sofort.
//! - `dml`: `execute` antwortet mit `rows: []` und `dbcnt: 2` (eine Schreibanweisung), die übrigen Methoden wie `normal`.
//! - `duplicate`: `execute` antwortet mit dem Fehler `kind: "duplicate"`, `message: "duplicate key"`; übrige wie `normal`.
//!
//! Die `capabilities`-Antwort meldet alle Abfrage- und Schreibfähigkeiten außer `like`, `between`, `in`,
//! `and_or_not` (der Stub kann sie nicht ausführen). Das ist für die Tests der Laufzeit ausreichend.

use std::io::{self, BufRead, Write};
use std::time::Duration;

use serde_json::{json, Value};

fn reply(value: Value) {
    let mut out = io::stdout().lock();
    let _ = writeln!(out, "{value}");
    let _ = out.flush();
}

/// Schreibanweisung mit zwei betroffenen Zeilen. Übrige Methoden wie `normal`.
fn dml(method: &str, id: u64, fetches: &mut u32) {
    match method {
        "execute" => reply(json!({"id": id, "ok": true, "rows": [], "dbcnt": 2})),
        _ => normal(method, id, fetches),
    }
}

/// Dublette bei `execute`. Übrige Methoden wie `normal`.
fn duplicate(method: &str, id: u64, fetches: &mut u32) {
    match method {
        "execute" => reply(json!({
            "id": id,
            "ok": false,
            "error": {"kind": "duplicate", "message": "duplicate key"}
        })),
        _ => normal(method, id, fetches),
    }
}

fn normal(method: &str, id: u64, fetches: &mut u32) {
    match method {
        "check" | "close_cursor" | "begin" | "commit" | "rollback" => {
            reply(json!({"id": id, "ok": true}));
        }
        "execute" => reply(json!({"id": id, "ok": true, "rows": [[1, "ab"]], "dbcnt": 1})),
        "open_cursor" => reply(json!({"id": id, "ok": true, "cursor": 7})),
        "fetch" => {
            *fetches += 1;
            let rows = match *fetches {
                1 => json!([[1, "ab"]]),
                2 => json!([[2, "cd"]]),
                _ => json!([]),
            };
            reply(json!({"id": id, "ok": true, "rows": rows}));
        }
        _ => reply(
            json!({"id": id, "ok": false, "error": {"kind": "protocol", "message": "unknown method"}}),
        ),
    }
}

fn main() {
    let mode = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "normal".to_string());
    if mode == "stderr" {
        eprintln!("stub diagnostic: adapter failed");
        std::process::exit(3);
    }
    let mut fetches = 0u32;
    for line in io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let Ok(request) = serde_json::from_str::<Value>(&line) else {
            break;
        };
        let id = request["id"].as_u64().unwrap_or(0);
        let method = request["method"].as_str().unwrap_or("");
        if method == "capabilities" {
            reply(json!({
                "id": id,
                "ok": true,
                "capabilities": {
                    "select": true,
                    "single": true,
                    "where": true,
                    "order_by": true,
                    "limit": true,
                    "insert": true,
                    "update_by_key": true,
                    "update_by_set": true,
                    "delete": true,
                    "delete_by_set": true,
                    "modify": true,
                    "commit": true,
                    "rollback": true,
                    "concurrent_cursors": false
                }
            }));
            continue;
        }
        match mode.as_str() {
            "exit" => std::process::exit(0),
            "sleep" => {
                std::thread::sleep(Duration::from_secs(10));
                reply(json!({"id": id, "ok": true}));
            }
            "badjson" => {
                println!("this is not json");
                let _ = io::stdout().flush();
            }
            "wrongid" => reply(json!({"id": id + 1, "ok": true})),
            "dml" => dml(method, id, &mut fetches),
            "duplicate" => duplicate(method, id, &mut fetches),
            "error" => reply(json!({
                "id": id,
                "ok": false,
                "error": {"kind": "db", "message": "connection refused"}
            })),
            _ => normal(method, id, &mut fetches),
        }
    }
}
