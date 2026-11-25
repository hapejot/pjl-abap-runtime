//! abapc: übersetzt ein ABAP-Report-Programm in eine ausführbare Datei.
//!
//! Aufruf: abapc <prog.abap> -o <exe> [--emit-ir <datei.ll>] [--runtime <runtime-lib>]

use abap_compiler::{ast::CompileError, codegen::Codegen, lower::parse_source};
use clap::Parser;
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
struct Args {
    /// Eingabe-ABAP-Datei
    input: PathBuf,
    /// Ausgabedatei
    #[clap(short = 'o', long = "output")]
    output: PathBuf,
    /// Optional: IR-Datei ausgeben
    #[clap(long = "emit-ir")]
    emit_ir: Option<PathBuf>,
    /// Optional: Laufzeitbibliothek
    #[clap(long = "runtime")]
    runtime: Option<PathBuf>,
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
    eprintln!("abapc: {} Fehler gefunden", errors.len());
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
        .ok_or("Zielmaschine konnte nicht erstellt werden")?;
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
        .map_err(|e| format!("Linker 'link.exe' nicht startbar: {e}"))?;

    #[cfg(not(target_env = "msvc"))]
    let output = Command::new("cc")
        .arg(obj)
        .arg(runtime)
        .arg("-o")
        .arg(exe)
        .args(["-lpthread", "-ldl", "-lm", "-lgcc_s"])
        .output()
        .map_err(|e| format!("Linker 'cc' nicht startbar: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "Linken fehlgeschlagen:\n{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

fn main() {
    let args = Args::parse();
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
    let src = std::fs::read_to_string(&args.input)
        .unwrap_or_else(|e| fail(format!("{}: {e}", args.input.display())));

    let ir = match parse_source(&src) {
        Ok(ir) => ir,
        Err(errors) => fail_all(errors),
    };

    let ctx = Context::create();
    let mut cg = Codegen::new(&ctx);
    if let Err(errors) = cg.emit_program(&ir) {
        fail_all(errors);
    }
    if let Some(ll) = &args.emit_ir {
        if let Err(e) = cg.module.print_to_file(ll) {
            fail(format!("IR-Datei nicht schreibbar: {e}"));
        }
    }

    let obj = args.output.with_extension("o");
    if let Err(e) = emit_object(&cg.module, &obj) {
        fail(format!("Objektdatei: {e}"));
    }
    let runtime = args.runtime.clone().unwrap_or_else(default_runtime);
    if !runtime.exists() {
        fail(format!(
            "Laufzeitbibliothek nicht gefunden: {}",
            runtime.display()
        ));
    }
    if let Err(e) = link(&obj, &runtime, &args.output) {
        fail(e);
    }
    let _ = std::fs::remove_file(&obj);
    info!("Compilation successful");
}
