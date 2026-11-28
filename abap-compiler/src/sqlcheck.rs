//! Prüfung der Datenbankanweisungen gegen die Konfiguration zur Übersetzungszeit (`abapc --db-config <datei>`).
//! Spezifikation: `doc/sql-spezifikation.md`, Abschnitte 8 und 9.
//!
//! Jede Anfrage des Programms wird der Tabelle und ihrer Verbindung zugeordnet. Fehlt die Tabelle in der
//! Konfiguration, fehlt eine Fähigkeit des SQLite-Adapters oder ist der Adapter in dieser Version nicht
//! umgesetzt, ist das ein Übersetzungsfehler mit der Zeile der Anweisung. Eine Verbindung vom Typ `extern`
//! wird nicht geprüft: ihre Fähigkeiten kommen aus dem Adapterprozess und werden zur Laufzeit geprüft.

use crate::ast::CompileError;
use abap_rt::sql::adapter::{missing_capabilities, table_of, Capabilities};
use abap_rt::sql::config::{AdapterKind, Config};
use abap_sql_ir::SqlRequest;

/// Prüft alle Anfragen gegen die Konfiguration. `sqlite` sind die Fähigkeiten des SQLite-Adapters (für
/// Verbindungen vom Typ `sqlite`). Liefert alle Fehler in der Reihenfolge der Anfragen.
pub fn check_requests(
    config: &Config,
    requests: &[SqlRequest],
    sqlite: &Capabilities,
) -> Vec<CompileError> {
    requests
        .iter()
        .filter_map(|request| check_request(config, request, sqlite).err())
        .collect()
}

/// Prüft eine Anfrage. COMMIT und ROLLBACK haben keine Tabelle und sind immer in Ordnung.
fn check_request(
    config: &Config,
    request: &SqlRequest,
    sqlite: &Capabilities,
) -> Result<(), CompileError> {
    let Some(table) = table_of(request) else {
        return Ok(());
    };
    let line = request.line as usize;
    let Some(table_config) = config.table(table) else {
        return Err(CompileError {
            line,
            message: format!("table {table} is not configured"),
        });
    };
    let connection = table_config.connection.as_str();
    let Some(connection_config) = config.connection(connection) else {
        return Err(CompileError {
            line,
            message: format!("connection {connection} is not configured"),
        });
    };
    match connection_config.kind {
        AdapterKind::Sqlite => {
            let missing = missing_capabilities(sqlite, request);
            if missing.is_empty() {
                Ok(())
            } else {
                Err(CompileError {
                    line,
                    message: format!(
                        "capability missing (connection {connection}, table {table}): {}",
                        missing.join(", ")
                    ),
                })
            }
        }
        AdapterKind::External => Ok(()),
        kind @ (AdapterKind::Postgres | AdapterKind::Odata) => Err(CompileError {
            line,
            message: format!(
                "adapter {} is not implemented in this version",
                kind.as_str()
            ),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lower::parse_source;
    use std::path::Path;

    /// Konfiguration: ZFLUG über SQLite, ZPG über PostgreSQL, ZEXT über einen Adapterprozess, ZODATA über OData.
    const CONFIG: &str = "[connections.DEFAULT]\n\
        adapter = \"sqlite\"\n\
        path = \"flug.sqlite\"\n\
        [connections.PG]\n\
        adapter = \"postgres\"\n\
        [connections.EXT]\n\
        adapter = \"extern\"\n\
        command = \"adapter.exe\"\n\
        [connections.WEB]\n\
        adapter = \"odata\"\n\
        [tables.ZFLUG]\n\
        connection = \"DEFAULT\"\n\
        target = \"flug\"\n\
        [tables.ZPG]\n\
        connection = \"PG\"\n\
        target = \"pg\"\n\
        [tables.ZEXT]\n\
        connection = \"EXT\"\n\
        target = \"ext\"\n\
        [tables.ZODATA]\n\
        connection = \"WEB\"\n\
        target = \"odata\"\n";

    fn config() -> Config {
        Config::from_toml(CONFIG, Path::new(".")).expect("configuration parses")
    }

    /// Anfragen eines Programms mit den Tabellen ZFLUG, ZPG, ZEXT und ZODATA (gleiche Spalten).
    fn requests(body: &str) -> Vec<SqlRequest> {
        let table = |name: &str| {
            format!(
                "TYPES: BEGIN OF {name},\n  carrid TYPE c LENGTH 3,\n  connid TYPE n LENGTH 4,\n  seatsmax TYPE i,\nEND OF {name}.\n\
                 DATA ls_{name} TYPE {name}.\n"
            )
        };
        let src = format!(
            "{}{}{}{}START-OF-SELECTION.\n{body}\n",
            table("zflug"),
            table("zpg"),
            table("zext"),
            table("zodata")
        );
        parse_source(&src)
            .unwrap_or_else(|e| panic!("{e:?}"))
            .sql_requests
    }

    fn messages(errors: &[CompileError]) -> Vec<String> {
        errors.iter().map(|e| e.to_string()).collect()
    }

    fn sqlite() -> Capabilities {
        Capabilities::sqlite()
    }

    #[test]
    fn sqlite_statements_with_all_capabilities_pass() {
        let errors = check_requests(
            &config(),
            &requests(
                "SELECT * FROM zflug INTO TABLE @DATA(lt) WHERE carrid = 'LH'.\n\
                 INSERT INTO zflug VALUES ls_zflug.\n\
                 DELETE FROM zflug WHERE connid = '0400'.\n\
                 COMMIT WORK.",
            ),
            &sqlite(),
        );
        assert!(errors.is_empty(), "{:?}", messages(&errors));
    }

    #[test]
    fn missing_sqlite_capability_is_reported_with_connection_table_and_line() {
        let caps = Capabilities {
            update_by_set: false,
            ..Capabilities::sqlite()
        };
        let errors = check_requests(
            &config(),
            &requests("UPDATE zflug SET seatsmax = 3 WHERE carrid = 'LH'."),
            &caps,
        );
        assert_eq!(
            messages(&errors),
            vec!["Line 26: capability missing (connection DEFAULT, table ZFLUG): update_by_set"]
        );
    }

    #[test]
    fn table_missing_from_configuration_is_reported_with_line() {
        // Die Tabelle ist im Programm bekannt (DDIC), fehlt aber in der Konfiguration.
        let src = "TYPES: BEGIN OF zfehlt,\n  carrid TYPE c LENGTH 3,\nEND OF zfehlt.\n\
                   DATA ls_zfehlt TYPE zfehlt.\n\
                   START-OF-SELECTION.\n\
                   SELECT SINGLE * FROM zfehlt INTO ls_zfehlt.\n";
        let program = parse_source(src).unwrap_or_else(|e| panic!("{e:?}"));
        let errors = check_requests(&config(), &program.sql_requests, &sqlite());
        assert_eq!(
            messages(&errors),
            vec!["Line 6: table ZFEHLT is not configured"]
        );
    }

    #[test]
    fn postgres_connection_is_not_implemented_in_this_version() {
        let errors = check_requests(
            &config(),
            &requests("SELECT SINGLE * FROM zpg INTO ls_zpg."),
            &sqlite(),
        );
        assert_eq!(
            messages(&errors),
            vec!["Line 26: adapter postgres is not implemented in this version"]
        );
    }

    #[test]
    fn odata_connection_is_not_implemented_in_this_version() {
        let errors = check_requests(
            &config(),
            &requests("DELETE FROM zodata WHERE connid = '0400'."),
            &sqlite(),
        );
        assert_eq!(
            messages(&errors),
            vec!["Line 26: adapter odata is not implemented in this version"]
        );
    }

    #[test]
    fn external_adapter_is_not_checked_at_translation_time() {
        let caps = Capabilities::default();
        let errors = check_requests(
            &config(),
            &requests("UPDATE zext SET seatsmax = 3 WHERE carrid = 'LH'."),
            &caps,
        );
        assert!(errors.is_empty(), "{:?}", messages(&errors));
    }

    #[test]
    fn errors_of_several_statements_are_all_returned_in_request_order() {
        let caps = Capabilities {
            delete_by_set: false,
            ..Capabilities::sqlite()
        };
        let errors = check_requests(
            &config(),
            &requests(
                "DELETE FROM zflug WHERE connid = '0400'.\n\
                 DELETE FROM zpg WHERE connid = '0400'.",
            ),
            &caps,
        );
        assert_eq!(
            messages(&errors),
            vec![
                "Line 26: capability missing (connection DEFAULT, table ZFLUG): delete_by_set",
                "Line 27: adapter postgres is not implemented in this version",
            ]
        );
    }
}
