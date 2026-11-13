//! Akzeptanztests: jedes Verzeichnis unter `tests/acceptance/` ist ein Fall.
//!
//! - `prog.abap`: ABAP-Quelltext
//! - `expected.txt`: erwartete stdout-Ausgabe des übersetzten Programms (exakt)
//! - `expected_error.txt`: Teil der Fehlermeldung, wenn die Übersetzung scheitern muss

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
        .map_err(|e| format!("abapc konnte nicht gestartet werden: {e}"))?;
    let stderr = String::from_utf8_lossy(&compile.stderr).to_string();

    if let Ok(expected_err) = fs::read_to_string(dir.join("expected_error.txt")) {
        if compile.status.success() {
            return Err("Übersetzung hätte scheitern müssen".into());
        }
        if !stderr.contains(expected_err.trim()) {
            return Err(format!("Fehlermeldung passt nicht: {stderr}"));
        }
        return Ok(());
    }

    if !compile.status.success() {
        return Err(format!("Übersetzung fehlgeschlagen: {stderr}"));
    }

    let run = Command::new(&exe)
        .output()
        .map_err(|e| format!("Programm konnte nicht gestartet werden: {e}"))?;

    // Erwarteter Laufzeitfehler: Programm muss fehlschlagen, stderr muss den Text enthalten
    if let Ok(expected_rt) = fs::read_to_string(dir.join("expected_runtime_error.txt")) {
        let err_out = String::from_utf8_lossy(&run.stderr).to_string();
        if run.status.success() {
            return Err("Laufzeitfehler erwartet, Programm lief durch".into());
        }
        if !err_out.contains(expected_rt.trim()) {
            return Err(format!("Laufzeitfehler passt nicht: {err_out}"));
        }
        return Ok(());
    }

    if !run.status.success() {
        return Err(format!("Programm mit Fehler beendet: {:?}", run.status));
    }
    let actual = String::from_utf8_lossy(&run.stdout).to_string();
    let expected = fs::read_to_string(dir.join("expected.txt"))
        .map_err(|e| format!("expected.txt fehlt: {e}"))?;
    if actual != expected {
        return Err(format!("Ausgabe weicht ab\n  erwartet: {expected:?}\n  erhalten: {actual:?}"));
    }
    Ok(())
}

#[test]
fn acceptance() {
    let work = std::env::temp_dir().join("abapc-acceptance");
    fs::create_dir_all(&work).unwrap();

    let mut dirs: Vec<PathBuf> = fs::read_dir(cases_dir())
        .expect("tests/acceptance fehlt")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.join("prog.abap").exists())
        .collect();
    dirs.sort();
    assert!(!dirs.is_empty(), "keine Testfälle gefunden");

    let mut failures = Vec::new();
    for dir in &dirs {
        if let Err(msg) = run_case(dir, &work) {
            failures.push(format!("{}: {msg}", dir.file_name().unwrap().to_string_lossy()));
        }
    }
    let passed = dirs.len() - failures.len();
    println!("{passed}/{} Akzeptanztests bestanden", dirs.len());
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
