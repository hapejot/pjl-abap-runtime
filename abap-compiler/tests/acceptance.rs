//! Akzeptanztests: jedes Verzeichnis unter `tests/acceptance/` ist ein Fall.
//!
//! - `prog.abap`: ABAP-Quelltext
//! - `expected.txt`: erwartete stdout-Ausgabe des übersetzten Programms (exakt)
//! - `expected_error.txt`: Teil der Fehlermeldung, wenn die Übersetzung scheitern muss
//! - `input.keys` und `expected_screen.txt`: Dynpro-Fälle. Eingabeskript und Gitterabzüge der Headless-Laufzeit

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn cases_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/acceptance")
}

fn run_case(dir: &Path, work: &Path) -> Result<(), String> {
    let name = dir.file_name().unwrap().to_string_lossy().to_string();
    let exe = work.join(format!("{name}.exe"));
    let ll = work.join(format!("{name}.ll"));
    let _ = fs::remove_file(&exe);

    let compile = Command::new(env!("CARGO_BIN_EXE_abapc"))
        .arg(dir.join("prog.abap"))
        .arg("-o")
        .arg(&exe)
        .arg("--emit-ir")
        .arg(&ll)
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
    let mut program = Command::new(&exe);
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
