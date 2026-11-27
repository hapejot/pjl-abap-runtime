//! abapc: übersetzt ein ABAP-Report-Programm in eine ausführbare Datei und untersucht abapGit-Exporte.
//!
//! Aufruf:
//!   abapc <prog.abap> -o <exe> [--emit-ir <datei.ll>] [--runtime <runtime-lib>]
//!   abapc inspect <export.zip> [--check|--objects|--deps|--external|--model|--tui|...]

use abap_compiler::{
    analyze,
    ast::CompileError,
    codegen::Codegen,
    inspect, library,
    lower::{lower_program_with, parse_source},
    project,
};
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

fn main() {
    let cli = Cli::parse();
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
    let ir = if is_zip {
        // Hauptprogramm aus dem Export, INCLUDEs aus demselben Export aufgelöst
        let mut programs = project::load_programs(&args.input).unwrap_or_else(|e| fail(e));
        let name =
            project::main_program(&programs, args.program.as_deref()).unwrap_or_else(|e| fail(e));
        let mut prog = programs
            .remove(&name)
            .expect("main program is present in the export");
        project::expand_includes(&mut prog, &programs);
        let ddic = project::load_ddic(&args.input).unwrap_or_else(|e| fail(e));
        for skipped in &ddic.skipped {
            eprintln!("abapc: note: DDIC object skipped: {skipped}");
        }
        match lower_program_with(&prog, &ddic) {
            Ok(ir) => ir,
            Err(errors) => fail_all(errors),
        }
    } else {
        let src = std::fs::read_to_string(&args.input)
            .unwrap_or_else(|e| fail(format!("{}: {e}", args.input.display())));
        match parse_source(&src) {
            Ok(ir) => ir,
            Err(errors) => fail_all(errors),
        }
    };

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
