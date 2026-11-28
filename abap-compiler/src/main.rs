//! abapc: übersetzt ein ABAP-Report-Programm in eine ausführbare Datei und untersucht abapGit-Exporte.
//!
//! Aufruf:
//!   abapc <prog.abap> -o <exe> [--emit-ir <datei.ll>] [--runtime <runtime-lib>]
//!   abapc inspect <export.zip> [--check|--objects|--deps|--external|--model|--tui|...]

use abap_compiler::{
    analyze,
    ast::CompileError,
    codegen::Codegen,
    dynpro, inspect, library,
    lower::{lower_program_with, parse_source, Ir},
    project, sqlcheck,
};
use abap_rt::sql::adapter::Capabilities;
use abap_rt::sql::config::Config;
use clap::{Parser, Subcommand};
use inkwell::context::Context;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine,
};
use inkwell::OptimizationLevel;
use std::path::{Path, PathBuf};
use std::process::{self, Command};
use tracing::*;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[derive(Debug, Parser)]
#[command(name = "abapc", args_conflicts_with_subcommands = true)]
struct Cli {
    #[command(subcommand)]
    command: Option<Sub>,
    /// Eingabe-ABAP-Datei (Übersetzung)
    input: Option<PathBuf>,
    /// Ausgabedatei
    #[clap(short = 'o', long = "output")]
    output: Option<PathBuf>,
    /// Optional: IR-Datei ausgeben
    #[clap(long = "emit-ir")]
    emit_ir: Option<PathBuf>,
    /// Optional: Laufzeitbibliothek
    #[clap(long = "runtime")]
    runtime: Option<PathBuf>,
    /// Bei ZIP-Eingabe: Hauptprogramm aus dem Export (sonst das einzige Programm mit REPORT)
    #[clap(long = "program")]
    program: Option<String>,
    /// Dynpro-Beschreibung (.dynp). Ohne Angabe wird <name>.dynp neben <name>.abap gesucht.
    #[clap(long = "dynp")]
    dynp: Option<PathBuf>,
    /// Nur übersetzen bis zur SQL-IR: gibt die SQL-Anfragen als JSON-Liste auf stdout aus (kein Codegen)
    #[clap(long = "dump-sql")]
    dump_sql: Option<PathBuf>,
    /// Datenbank-Konfiguration (TOML): prüft die SQL-Anweisungen zur Übersetzungszeit gegen Tabellen,
    /// Verbindungen und Fähigkeiten. Ohne Angabe findet keine Prüfung statt.
    #[clap(long = "db-config")]
    db_config: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
enum Sub {
    /// abapGit-Export untersuchen: Objekte, Abhängigkeiten, externe Anforderungen, TUI
    Inspect(inspect::cli::Options),
    /// Übersetzungsfehler auswerten: unbekannte Namen, vermutete Art, Compiler-Lücken
    Analyze(analyze::Options),
    /// Externe Bibliotheken verwalten
    #[command(subcommand)]
    Library(LibraryCmd),
}

#[derive(Debug, Subcommand)]
enum LibraryCmd {
    /// Bibliothek aus Quelltext (Ordner oder ZIP) als JSON erzeugen
    Build(library::BuildOptions),
}

/// Argumente der Übersetzung, nachdem die Pflichtangaben geprüft sind.
struct Args {
    input: PathBuf,
    output: PathBuf,
    emit_ir: Option<PathBuf>,
    runtime: Option<PathBuf>,
    program: Option<String>,
    dynp: Option<PathBuf>,
    db_config: Option<PathBuf>,
}

/// Prüft die SQL-Anfragen des Programms gegen die Datenbank-Konfiguration (`--db-config`). Fehler gehen nach
/// Zeile sortiert nach stderr; ein Fehler beendet den Lauf.
fn check_database(ir: &Ir, path: &Path) {
    let config = Config::from_file(path).unwrap_or_else(|e| fail(e));
    let errors = sqlcheck::check_requests(&config, &ir.sql_requests, &Capabilities::sqlite());
    if !errors.is_empty() {
        fail_all(errors);
    }
}

/// Liest die Dynpro-Datei (`--dynp` oder `<name>.dynp` neben dem Programm), prüft sie gegen das Programm und
/// hängt sie an die IR. Fehler und Warnungen gehen nach stderr; ein Fehler beendet den Lauf.
fn attach_dynpro(ir: &mut Ir, input: &Path, dynp: Option<&Path>, is_zip: bool) {
    if is_zip {
        if dynp.is_some() {
            fail("--dynp is not supported with ZIP input");
        }
        if let Some(r) = ir.screen_refs.first() {
            fail(format!(
                "Line {}: {} is not supported with ZIP input (abapGit DYNP objects are not read)",
                r.line, r.what
            ));
        }
        return;
    }
    let path = match dynp {
        Some(p) => Some(p.to_path_buf()),
        None => Some(input.with_extension("dynp")).filter(|p| p.exists()),
    };
    let file = path.map(|p| {
        let src =
            std::fs::read_to_string(&p).unwrap_or_else(|e| fail(format!("{}: {e}", p.display())));
        dynpro::parse_dynp(&p.display().to_string(), &src).unwrap_or_else(|e| fail(e))
    });
    let abap = input.display().to_string();
    let diags = dynpro::check(file.as_ref(), ir, &abap);
    let mut errors = 0;
    for d in &diags {
        eprintln!("abapc: {d}");
        if !d.warning {
            errors += 1;
        }
    }
    if errors > 0 {
        fail(format!("{errors} error(s) found"));
    }
    ir.screens = file;
}

fn runtime_library_name() -> &'static str {
    if cfg!(target_env = "msvc") {
        "abap_rt.lib"
    } else {
        "libabap_rt.a"
    }
}

fn default_runtime() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(runtime_library_name())))
        .unwrap_or_else(|| PathBuf::from(runtime_library_name()))
}

/// Gibt alle gesammelten Fehler nach Zeile sortiert aus und beendet das Programm.
fn fail_all(mut errors: Vec<CompileError>) -> ! {
    errors.sort_by_key(|e| e.line);
    for e in &errors {
        eprintln!("abapc: {e}");
    }
    eprintln!("abapc: {} error(s) found", errors.len());
    process::exit(1);
}

fn fail(msg: impl std::fmt::Display) -> ! {
    eprintln!("abapc: {msg}");
    process::exit(1);
}

fn emit_object(module: &inkwell::module::Module, path: &Path) -> Result<(), String> {
    Target::initialize_native(&InitializationConfig::default()).map_err(|e| e.to_string())?;
    let triple = TargetMachine::get_default_triple();
    let target = Target::from_triple(&triple).map_err(|e| e.to_string())?;
    let cpu = TargetMachine::get_host_cpu_name();
    let features = TargetMachine::get_host_cpu_features();
    let tm = target
        .create_target_machine(
            &triple,
            cpu.to_str().unwrap_or("generic"),
            features.to_str().unwrap_or(""),
            OptimizationLevel::Default,
            RelocMode::PIC,
            CodeModel::Default,
        )
        .ok_or("Could not create target machine")?;
    module.set_triple(&triple);
    module.set_data_layout(&tm.get_target_data().get_data_layout());
    tm.write_to_file(module, FileType::Object, path)
        .map_err(|e| e.to_string())
}

/// Sucht link.exe samt LIB/PATH über die Visual-Studio-Installation,
/// sodass keine Developer-Shell nötig ist; Fallback ist link.exe im PATH.
#[cfg(target_env = "msvc")]
fn msvc_linker() -> Command {
    let arch = if cfg!(target_arch = "aarch64") {
        "aarch64-pc-windows-msvc"
    } else {
        "x86_64-pc-windows-msvc"
    };
    cc::windows_registry::find(arch, "link.exe")
        .map(Command::from)
        .unwrap_or_else(|| Command::new("link.exe"))
}

fn link(obj: &Path, runtime: &Path, exe: &Path) -> Result<(), String> {
    #[cfg(target_env = "msvc")]
    let output = msvc_linker()
        .arg("/NOLOGO")
        .arg("/SUBSYSTEM:CONSOLE")
        .arg(format!("/OUT:{}", exe.display()))
        .arg(obj)
        .arg(runtime)
        .args([
            "kernel32.lib",
            "ntdll.lib",
            "userenv.lib",
            "ws2_32.lib",
            "dbghelp.lib",
            "user32.lib",
            "/DEFAULTLIB:msvcrt",
        ])
        .output()
        .map_err(|e| format!("Cannot start linker 'link.exe': {e}"))?;

    #[cfg(not(target_env = "msvc"))]
    let output = Command::new("cc")
        .arg(obj)
        .arg(runtime)
        .arg("-o")
        .arg(exe)
        .args(["-lpthread", "-ldl", "-lm", "-lgcc_s"])
        .output()
        .map_err(|e| format!("Cannot start linker 'cc': {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "Linking failed:\n{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

/// Übersetzt die Eingabe bis zur IR: Hauptprogramm aus einem ZIP-Export mit DDIC, sonst eine ABAP-Datei.
fn lower_input(input: &Path, is_zip: bool, program: Option<&str>) -> Ir {
    if is_zip {
        // Hauptprogramm aus dem Export, INCLUDEs aus demselben Export aufgelöst
        let mut programs = project::load_programs(input).unwrap_or_else(|e| fail(e));
        let name = project::main_program(&programs, program).unwrap_or_else(|e| fail(e));
        let mut prog = programs
            .remove(&name)
            .expect("main program is present in the export");
        project::expand_includes(&mut prog, &programs);
        let ddic = project::load_ddic(input).unwrap_or_else(|e| fail(e));
        for skipped in &ddic.skipped {
            eprintln!("abapc: note: DDIC object skipped: {skipped}");
        }
        match lower_program_with(&prog, &ddic) {
            Ok(ir) => ir,
            Err(errors) => fail_all(errors),
        }
    } else {
        let src = std::fs::read_to_string(input)
            .unwrap_or_else(|e| fail(format!("{}: {e}", input.display())));
        match parse_source(&src) {
            Ok(ir) => ir,
            Err(errors) => fail_all(errors),
        }
    }
}

fn main() {
    let cli = Cli::parse();
    if let Some(path) = &cli.dump_sql {
        // Nur bis zur IR übersetzen und die SQL-Anfragen als JSON ausgeben (kein Codegen, kein Linken)
        let is_zip = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("zip"));
        let ir = lower_input(path, is_zip, cli.program.as_deref());
        let json = serde_json::to_string_pretty(&ir.sql_requests).unwrap_or_else(|e| fail(e));
        println!("{json}");
        return;
    }
    match cli.command {
        Some(Sub::Inspect(opts)) => {
            inspect::cli::run(opts);
            return;
        }
        Some(Sub::Analyze(opts)) => {
            analyze::run(opts);
            return;
        }
        Some(Sub::Library(LibraryCmd::Build(opts))) => {
            if let Err(e) = library::build(opts) {
                fail(e);
            }
            return;
        }
        None => {}
    }
    let (Some(input), Some(output)) = (cli.input, cli.output) else {
        fail("Specify input file and -o <output> (analysis: abapc inspect <zip>)")
    };
    let is_zip = input
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("zip"));
    let args = Args {
        input,
        output,
        emit_ir: cli.emit_ir,
        runtime: cli.runtime,
        program: cli.program,
        dynp: cli.dynp,
        db_config: cli.db_config,
    };
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();
    info!("ABAPC starting");
    info!(
        "Translating {} into {}",
        args.input.display(),
        args.output.display()
    );
    let mut ir = lower_input(&args.input, is_zip, args.program.as_deref());

    if let Some(path) = &args.db_config {
        check_database(&ir, path);
    }
    attach_dynpro(&mut ir, &args.input, args.dynp.as_deref(), is_zip);

    let ctx = Context::create();
    let mut cg = Codegen::new(&ctx);
    if let Err(errors) = cg.emit_program(&ir) {
        fail_all(errors);
    }
    if let Some(ll) = &args.emit_ir {
        if let Err(e) = cg.module.print_to_file(ll) {
            fail(format!("IR file not writable: {e}"));
        }
    }

    let obj = args.output.with_extension("o");
    if let Err(e) = emit_object(&cg.module, &obj) {
        fail(format!("Object file: {e}"));
    }
    let runtime = args.runtime.clone().unwrap_or_else(default_runtime);
    if !runtime.exists() {
        fail(format!("runtime library not found: {}", runtime.display()));
    }
    if let Err(e) = link(&obj, &runtime, &args.output) {
        fail(e);
    }
    let _ = std::fs::remove_file(&obj);
    info!("Compilation successful");
}
