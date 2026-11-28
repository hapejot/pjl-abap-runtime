//! Akzeptanztests: jedes Verzeichnis unter `tests/acceptance/` ist ein Fall.
//!
//! - `prog.abap`: ABAP-Quelltext
//! - `expected.txt`: erwartete stdout-Ausgabe des übersetzten Programms (exakt)
//! - `expected_error.txt`: Teil der Fehlermeldung, wenn die Übersetzung scheitern muss
//! - `input.keys` und `expected_screen.txt`: Dynpro-Fälle. Eingabeskript und Gitterabzüge der Headless-Laufzeit
//! - `db.sql`: Datenbank-Fall. Die Datenbank wird vor der Übersetzung angelegt und mit `--db-config` geprüft
//! - `db-config.toml`: Konfiguration, die `abapc` mit `--db-config` prüft (ohne `db.sql`)
//! - `ddic/`: DDIC-Fall. Verzeichnis mit abapGit-XML (`*.doma.xml`, `*.dtel.xml`, `*.tabl.xml`), geht an `abapc --ddic`
//! - `zu-pruefen.txt`: Erwartung, die gegen den ABAP-Standard noch nicht belegt ist (nur Hinweis im Bericht)

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn cases_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/acceptance")
}

/// Richtet die Datenbank eines Falls ein: `db.sql` wird in `<name>.sqlite` im Arbeitsverzeichnis ausgeführt
/// (vorher gelöscht). Danach steht `<name>_db.toml` mit dem absoluten Pfad, und jede Tabelle aus
/// `CREATE TABLE` bekommt einen Eintrag mit dem Namen in Kleinbuchstaben als Ziel. Liefert den Pfad der Konfiguration.
fn prepare_database(dir: &Path, name: &str, work: &Path) -> Result<PathBuf, String> {
    let script = fs::read_to_string(dir.join("db.sql")).map_err(|e| format!("db.sql: {e}"))?;
    let sqlite = work.join(format!("{name}.sqlite"));
    let _ = fs::remove_file(&sqlite);
    {
        let conn = rusqlite::Connection::open(&sqlite).map_err(|e| format!("sqlite: {e}"))?;
        conn.execute_batch(&script)
            .map_err(|e| format!("db.sql could not be executed: {e}"))?;
    }
    // Literal-Zeichenkette in TOML: keine Escapes, daher auch für Windows-Pfade geeignet
    let mut config = format!(
        "[connections.DEFAULT]\nadapter = \"sqlite\"\npath = '{}'\n",
        sqlite.display()
    );
    let create =
        regex::Regex::new(r"(?i)create\s+table\s+([a-z_][a-z0-9_]*)").map_err(|e| e.to_string())?;
    // Der Schlüssel einer Tabelle ist ihr `PRIMARY KEY` (entspricht dem DDIC-Schlüssel, für MODIFY und FROM wa)
    let primary =
        regex::Regex::new(r"(?i)primary\s+key\s*\(([^)]*)\)").map_err(|e| e.to_string())?;
    for caps in create.captures_iter(&script) {
        let whole = caps.get(0).expect("whole match");
        let end = script[whole.end()..]
            .find(';')
            .map_or(script.len(), |i| whole.end() + i);
        let statement = &script[whole.start()..end];
        let key: Vec<String> = primary
            .captures(statement)
            .map(|k| {
                k[1].split(',')
                    .map(|column| format!("\"{}\"", column.trim().to_uppercase()))
                    .collect()
            })
            .unwrap_or_default();
        config.push_str(&format!(
            "[tables.{}]\nconnection = \"DEFAULT\"\ntarget = \"{}\"\nkey = [{}]\n",
            caps[1].to_uppercase(),
            caps[1].to_lowercase(),
            key.join(", ")
        ));
    }
    let path = work.join(format!("{name}_db.toml"));
    fs::write(&path, config).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

fn run_case(dir: &Path, work: &Path) -> Result<(), String> {
    let name = dir.file_name().unwrap().to_string_lossy().to_string();
    let exe = work.join(format!("{name}.exe"));
    let ll = work.join(format!("{name}.ll"));
    let _ = fs::remove_file(&exe);

    // Datenbank-Fall: `db.sql` wird vor der Übersetzung ausgeführt, die Konfiguration geht an `abapc`
    // (`--db-config`) und an den Lauf (`ABAPC_DB_CONFIG`). Ohne `db.sql` prüft `abapc` die Datei
    // `db-config.toml` des Falls, falls sie existiert (z. B. für eine Verbindung, die nicht umgesetzt ist).
    let config = if dir.join("db.sql").exists() {
        Some(prepare_database(dir, &name, work)?)
    } else {
        None
    };
    let check_config = match &config {
        Some(path) => Some(path.clone()),
        None => Some(dir.join("db-config.toml")).filter(|p| p.exists()),
    };
    let mut compile_command = Command::new(env!("CARGO_BIN_EXE_abapc"));
    compile_command
        .arg(dir.join("prog.abap"))
        .arg("-o")
        .arg(&exe)
        .arg("--emit-ir")
        .arg(&ll);
    // DDIC-Fall: `ddic/` enthält abapGit-XML (DOMA, DTEL, TABL) als synthetische Testdaten
    if dir.join("ddic").is_dir() {
        compile_command.arg("--ddic").arg(dir.join("ddic"));
    }
    if let Some(path) = &check_config {
        compile_command.arg("--db-config").arg(path);
    }
    let compile = compile_command
        .output()
        .map_err(|e| format!("abapc could not be started: {e}"))?;
    let stderr = String::from_utf8_lossy(&compile.stderr).to_string();

    if let Ok(expected_err) = fs::read_to_string(dir.join("expected_error.txt")) {
        if compile.status.success() {
            return Err("translation should have failed".into());
        }
        if !stderr.contains(expected_err.trim()) {
            return Err(format!("error message does not match: {stderr}"));
        }
        return Ok(());
    }

    if !compile.status.success() {
        return Err(format!("translation failed: {stderr}"));
    }

    // Dynpro-Fall: Eingabeskript und Gitterabzüge über die Headless-Schiene der Laufzeit (Spec 9.2, 9.3a)
    let script = dir.join("input.keys");
    let has_screen = script.exists();
    let screen_out = work.join(format!("{name}.screen.txt"));
    let _ = fs::remove_file(&screen_out);
    // Ohne `db.sql` gibt es keine Konfiguration für den Lauf; eine alte `<name>.toml` im Arbeitsverzeichnis wird entfernt.
    let _ = fs::remove_file(work.join(format!("{name}.toml")));
    let mut program = Command::new(&exe);
    program.current_dir(work);
    match &config {
        Some(path) => {
            program.env("ABAPC_DB_CONFIG", path);
        }
        None => {
            program.env_remove("ABAPC_DB_CONFIG");
        }
    }
    if has_screen {
        program
            .env("ABAPRT_SCREEN_SCRIPT", &script)
            .env("ABAPRT_SCREEN_OUT", &screen_out);
    }
    let run = program
        .output()
        .map_err(|e| format!("program could not be started: {e}"))?;

    // Erwarteter Laufzeitfehler: Programm muss fehlschlagen, stderr muss den Text enthalten
    if let Ok(expected_rt) = fs::read_to_string(dir.join("expected_runtime_error.txt")) {
        let err_out = String::from_utf8_lossy(&run.stderr).to_string();
        if run.status.success() {
            return Err("runtime error expected, but program ran successfully".into());
        }
        if !err_out.contains(expected_rt.trim()) {
            return Err(format!("runtime error does not match: {err_out}"));
        }
        return Ok(());
    }

    if !run.status.success() {
        return Err(format!("program ended with error: {:?}", run.status));
    }
    let actual = String::from_utf8_lossy(&run.stdout).to_string();
    let expected = fs::read_to_string(dir.join("expected.txt"))
        .map_err(|e| format!("expected.txt missing: {e}"))?;
    if actual != expected {
        return Err(format!(
            "Output differs\n  expected: {expected:?}\n  actual: {actual:?}"
        ));
    }
    if has_screen {
        let actual_screen = fs::read_to_string(&screen_out)
            .map_err(|e| format!("screen snapshots missing: {e}"))?;
        let expected_screen = fs::read_to_string(dir.join("expected_screen.txt"))
            .map_err(|e| format!("expected_screen.txt missing: {e}"))?;
        if actual_screen != expected_screen {
            return Err(format!(
                "Screen snapshots differ\n  expected: {expected_screen:?}\n  actual: {actual_screen:?}"
            ));
        }
    }
    Ok(())
}

/// Liest `zu-pruefen.txt` im Fallverzeichnis: Fälle, deren Erwartung gegen den ABAP-Standard
/// noch nicht belegt ist. Der Fall läuft trotzdem wie gewohnt; der Hinweis erscheint nur im Bericht.
fn zu_pruefen_hinweis(dir: &Path) -> Option<String> {
    let text = fs::read_to_string(dir.join("zu-pruefen.txt")).ok()?;
    let erste_zeile = text.lines().next().unwrap_or_default().trim().to_string();
    Some(erste_zeile)
}

#[test]
fn acceptance() {
    let work = std::env::temp_dir().join("abapc-acceptance");
    fs::create_dir_all(&work).unwrap();

    let mut dirs: Vec<PathBuf> = fs::read_dir(cases_dir())
        .expect("tests/acceptance missing")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.join("prog.abap").exists())
        .collect();
    dirs.sort();
    assert!(!dirs.is_empty(), "no test cases found");

    let mut zu_pruefen = Vec::new();
    for dir in &dirs {
        if let Some(hinweis) = zu_pruefen_hinweis(dir) {
            zu_pruefen.push(format!(
                "{}: {hinweis}",
                dir.file_name().unwrap().to_string_lossy()
            ));
        }
    }
    if !zu_pruefen.is_empty() {
        println!("To check (expectation not verified against the ABAP standard):");
        for zeile in &zu_pruefen {
            println!("  {zeile}");
        }
    }

    let mut failures = Vec::new();
    for dir in &dirs {
        if let Err(msg) = run_case(dir, &work) {
            failures.push(format!(
                "{}: {msg}",
                dir.file_name().unwrap().to_string_lossy()
            ));
        }
    }
    let passed = dirs.len() - failures.len();
    println!("{passed}/{} acceptance tests passed", dirs.len());
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
