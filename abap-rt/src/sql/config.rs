//! Konfiguration der Datenbankverbindungen (`doc/sql-spezifikation.md`, Abschnitt 9).
//!
//! Die Datei ist TOML mit `[connections.<NAME>]` und `[tables.<DDIC-NAME>]`. Pfade (SQLite-Datei, Adapterprogramm)
//! sind relativ zum Verzeichnis der Konfigurationsdatei. Namen werden ohne Unterschied der Groß- und
//! Kleinschreibung verglichen. Die Datei enthält nur Namen von Umgebungsvariablen, keine Geheimnisse.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Name der Umgebungsvariable, die die Konfigurationsdatei festlegt.
pub const CONFIG_ENV: &str = "ABAPC_DB_CONFIG";
/// Standard-Zeitlimit des externen Adapters in Millisekunden.
pub const DEFAULT_TIMEOUT_MS: u64 = 30_000;

/// Art eines Adapters aus `connections.<NAME>.adapter`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdapterKind {
    Sqlite,
    External,
    /// Nur lesbar, in Phase 1 nicht implementiert.
    Postgres,
    /// Nur lesbar, in Phase 1 nicht implementiert.
    Odata,
}

impl AdapterKind {
    /// Name wie in der Konfiguration.
    pub fn as_str(self) -> &'static str {
        match self {
            AdapterKind::Sqlite => "sqlite",
            AdapterKind::External => "extern",
            AdapterKind::Postgres => "postgres",
            AdapterKind::Odata => "odata",
        }
    }
}

/// Eine Verbindung mit aufgelösten Pfaden.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionConfig {
    /// Name wie in der Konfiguration (für Meldungen).
    pub name: String,
    pub kind: AdapterKind,
    /// SQLite-Datei, absolut oder relativ zur Konfigurationsdatei aufgelöst.
    pub path: Option<PathBuf>,
    /// Adapterprogramm, relativ zur Konfigurationsdatei aufgelöst, wenn nicht absolut.
    pub command: Option<PathBuf>,
    pub args: Vec<String>,
    pub timeout_ms: u64,
}

/// Abbildung einer DDIC-Tabelle auf das Ziel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableConfig {
    /// DDIC-Name in Großbuchstaben.
    pub ddic: String,
    /// Name der Verbindung (wie in der Konfiguration).
    pub connection: String,
    /// Tabellenname im Ziel, unverändert wie konfiguriert.
    pub target: String,
    /// DDIC-Spalte (Großbuchstaben) -> Zielspalte. Fehlende Spalten behalten ihren Namen.
    pub columns: HashMap<String, String>,
    /// Schlüssel (nur gespeichert, Phase 1 nutzt ihn nicht).
    pub key: Vec<String>,
}

impl TableConfig {
    /// Zielspaltenname für eine DDIC-Spalte (Groß-/Kleinschreibung egal).
    pub fn column(&self, ddic_column: &str) -> String {
        self.columns
            .get(&ddic_column.to_uppercase())
            .cloned()
            .unwrap_or_else(|| ddic_column.to_string())
    }
}

/// Geladene Konfiguration. Schlüssel der Maps sind Großbuchstaben.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Config {
    connections: HashMap<String, ConnectionConfig>,
    tables: HashMap<String, TableConfig>,
}

#[derive(Debug, Deserialize)]
struct RawConfig {
    #[serde(default)]
    connections: BTreeMap<String, RawConnection>,
    #[serde(default)]
    tables: BTreeMap<String, RawTable>,
}

#[derive(Debug, Deserialize)]
struct RawConnection {
    adapter: String,
    path: Option<String>,
    command: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    timeout_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct RawTable {
    connection: String,
    target: String,
    #[serde(default)]
    columns: BTreeMap<String, String>,
    #[serde(default)]
    key: Vec<String>,
}

/// Ein Bezeichner für SQL: ASCII-Buchstabe oder `_` am Anfang, danach Buchstaben, Ziffern und `_`.
/// Anführungszeichen, Leerzeichen und Sonderzeichen sind damit ausgeschlossen.
pub fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() || first == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

impl Config {
    /// Liest die Konfiguration aus TOML-Text. `dir` ist das Verzeichnis der Konfigurationsdatei.
    pub fn from_toml(text: &str, dir: &Path) -> Result<Config, String> {
        let raw: RawConfig =
            toml::from_str(text).map_err(|e| format!("invalid database configuration: {e}"))?;
        let mut config = Config::default();
        for (name, conn) in raw.connections {
            let kind = match conn.adapter.as_str() {
                "sqlite" => AdapterKind::Sqlite,
                "extern" => AdapterKind::External,
                "postgres" => AdapterKind::Postgres,
                "odata" => AdapterKind::Odata,
                other => return Err(format!("unknown adapter '{other}' for connection {name}")),
            };
            let path = match (kind, conn.path) {
                (AdapterKind::Sqlite, Some(p)) => Some(resolve_path(dir, &p)),
                (AdapterKind::Sqlite, None) => {
                    return Err(format!("connection {name} needs a path for adapter sqlite"))
                }
                _ => None,
            };
            let command = match (kind, conn.command) {
                (AdapterKind::External, Some(c)) => Some(resolve_path(dir, &c)),
                (AdapterKind::External, None) => {
                    return Err(format!(
                        "connection {name} needs a command for adapter extern"
                    ))
                }
                _ => None,
            };
            let connection = ConnectionConfig {
                name: name.clone(),
                kind,
                path,
                command,
                args: conn.args,
                timeout_ms: conn.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS),
            };
            config.connections.insert(name.to_uppercase(), connection);
        }
        for (ddic, table) in raw.tables {
            if !is_identifier(&table.target) {
                return Err(format!(
                    "invalid target table name '{}' for table {ddic}",
                    table.target
                ));
            }
            let mut columns = HashMap::new();
            for (ddic_column, target_column) in table.columns {
                if !is_identifier(&target_column) {
                    return Err(format!(
                        "invalid target column name '{target_column}' for table {ddic}"
                    ));
                }
                columns.insert(ddic_column.to_uppercase(), target_column);
            }
            let entry = TableConfig {
                ddic: ddic.to_uppercase(),
                connection: table.connection,
                target: table.target,
                columns,
                key: table.key,
            };
            config.tables.insert(ddic.to_uppercase(), entry);
        }
        Ok(config)
    }

    /// Liest die Konfiguration aus einer Datei. Relative Pfade beziehen sich auf das Dateiverzeichnis.
    pub fn from_file(path: &Path) -> Result<Config, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read database configuration {}: {e}", path.display()))?;
        let dir = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        Config::from_toml(&text, dir)
    }

    /// Verbindung nach Name (Groß-/Kleinschreibung egal).
    pub fn connection(&self, name: &str) -> Option<&ConnectionConfig> {
        self.connections.get(&name.to_uppercase())
    }

    /// Tabelle nach DDIC-Name (Groß-/Kleinschreibung egal).
    pub fn table(&self, ddic: &str) -> Option<&TableConfig> {
        self.tables.get(&ddic.to_uppercase())
    }

    /// Alle Tabellen, die über die Verbindung `connection` laufen.
    pub fn tables_of(&self, connection: &str) -> Vec<TableConfig> {
        let key = connection.to_uppercase();
        self.tables
            .values()
            .filter(|t| t.connection.to_uppercase() == key)
            .cloned()
            .collect()
    }
}

fn resolve_path(dir: &Path, text: &str) -> PathBuf {
    let path = Path::new(text);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        dir.join(path)
    }
}

/// Sucht die Konfigurationsdatei in der Reihenfolge aus Abschnitt 9:
/// 1. `ABAPC_DB_CONFIG` (gesetzt und nicht leer, dann muss die Datei existieren),
/// 2. `<Programm>.toml` neben der EXE, 3. `<Programm>.toml` im Arbeitsverzeichnis.
///
/// `None`, wenn nichts gefunden wurde.
pub fn find_config(
    env_value: Option<&str>,
    exe: Option<&Path>,
    cwd: Option<&Path>,
) -> Result<Option<PathBuf>, String> {
    if let Some(value) = env_value.filter(|v| !v.trim().is_empty()) {
        let path = PathBuf::from(value);
        if path.is_file() {
            return Ok(Some(path));
        }
        return Err(format!(
            "database configuration file not found: {} (from {CONFIG_ENV})",
            path.display()
        ));
    }
    // Ohne Programmname gibt es keinen Dateinamen für die Suche.
    let Some(stem) = exe.and_then(|e| e.file_stem()) else {
        return Ok(None);
    };
    let file_name = format!("{}.toml", stem.to_string_lossy());
    if let Some(dir) = exe.and_then(|e| e.parent()) {
        let candidate = dir.join(&file_name);
        if candidate.is_file() {
            return Ok(Some(candidate));
        }
    }
    if let Some(dir) = cwd {
        let candidate = dir.join(&file_name);
        if candidate.is_file() {
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}

/// Lädt die Konfiguration über die Suchreihenfolge des laufenden Programms.
pub fn load_default() -> Result<Config, String> {
    let env_value = std::env::var(CONFIG_ENV).ok();
    let exe = std::env::current_exe().ok();
    let cwd = std::env::current_dir().ok();
    match find_config(env_value.as_deref(), exe.as_deref(), cwd.as_deref())? {
        Some(path) => Config::from_file(&path),
        None => Err(format!(
            "no database configuration found: set {CONFIG_ENV} or place <program>.toml next to the program or in the working directory"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "abap-rt-config-{tag}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    const SAMPLE: &str = r#"
[connections.DEFAULT]
adapter = "sqlite"
path = "data/flug.sqlite"

[connections.EXTERN]
adapter = "extern"
command = "adapter/meinadapter.exe"
args = ["--modus", "lesen"]
timeout_ms = 1500

[connections.ARCHIV]
adapter = "postgres"
host = "db.example.internal"
password_env = "ABAPC_ARCHIV_PASSWORD"

[tables.ZFLUG]
connection = "DEFAULT"
target = "flug"
columns = { CARRID = "carrier_id", connid = "connection_id" }
key = ["CARRID", "CONNID"]
"#;

    #[test]
    fn environment_variable_wins_over_program_directory_and_working_directory() {
        let exe_dir = unique_dir("order-exe");
        let cwd_dir = unique_dir("order-cwd");
        let env_dir = unique_dir("order-env");
        let exe = exe_dir.join("flug.exe");
        std::fs::write(exe_dir.join("flug.toml"), "").unwrap();
        std::fs::write(cwd_dir.join("flug.toml"), "").unwrap();
        let env_file = env_dir.join("anders.toml");
        std::fs::write(&env_file, "").unwrap();

        let found =
            find_config(Some(env_file.to_str().unwrap()), Some(&exe), Some(&cwd_dir)).unwrap();
        assert_eq!(found, Some(env_file));
    }

    #[test]
    fn program_directory_wins_over_working_directory() {
        let exe_dir = unique_dir("pref-exe");
        let cwd_dir = unique_dir("pref-cwd");
        let exe = exe_dir.join("flug.exe");
        std::fs::write(exe_dir.join("flug.toml"), "").unwrap();
        std::fs::write(cwd_dir.join("flug.toml"), "").unwrap();

        let found = find_config(None, Some(&exe), Some(&cwd_dir)).unwrap();
        assert_eq!(found, Some(exe_dir.join("flug.toml")));
    }

    #[test]
    fn working_directory_is_used_when_program_directory_has_no_file() {
        let exe_dir = unique_dir("cwd-exe");
        let cwd_dir = unique_dir("cwd-cwd");
        let exe = exe_dir.join("flug.exe");
        std::fs::write(cwd_dir.join("flug.toml"), "").unwrap();

        let found = find_config(None, Some(&exe), Some(&cwd_dir)).unwrap();
        assert_eq!(found, Some(cwd_dir.join("flug.toml")));
    }

    #[test]
    fn nothing_found_gives_none() {
        let dir = unique_dir("none");
        let found = find_config(None, Some(&dir.join("flug.exe")), Some(&dir)).unwrap();
        assert_eq!(found, None);
    }

    #[test]
    fn empty_environment_variable_is_ignored() {
        let dir = unique_dir("empty-env");
        std::fs::write(dir.join("flug.toml"), "").unwrap();
        let found = find_config(Some("  "), Some(&dir.join("flug.exe")), Some(&dir)).unwrap();
        assert_eq!(found, Some(dir.join("flug.toml")));
    }

    #[test]
    fn missing_file_named_by_environment_variable_is_an_error() {
        let dir = unique_dir("missing-env");
        let missing = dir.join("gibt-es-nicht.toml");
        let err = find_config(Some(missing.to_str().unwrap()), None, None).unwrap_err();
        assert!(err.contains("not found"), "{err}");
        assert!(err.contains("ABAPC_DB_CONFIG"), "{err}");
    }

    #[test]
    fn relative_paths_resolve_against_configuration_directory() {
        let dir = unique_dir("relative");
        let config = Config::from_toml(SAMPLE, &dir).expect("parses");
        let default = config.connection("DEFAULT").expect("connection");
        assert_eq!(default.path, Some(dir.join("data/flug.sqlite")));
        let external = config.connection("EXTERN").expect("connection");
        assert_eq!(external.command, Some(dir.join("adapter/meinadapter.exe")));
        assert_eq!(
            external.args,
            vec!["--modus".to_string(), "lesen".to_string()]
        );
        assert_eq!(external.timeout_ms, 1500);
    }

    #[test]
    fn absolute_paths_are_kept() {
        let dir = unique_dir("absolute");
        let absolute = std::env::temp_dir().join("x.sqlite");
        let text = format!(
            "[connections.D]\nadapter = \"sqlite\"\npath = {:?}\n",
            absolute.to_str().unwrap()
        );
        let config = Config::from_toml(&text, &dir).expect("parses");
        assert_eq!(config.connection("D").unwrap().path, Some(absolute));
    }

    #[test]
    fn default_timeout_is_thirty_seconds() {
        let dir = unique_dir("timeout");
        let text = "[connections.E]\nadapter = \"extern\"\ncommand = \"a.exe\"\n";
        let config = Config::from_toml(text, &dir).expect("parses");
        assert_eq!(config.connection("E").unwrap().timeout_ms, 30_000);
    }

    #[test]
    fn table_and_connection_names_ignore_case() {
        let dir = unique_dir("case");
        let config = Config::from_toml(SAMPLE, &dir).expect("parses");
        assert!(config.table("zflug").is_some());
        assert!(config.table("ZFLUG").is_some());
        assert!(config.connection("default").is_some());
        assert_eq!(config.table("zflug").unwrap().target, "flug");
    }

    #[test]
    fn column_mapping_uses_target_name_or_keeps_unmapped_names() {
        let dir = unique_dir("columns");
        let config = Config::from_toml(SAMPLE, &dir).expect("parses");
        let table = config.table("ZFLUG").unwrap();
        assert_eq!(table.column("carrid"), "carrier_id");
        assert_eq!(table.column("CONNID"), "connection_id");
        assert_eq!(table.column("fldate"), "fldate");
        assert_eq!(table.key, vec!["CARRID".to_string(), "CONNID".to_string()]);
    }

    #[test]
    fn tables_of_lists_only_tables_of_that_connection() {
        let dir = unique_dir("tables-of");
        let text =
            format!("{SAMPLE}\n[tables.ZARCH]\nconnection = \"archiv\"\ntarget = \"arch\"\n");
        let config = Config::from_toml(&text, &dir).expect("parses");
        let names: Vec<String> = config
            .tables_of("default")
            .into_iter()
            .map(|t| t.ddic)
            .collect();
        assert_eq!(names, vec!["ZFLUG".to_string()]);
    }

    #[test]
    fn postgres_connection_is_readable_and_has_its_kind() {
        let dir = unique_dir("postgres");
        let config = Config::from_toml(SAMPLE, &dir).expect("parses");
        let archive = config.connection("ARCHIV").expect("connection");
        assert_eq!(archive.kind, AdapterKind::Postgres);
        assert_eq!(archive.kind.as_str(), "postgres");
    }

    #[test]
    fn syntax_error_is_reported_with_prefix() {
        let dir = unique_dir("syntax");
        let err = Config::from_toml("[connections.DEFAULT\nadapter = ", &dir).unwrap_err();
        assert!(err.starts_with("invalid database configuration"), "{err}");
    }

    #[test]
    fn unknown_adapter_name_is_an_error() {
        let dir = unique_dir("unknown-adapter");
        let err = Config::from_toml("[connections.X]\nadapter = \"mysql\"\n", &dir).unwrap_err();
        assert!(err.contains("unknown adapter 'mysql'"), "{err}");
    }

    #[test]
    fn sqlite_connection_without_path_is_an_error() {
        let dir = unique_dir("no-path");
        let err = Config::from_toml("[connections.X]\nadapter = \"sqlite\"\n", &dir).unwrap_err();
        assert!(err.contains("needs a path"), "{err}");
    }

    #[test]
    fn target_name_with_quotes_is_rejected() {
        let dir = unique_dir("quote");
        let text = "[tables.ZFLUG]\nconnection = \"DEFAULT\"\ntarget = \"flug\\\"; DROP\"\n";
        let err = Config::from_toml(text, &dir).unwrap_err();
        assert!(err.contains("invalid target table name"), "{err}");
    }

    #[test]
    fn target_column_with_space_is_rejected() {
        let dir = unique_dir("space");
        let text = "[tables.ZFLUG]\nconnection = \"DEFAULT\"\ntarget = \"flug\"\ncolumns = { CARRID = \"car id\" }\n";
        let err = Config::from_toml(text, &dir).unwrap_err();
        assert!(err.contains("invalid target column name"), "{err}");
    }

    #[test]
    fn identifier_rule_accepts_plain_names_only() {
        assert!(is_identifier("flug"));
        assert!(is_identifier("_x1"));
        assert!(!is_identifier(""));
        assert!(!is_identifier("1abc"));
        assert!(!is_identifier("a\"b"));
        assert!(!is_identifier("a b"));
        assert!(!is_identifier("a-b"));
    }
}
